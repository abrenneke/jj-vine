use crate::error::{Error, Result};
use crate::jj::{Bookmark, Jujutsu};
use std::collections::{HashMap, HashSet};
use tracing::{debug, trace, warn};

/// Bookmark dependency graph
///
/// Represents the relationships between bookmarks in a jujutsu repository
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
        let all_bookmarks: Vec<_> = self.bookmarks.values().cloned().collect();

        for bookmark_name in bookmarks {
            let bookmark =
                self.bookmarks
                    .get(bookmark_name)
                    .ok_or_else(|| Error::BookmarkNotFound {
                        name: bookmark_name.clone(),
                    })?;

            // Get the commit for this bookmark
            let changes = jj.get_changes(&bookmark.commit_id, &bookmark.commit_id)?;

            if let Some(change) = changes.first() {
                // Check if bookmark itself is a merge commit
                if change.parent_ids.len() > 1 {
                    return Err(Error::InvalidGraph {
                        message: format!(
                            "Bookmark '{}' points to a merge commit with {} parents. \
                             Merge commits are not supported in bookmark stacks. \
                             Please use a linear history for stacked MRs.",
                            bookmark_name,
                            change.parent_ids.len()
                        ),
                    });
                }

                // Check ancestors for merge commits
                Self::validate_no_merges_in_ancestors(
                    jj,
                    bookmark_name,
                    &change.parent_ids,
                    &all_bookmarks,
                )?;
            }
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
        // OR bookmarks that don't appear in the adjacency list at all (isolated bookmarks)
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
    /// Returns bookmarks from the root of the stack up to and including the target bookmark
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
    /// Returns bookmarks ordered such that parent bookmarks appear before their children.
    /// Handles disconnected bookmarks gracefully.
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
    /// Traverses the commit ancestry until finding a commit with a bookmark.
    /// Returns the name of the first bookmark found, or None if no bookmarked
    /// ancestor exists.
    fn find_nearest_bookmarked_ancestor(
        jj: &Jujutsu,
        start_commit_id: &str,
        bookmarks: &[Bookmark],
    ) -> Result<Option<String>> {
        use std::collections::HashSet;
        let mut visited = HashSet::new();
        let bookmarked_commits: HashMap<&str, &str> = bookmarks
            .iter()
            .map(|b| (b.commit_id.as_str(), b.name.as_str()))
            .collect();

        let mut current_id = start_commit_id.to_string();
        let mut steps = 0;

        loop {
            steps += 1;
            if steps % 100 == 0 {
                warn!(
                    "find_nearest_bookmarked_ancestor: {} steps traversed",
                    steps
                );
            }

            // Avoid infinite loops
            if visited.contains(&current_id) {
                debug!(
                    "find_nearest_bookmarked_ancestor: reached cycle after {} steps",
                    steps
                );
                return Ok(None);
            }
            visited.insert(current_id.clone());

            // Check if current commit has a bookmark
            if let Some(bookmark_name) = bookmarked_commits.get(current_id.as_str()) {
                debug!(
                    "find_nearest_bookmarked_ancestor: found bookmark '{}' after {} steps",
                    bookmark_name, steps
                );
                return Ok(Some(bookmark_name.to_string()));
            }

            // Get parent commits
            trace!(
                "find_nearest_bookmarked_ancestor: getting changes for commit {} (step {})",
                &current_id[..8],
                steps
            );
            let changes = jj.get_changes(&current_id, &current_id)?;
            if let Some(change) = changes.first() {
                if change.parent_ids.is_empty() {
                    // Reached root with no bookmark
                    debug!(
                        "find_nearest_bookmarked_ancestor: reached root after {} steps",
                        steps
                    );
                    return Ok(None);
                }
                // Follow first parent
                current_id = change.parent_ids[0].clone();
            } else {
                // No change found
                debug!(
                    "find_nearest_bookmarked_ancestor: no change found after {} steps",
                    steps
                );
                return Ok(None);
            }
        }
    }

    /// Validate that no merge commits exist in the ancestor chain
    ///
    /// Recursively checks all ancestor commits until reaching a bookmarked commit
    /// or the root. This catches merge commits at any depth in the history.
    fn validate_no_merges_in_ancestors(
        jj: &Jujutsu,
        bookmark_name: &str,
        parent_ids: &[String],
        bookmarks: &[Bookmark],
    ) -> Result<()> {
        use std::collections::HashSet;
        let mut visited = HashSet::new();
        let bookmarked_commits: HashSet<_> =
            bookmarks.iter().map(|b| b.commit_id.as_str()).collect();

        for parent_id in parent_ids {
            Self::check_ancestors_recursive(
                jj,
                bookmark_name,
                parent_id,
                &bookmarked_commits,
                &mut visited,
            )?;
        }
        Ok(())
    }

    /// Recursively check ancestors for merge commits
    fn check_ancestors_recursive(
        jj: &Jujutsu,
        original_bookmark: &str,
        commit_id: &str,
        bookmarked_commits: &HashSet<&str>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        // Stop if we've already checked this commit
        if visited.contains(commit_id) {
            return Ok(());
        }
        visited.insert(commit_id.to_string());

        // Stop if this commit has a bookmark (different stack)
        if bookmarked_commits.contains(commit_id) {
            return Ok(());
        }

        // Get the commit's parents
        let changes = jj.get_changes(commit_id, commit_id)?;
        if let Some(change) = changes.first() {
            // Check if this is a merge commit
            if change.parent_ids.len() > 1 {
                return Err(Error::InvalidGraph {
                    message: format!(
                        "Bookmark '{}' has an ancestor commit that is a merge with {} parents. \
                         Merge commits are not supported in bookmark stacks. \
                         Please use a linear history for stacked MRs.",
                        original_bookmark,
                        change.parent_ids.len()
                    ),
                });
            }

            // Recursively check parents
            for parent_id in &change.parent_ids {
                Self::check_ancestors_recursive(
                    jj,
                    original_bookmark,
                    parent_id,
                    bookmarked_commits,
                    visited,
                )?;
            }
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
        // Create a branching structure: feature-a has two children (feature-b and alt-feature)
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
        use crate::jj::{Jujutsu, run_jj_command};
        use std::path::PathBuf;
        use std::process::Command as StdCommand;
        use tempfile::TempDir;

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
        run_jj_command(&repo_path, &["describe", "-m", "initial"]).expect("Failed to describe");

        // Create first branch
        run_jj_command(&repo_path, &["bookmark", "create", "branch1"])
            .expect("Failed to create branch1");
        run_jj_command(&repo_path, &["new"]).expect("Failed to create new commit");
        run_jj_command(&repo_path, &["describe", "-m", "branch1-commit"])
            .expect("Failed to describe");

        // Go back and create second branch
        run_jj_command(&repo_path, &["new", "branch1-"]).expect("Failed to checkout parent");
        run_jj_command(&repo_path, &["bookmark", "create", "branch2"])
            .expect("Failed to create branch2");
        run_jj_command(&repo_path, &["describe", "-m", "branch2-commit"])
            .expect("Failed to describe");

        // Create merge commit
        let branch1_id = run_jj_command(
            &repo_path,
            &["log", "-r", "branch1", "--no-graph", "-T", "commit_id"],
        )
        .expect("Failed to get branch1 id");
        let branch2_id = run_jj_command(
            &repo_path,
            &["log", "-r", "branch2", "--no-graph", "-T", "commit_id"],
        )
        .expect("Failed to get branch2 id");

        run_jj_command(&repo_path, &["new", branch1_id.trim(), branch2_id.trim()])
            .expect("Failed to create merge");
        run_jj_command(&repo_path, &["describe", "-m", "merge-commit"])
            .expect("Failed to describe merge");
        run_jj_command(&repo_path, &["bookmark", "create", "wip"])
            .expect("Failed to create wip bookmark");

        // Create a normal linear bookmark
        run_jj_command(&repo_path, &["new", "root()"]).expect("Failed to create new change");
        run_jj_command(&repo_path, &["describe", "-m", "feature-a-commit"])
            .expect("Failed to describe");
        run_jj_command(&repo_path, &["bookmark", "create", "feature-a"])
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
