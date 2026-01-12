use std::collections::{HashMap, HashSet};

use tracing::{debug, trace};

use crate::{
    error::{Error, Result},
    jj::{Bookmark, Jujutsu},
};

/// Bookmark dependency graph
///
/// Represents the relationships between bookmarks in a jujutsu repository
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkGraph {
    /// All bookmarks in the repository (keyed by name)
    pub bookmarks: HashMap<String, Bookmark>,

    /// Adjacency list representing parent relationships
    /// Maps child bookmark name -> parent bookmark name
    pub adjacency_list: HashMap<String, String>,

    /// Stacks of related bookmarks
    pub stacks: Vec<BranchStack>,
}

/// A stack of related bookmarks
#[derive(Debug, Clone, PartialEq)]
pub struct BranchStack {
    /// Bookmarks in this stack, ordered from root to leaf
    pub bookmarks: Vec<String>,

    /// The base branch this stack builds on (e.g., "main")
    pub base: String,
}

impl BookmarkGraph {
    /// Build a bookmark graph from a list of bookmarks
    ///
    /// # Arguments
    ///
    /// * `jj` - Jujutsu instance
    /// * `default_branch` - The default branch name (e.g., "main", "master")
    /// * `bookmarks_list` - List of bookmarks to include in the graph
    pub async fn build(
        jj: &Jujutsu,
        default_branch: &str,
        bookmarks_list: Vec<Bookmark>,
    ) -> Result<Self> {
        debug!("Building graph for {} bookmarks", bookmarks_list.len());

        // Filter to only local bookmarks (not remote-tracking ones)
        let local_bookmarks: Vec<_> = bookmarks_list.into_iter().filter(|b| b.is_local).collect();
        debug!("Filtered to {} local bookmarks", local_bookmarks.len());

        // Build a map of bookmarks by name
        let mut bookmarks = HashMap::new();
        for bookmark in &local_bookmarks {
            bookmarks.insert(bookmark.name.clone(), bookmark.clone());
        }

        // Build adjacency list by finding parent relationships
        let mut adjacency_list = HashMap::new();

        debug!(
            "Building adjacency list for {} bookmarks",
            local_bookmarks.len()
        );
        for (i, bookmark) in local_bookmarks.iter().enumerate() {
            // Skip the default branch - it has no parent bookmark by definition
            if bookmark.name == default_branch {
                debug!(
                    "Skipping default branch '{}' during adjacency list building",
                    default_branch
                );
                continue;
            }

            debug!(
                "Processing bookmark {}/{}: {}",
                i + 1,
                local_bookmarks.len(),
                bookmark.name
            );
            // Get the commit for this bookmark
            let changes = jj.get_changes(&bookmark.commit_id, &bookmark.commit_id)?;

            if let Some(change) = changes.first() {
                // For each parent commit, traverse ancestry to find nearest bookmark
                for parent_id in &change.parent_ids {
                    debug!(
                        "Finding nearest bookmarked ancestor for bookmark '{}' starting from parent {}",
                        bookmark.name, parent_id
                    );
                    if let Some(parent_bookmark_name) =
                        Self::find_nearest_bookmarked_ancestor(jj, parent_id, &local_bookmarks)?
                    {
                        debug!(
                            "Found parent bookmark '{}' for '{}'",
                            parent_bookmark_name, bookmark.name
                        );
                        adjacency_list.insert(bookmark.name.clone(), parent_bookmark_name);
                        break;
                    } else {
                        debug!("No bookmarked ancestor found for '{}'", bookmark.name);
                    }
                }
            }
        }

        // Build stacks by traversing the graph
        debug!("Building stacks");
        let stacks = Self::build_stacks(&bookmarks, &adjacency_list, default_branch);
        debug!("Built {} stacks", stacks.len());

        Ok(Self {
            bookmarks,
            adjacency_list,
            stacks,
        })
    }

    /// Validate that bookmarks have linear history (no merge commits)
    ///
    /// This should be called after building the graph, and only for bookmarks
    /// that will be submitted as MRs. This allows the graph to be built for
    /// all bookmarks (including those with merge commits) while only validating
    /// the ones that need to follow MR submission rules.
    pub fn validate_bookmarks(&self, jj: &Jujutsu, bookmarks: &[String]) -> Result<()> {
        for bookmark_name in bookmarks {
            let bookmark =
                self.bookmarks
                    .get(bookmark_name)
                    .ok_or_else(|| Error::BookmarkNotFound {
                        name: bookmark_name.clone(),
                    })?;

            // Check for merge commits in the new commits (not in trunk)
            Self::validate_no_merges_in_ancestors(jj, bookmark_name, &bookmark.commit_id)?;
        }

        Ok(())
    }

    /// Build stacks from the bookmark graph
    fn build_stacks(
        bookmarks: &HashMap<String, Bookmark>,
        adjacency_list: &HashMap<String, String>,
        default_branch: &str,
    ) -> Vec<BranchStack> {
        let mut stacks = Vec::new();

        // Find all parent bookmarks (bookmarks that appear as values in adjacency_list)
        let parents: HashSet<_> = adjacency_list.values().cloned().collect();

        // Leaves are bookmarks that are not parents (i.e., they have no children)
        // OR bookmarks that don't appear in the adjacency list at all (isolated
        // bookmarks)
        for name in bookmarks.keys() {
            let is_leaf = !parents.contains(name);

            if is_leaf {
                // Build a stack by tracing back from this leaf to the root
                let mut stack_bookmarks = Vec::new();
                let mut current = name.clone();

                // Trace back to the root
                loop {
                    stack_bookmarks.push(current.clone());

                    // Find the parent of current
                    match adjacency_list.get(&current) {
                        Some(parent) => {
                            current = parent.clone();
                        }
                        None => break, // Reached the root
                    }
                }

                // Reverse to get root-to-leaf order
                stack_bookmarks.reverse();

                stacks.push(BranchStack {
                    bookmarks: stack_bookmarks,
                    base: default_branch.to_string(),
                });
            }
        }

        stacks
    }

    /// Find the stack containing a specific bookmark
    pub fn find_stack_for_bookmark(&self, bookmark_name: &str) -> Option<&BranchStack> {
        self.stacks
            .iter()
            .find(|stack| stack.bookmarks.contains(&bookmark_name.to_string()))
    }

    /// Get all bookmarks in the downstack of a given bookmark (inclusive)
    ///
    /// Returns bookmarks from the root of the stack up to and including the
    /// target bookmark
    pub fn get_downstack(&self, bookmark_name: &str) -> Result<Vec<String>> {
        let stack =
            self.find_stack_for_bookmark(bookmark_name)
                .ok_or_else(|| Error::BookmarkNotFound {
                    name: bookmark_name.to_string(),
                })?;

        // Find the position of the bookmark in the stack
        let pos = stack
            .bookmarks
            .iter()
            .position(|b| b == bookmark_name)
            .ok_or_else(|| Error::BookmarkNotFound {
                name: bookmark_name.to_string(),
            })?;

        // Return all bookmarks from the start up to and including this position
        Ok(stack.bookmarks[..=pos].to_vec())
    }

    /// Get the parent bookmark of a given bookmark
    pub fn get_parent(&self, bookmark_name: &str) -> Option<&String> {
        self.adjacency_list.get(bookmark_name)
    }

    /// Sort bookmarks in topological order (dependencies first)
    ///
    /// Returns bookmarks ordered such that parent bookmarks appear before their
    /// children. Handles disconnected bookmarks gracefully.
    pub fn topological_sort(&self, bookmarks: &[String]) -> Result<Vec<String>> {
        use std::collections::{HashMap, HashSet};

        if bookmarks.is_empty() {
            return Ok(Vec::new());
        }

        // Build a set of bookmarks we're sorting for quick lookup
        let bookmark_set: HashSet<_> = bookmarks.iter().collect();

        // Build in-degree map (count of dependencies) for the given bookmarks
        let mut in_degree: HashMap<&String, usize> = HashMap::new();
        let mut children: HashMap<&String, Vec<&String>> = HashMap::new();

        // Initialize all bookmarks with in-degree 0
        for bookmark in bookmarks {
            in_degree.entry(bookmark).or_insert(0);
        }

        // Build the graph for only the bookmarks we're sorting
        for bookmark in bookmarks {
            if let Some(parent) = self.adjacency_list.get(bookmark) {
                // Only count the parent if it's in our list of bookmarks to sort
                if bookmark_set.contains(parent) {
                    *in_degree.entry(bookmark).or_insert(0) += 1;
                    children.entry(parent).or_default().push(bookmark);
                }
            }
        }

        // Kahn's algorithm: process bookmarks with no dependencies
        let mut queue: Vec<&String> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(bookmark, _)| *bookmark)
            .collect();

        // Sort the initial queue for deterministic output
        queue.sort();

        let mut result = Vec::new();

        while let Some(current) = queue.pop() {
            result.push(current.clone());

            // Process all children of current
            if let Some(child_list) = children.get(current) {
                for &child in child_list {
                    if let Some(degree) = in_degree.get_mut(child) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(child);
                            queue.sort(); // Keep sorted for deterministic output
                        }
                    }
                }
            }
        }

        // Check if all bookmarks were processed (no cycles)
        if result.len() != bookmarks.len() {
            return Err(Error::InvalidGraph {
                message: "Circular dependency detected in bookmarks".to_string(),
            });
        }

        Ok(result)
    }

    /// Find the nearest bookmarked ancestor starting from a given commit
    ///
    /// Uses revset queries to efficiently find bookmarked ancestors that are
    /// not part of trunk's history. Returns the name of the nearest
    /// bookmark found, or None if the commit is based on trunk or has no
    /// bookmarked ancestors.
    fn find_nearest_bookmarked_ancestor(
        jj: &Jujutsu,
        start_commit_id: &str,
        bookmarks: &[Bookmark],
    ) -> Result<Option<String>> {
        // Query for bookmarked ancestors that are not part of trunk's history
        // Format: "ancestors of start_commit, excluding ancestors of trunk, that have
        // bookmarks"
        let revset = format!("::{} ~ ::trunk() & bookmarks()", start_commit_id);
        debug!(
            "find_nearest_bookmarked_ancestor: querying revset: {}",
            revset
        );

        let candidates = jj.get_bookmarks_with_revset(&revset)?;

        // Filter to only include bookmarks from our provided list
        let bookmark_names: HashSet<_> = bookmarks.iter().map(|b| b.name.as_str()).collect();
        let candidates: Vec<_> = candidates
            .into_iter()
            .filter(|b| bookmark_names.contains(b.name.as_str()))
            .collect();

        debug!(
            "find_nearest_bookmarked_ancestor: found {} candidate bookmarks",
            candidates.len()
        );

        match candidates.len() {
            0 => {
                debug!(
                    "find_nearest_bookmarked_ancestor: no bookmarked ancestors found (based on trunk)"
                );
                Ok(None)
            }
            1 => {
                let bookmark_name = &candidates[0].name;
                debug!(
                    "find_nearest_bookmarked_ancestor: found single bookmark '{}'",
                    bookmark_name
                );
                Ok(Some(bookmark_name.clone()))
            }
            _ => {
                // Multiple candidates - find the nearest one by counting commits
                debug!("find_nearest_bookmarked_ancestor: multiple candidates, finding nearest");

                let mut nearest_bookmark: Option<(String, usize)> = None;

                for candidate in &candidates {
                    // Count commits between the candidate and start_commit
                    // Using revset: "candidate..start_commit" (exclusive on left, inclusive on
                    // right)
                    let distance_revset = format!("{}..{}", candidate.commit_id, start_commit_id);
                    let distance = jj.count_commits_in_revset(&distance_revset)?;

                    trace!(
                        "find_nearest_bookmarked_ancestor: bookmark '{}' has distance {}",
                        candidate.name, distance
                    );

                    match &nearest_bookmark {
                        None => {
                            nearest_bookmark = Some((candidate.name.clone(), distance));
                        }
                        Some((_, current_min_distance)) if distance < *current_min_distance => {
                            nearest_bookmark = Some((candidate.name.clone(), distance));
                        }
                        _ => {}
                    }
                }

                if let Some((bookmark_name, distance)) = nearest_bookmark {
                    debug!(
                        "find_nearest_bookmarked_ancestor: nearest bookmark is '{}' at distance {}",
                        bookmark_name, distance
                    );
                    Ok(Some(bookmark_name))
                } else {
                    debug!("find_nearest_bookmarked_ancestor: no nearest bookmark found");
                    Ok(None)
                }
            }
        }
    }

    /// Validate that no merge commits exist in the new commits
    ///
    /// Only checks commits in (::bookmark ~ ::trunk()), which are the new
    /// commits not in trunk's history. This matches how jj-stack handles
    /// merge validation.
    fn validate_no_merges_in_ancestors(
        jj: &Jujutsu,
        bookmark_name: &str,
        bookmark_commit_id: &str,
    ) -> Result<()> {
        // Query for merge commits in the new commits only
        // Uses jj's built-in trunk() revset which resolves to the trunk branch
        let revset = format!("(::{}  ~ ::trunk()) & merges()", bookmark_commit_id);

        let output = jj.run_captured(&[
            "log",
            "-r",
            &revset,
            "--no-graph",
            "-T",
            r#"commit_id ++ " parents: " ++ parents.len()"#,
        ])?;

        if !output.stdout.trim().is_empty() {
            // Found merge commits in the new commits
            return Err(Error::InvalidGraph {
                message: format!(
                    "Bookmark '{}' has an ancestor commit that is a merge with multiple parents. \
                     Merge commits are not supported in bookmark stacks. \
                     Please use a linear history for stacked MRs.",
                    bookmark_name
                ),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_stacks_simple_linear() {
        let mut bookmarks = HashMap::new();
        bookmarks.insert(
            "feature-a".to_string(),
            Bookmark {
                name: "feature-a".to_string(),
                commit_id: "commit1".to_string(),
                change_id: "change1".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-b".to_string(),
            Bookmark {
                name: "feature-b".to_string(),
                commit_id: "commit2".to_string(),
                change_id: "change2".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );

        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());

        let stacks = BookmarkGraph::build_stacks(&bookmarks, &adjacency_list, "main");

        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].bookmarks, vec!["feature-a", "feature-b"]);
        assert_eq!(stacks[0].base, "main");
    }

    #[test]
    fn test_build_stacks_multiple() {
        let mut bookmarks = HashMap::new();
        bookmarks.insert(
            "feature-a".to_string(),
            Bookmark {
                name: "feature-a".to_string(),
                commit_id: "commit1".to_string(),
                change_id: "change1".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-b".to_string(),
            Bookmark {
                name: "feature-b".to_string(),
                commit_id: "commit2".to_string(),
                change_id: "change2".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );

        // No adjacency (two independent stacks)
        let adjacency_list = HashMap::new();

        let stacks = BookmarkGraph::build_stacks(&bookmarks, &adjacency_list, "main");

        assert_eq!(stacks.len(), 2);
        assert_eq!(stacks[0].base, "main");
        assert_eq!(stacks[1].base, "main");
    }

    #[test]
    fn test_find_stack_for_bookmark() {
        let mut bookmarks = HashMap::new();
        bookmarks.insert(
            "feature-a".to_string(),
            Bookmark {
                name: "feature-a".to_string(),
                commit_id: "commit1".to_string(),
                change_id: "change1".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-b".to_string(),
            Bookmark {
                name: "feature-b".to_string(),
                commit_id: "commit2".to_string(),
                change_id: "change2".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );

        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());

        let graph = BookmarkGraph {
            bookmarks,
            adjacency_list,
            stacks: vec![BranchStack {
                bookmarks: vec!["feature-a".to_string(), "feature-b".to_string()],
                base: "main".to_string(),
            }],
        };

        let stack = graph.find_stack_for_bookmark("feature-b");
        assert!(stack.is_some());
        assert_eq!(stack.unwrap().bookmarks.len(), 2);
    }

    #[test]
    fn test_get_downstack() {
        let mut bookmarks = HashMap::new();
        bookmarks.insert(
            "feature-a".to_string(),
            Bookmark {
                name: "feature-a".to_string(),
                commit_id: "commit1".to_string(),
                change_id: "change1".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-b".to_string(),
            Bookmark {
                name: "feature-b".to_string(),
                commit_id: "commit2".to_string(),
                change_id: "change2".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-c".to_string(),
            Bookmark {
                name: "feature-c".to_string(),
                commit_id: "commit3".to_string(),
                change_id: "change3".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );

        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());
        adjacency_list.insert("feature-c".to_string(), "feature-b".to_string());

        let graph = BookmarkGraph {
            bookmarks,
            adjacency_list,
            stacks: vec![BranchStack {
                bookmarks: vec![
                    "feature-a".to_string(),
                    "feature-b".to_string(),
                    "feature-c".to_string(),
                ],
                base: "main".to_string(),
            }],
        };

        let downstack = graph.get_downstack("feature-b").unwrap();
        assert_eq!(downstack, vec!["feature-a", "feature-b"]);

        let downstack_c = graph.get_downstack("feature-c").unwrap();
        assert_eq!(downstack_c, vec!["feature-a", "feature-b", "feature-c"]);
    }

    #[test]
    fn test_get_parent() {
        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());

        let graph = BookmarkGraph {
            bookmarks: HashMap::new(),
            adjacency_list,
            stacks: Vec::new(),
        };

        assert_eq!(
            graph.get_parent("feature-b"),
            Some(&"feature-a".to_string())
        );
        assert_eq!(graph.get_parent("feature-a"), None);
    }

    #[test]
    fn test_build_stacks_with_branching() {
        // Create a branching structure: feature-a has two children (feature-b and
        // alt-feature)
        let mut bookmarks = HashMap::new();
        bookmarks.insert(
            "feature-a".to_string(),
            Bookmark {
                name: "feature-a".to_string(),
                commit_id: "commit1".to_string(),
                change_id: "change1".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "feature-b".to_string(),
            Bookmark {
                name: "feature-b".to_string(),
                commit_id: "commit2".to_string(),
                change_id: "change2".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );
        bookmarks.insert(
            "alt-feature".to_string(),
            Bookmark {
                name: "alt-feature".to_string(),
                commit_id: "commit3".to_string(),
                change_id: "change3".to_string(),
                remote: None,
                is_local: true,
                has_remote: false,
            },
        );

        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());
        adjacency_list.insert("alt-feature".to_string(), "feature-a".to_string());

        let stacks = BookmarkGraph::build_stacks(&bookmarks, &adjacency_list, "main");

        // Both branches should be in separate stacks
        assert_eq!(stacks.len(), 2);

        // Each stack should have the common ancestor
        for stack in &stacks {
            assert!(stack.bookmarks.contains(&"feature-a".to_string()));
        }

        // One stack should contain feature-b, the other alt-feature
        let has_feature_b = stacks
            .iter()
            .any(|s| s.bookmarks.contains(&"feature-b".to_string()));
        let has_alt_feature = stacks
            .iter()
            .any(|s| s.bookmarks.contains(&"alt-feature".to_string()));
        assert!(has_feature_b);
        assert!(has_alt_feature);

        // Create graph and verify both bookmarks can be found
        let graph = BookmarkGraph {
            bookmarks,
            adjacency_list,
            stacks,
        };

        assert!(graph.find_stack_for_bookmark("feature-b").is_some());
        assert!(graph.find_stack_for_bookmark("alt-feature").is_some());
    }

    #[test]
    fn test_topological_sort_single() {
        let graph = BookmarkGraph {
            bookmarks: HashMap::new(),
            adjacency_list: HashMap::new(),
            stacks: Vec::new(),
        };

        let bookmarks = vec!["feature-a".to_string()];
        let sorted = graph
            .topological_sort(&bookmarks)
            .expect("Failed to sort single bookmark");

        assert_eq!(sorted, vec!["feature-a"]);
    }

    #[test]
    fn test_topological_sort_chain() {
        // Create a linear chain: A -> B -> C
        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());
        adjacency_list.insert("feature-c".to_string(), "feature-b".to_string());

        let graph = BookmarkGraph {
            bookmarks: HashMap::new(),
            adjacency_list,
            stacks: Vec::new(),
        };

        let bookmarks = vec![
            "feature-c".to_string(),
            "feature-a".to_string(),
            "feature-b".to_string(),
        ];

        let sorted = graph
            .topological_sort(&bookmarks)
            .expect("Failed to sort chain");

        // Should be ordered A, B, C (dependencies first)
        assert_eq!(
            sorted,
            vec!["feature-a", "feature-b", "feature-c"],
            "Expected A -> B -> C order"
        );
    }

    #[test]
    fn test_topological_sort_complex() {
        // Create a complex DAG:
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());
        adjacency_list.insert("feature-c".to_string(), "feature-a".to_string());
        adjacency_list.insert("feature-d".to_string(), "feature-b".to_string());
        // Note: D could also depend on C, but for simplicity we just use B

        let graph = BookmarkGraph {
            bookmarks: HashMap::new(),
            adjacency_list,
            stacks: Vec::new(),
        };

        let bookmarks = vec![
            "feature-d".to_string(),
            "feature-a".to_string(),
            "feature-b".to_string(),
            "feature-c".to_string(),
        ];

        let sorted = graph
            .topological_sort(&bookmarks)
            .expect("Failed to sort complex DAG");

        // A should come first
        assert_eq!(sorted[0], "feature-a", "A should be first");

        // B and C should come after A but before D
        let a_pos = sorted.iter().position(|b| b == "feature-a").unwrap();
        let b_pos = sorted.iter().position(|b| b == "feature-b").unwrap();
        let c_pos = sorted.iter().position(|b| b == "feature-c").unwrap();
        let d_pos = sorted.iter().position(|b| b == "feature-d").unwrap();

        assert!(b_pos > a_pos, "B should come after A");
        assert!(c_pos > a_pos, "C should come after A");
        assert!(d_pos > b_pos, "D should come after B");
    }

    #[test]
    fn test_topological_sort_disconnected() {
        // Two independent chains: A -> B and X -> Y
        let mut adjacency_list = HashMap::new();
        adjacency_list.insert("feature-b".to_string(), "feature-a".to_string());
        adjacency_list.insert("feature-y".to_string(), "feature-x".to_string());

        let graph = BookmarkGraph {
            bookmarks: HashMap::new(),
            adjacency_list,
            stacks: Vec::new(),
        };

        let bookmarks = vec![
            "feature-b".to_string(),
            "feature-y".to_string(),
            "feature-a".to_string(),
            "feature-x".to_string(),
        ];

        let sorted = graph
            .topological_sort(&bookmarks)
            .expect("Failed to sort disconnected bookmarks");

        assert_eq!(sorted.len(), 4, "Should have all 4 bookmarks");

        // Within each chain, order should be preserved
        let a_pos = sorted.iter().position(|b| b == "feature-a").unwrap();
        let b_pos = sorted.iter().position(|b| b == "feature-b").unwrap();
        let x_pos = sorted.iter().position(|b| b == "feature-x").unwrap();
        let y_pos = sorted.iter().position(|b| b == "feature-y").unwrap();

        assert!(b_pos > a_pos, "B should come after A");
        assert!(y_pos > x_pos, "Y should come after X");
    }

    #[test]
    fn test_build_graph_with_merge_commits_succeeds() {
        use std::{path::PathBuf, process::Command as StdCommand};

        use tempfile::TempDir;

        use crate::jj::{Jujutsu, jj_exec};

        // Helper to create test repo
        fn create_test_repo() -> (TempDir, PathBuf) {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let repo_path = temp_dir.path().to_path_buf();

            // Initialize jj repo
            let output = StdCommand::new(crate::jj::which_jj().expect("jj not found"))
                .current_dir(&repo_path)
                .args(["git", "init", "--colocate"])
                .output()
                .expect("Failed to init jj repo");

            assert!(output.status.success(), "Failed to init jj repo");

            (temp_dir, repo_path)
        }

        let (_temp, repo_path) = create_test_repo();

        // Create initial commit
        jj_exec(&repo_path, ["describe", "-m", "initial"]).expect("Failed to describe");

        // Create first branch
        jj_exec(&repo_path, ["bookmark", "create", "branch1"]).expect("Failed to create branch1");
        jj_exec(&repo_path, ["new"]).expect("Failed to create new commit");
        jj_exec(&repo_path, ["describe", "-m", "branch1-commit"]).expect("Failed to describe");

        // Go back and create second branch
        jj_exec(&repo_path, ["new", "branch1-"]).expect("Failed to checkout parent");
        jj_exec(&repo_path, ["bookmark", "create", "branch2"]).expect("Failed to create branch2");
        jj_exec(&repo_path, ["describe", "-m", "branch2-commit"]).expect("Failed to describe");

        // Create merge commit
        let branch1_id = jj_exec(
            &repo_path,
            ["log", "-r", "branch1", "--no-graph", "-T", "commit_id"],
        )
        .expect("Failed to get branch1 id");
        let branch2_id = jj_exec(
            &repo_path,
            ["log", "-r", "branch2", "--no-graph", "-T", "commit_id"],
        )
        .expect("Failed to get branch2 id");

        jj_exec(
            &repo_path,
            ["new", branch1_id.stdout.trim(), branch2_id.stdout.trim()],
        )
        .expect("Failed to create merge");
        jj_exec(&repo_path, ["describe", "-m", "merge-commit"]).expect("Failed to describe merge");
        jj_exec(&repo_path, ["bookmark", "create", "wip"]).expect("Failed to create wip bookmark");

        // Create a normal linear bookmark
        jj_exec(&repo_path, ["new", "root()"]).expect("Failed to create new change");
        jj_exec(&repo_path, ["describe", "-m", "feature-a-commit"]).expect("Failed to describe");
        jj_exec(&repo_path, ["bookmark", "create", "feature-a"])
            .expect("Failed to create feature-a");

        let jj = Jujutsu::new(repo_path.clone()).unwrap();

        // Build graph - should succeed because validation is separate
        let bookmarks = jj.get_bookmarks().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let graph = runtime.block_on(BookmarkGraph::build(&jj, "main", bookmarks));

        // This assertion will fail with current code (which is what we want for TDD)
        // After implementing the fix, it should pass
        assert!(
            graph.is_ok(),
            "build() should succeed even with merge commits present"
        );

        let graph = graph.unwrap();

        // After the fix, validating only "feature-a" should succeed
        let result = graph.validate_bookmarks(&jj, &["feature-a".to_string()]);
        assert!(result.is_ok(), "Validating linear bookmark should succeed");

        // Validating "wip" should fail (it's a merge)
        let result = graph.validate_bookmarks(&jj, &["wip".to_string()]);
        assert!(
            result.is_err(),
            "Validating merge commit bookmark should fail"
        );
    }
}
