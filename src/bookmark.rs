use crate::error::{Error, Result};
use crate::jj::{Bookmark, Jujutsu};
use std::collections::{HashMap, HashSet};

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
    /// Build a bookmark graph from a jujutsu repository
    pub async fn build(jj: &Jujutsu, default_branch: &str) -> Result<Self> {
        // Get all bookmarks
        let bookmarks_list = jj.get_bookmarks()?;

        // Filter to only local bookmarks (not remote-tracking ones)
        let local_bookmarks: Vec<_> = bookmarks_list.into_iter().filter(|b| b.is_local).collect();

        // Build a map of bookmarks by name
        let mut bookmarks = HashMap::new();
        for bookmark in &local_bookmarks {
            bookmarks.insert(bookmark.name.clone(), bookmark.clone());
        }

        // Build adjacency list by finding parent relationships
        let mut adjacency_list = HashMap::new();

        for bookmark in &local_bookmarks {
            // Get the commit for this bookmark
            let changes = jj.get_changes(&bookmark.commit_id, &bookmark.commit_id)?;

            if let Some(change) = changes.first() {
                // Detect merge commits (multiple parents) on the bookmark's commit
                if change.parent_ids.len() > 1 {
                    return Err(Error::InvalidGraph {
                        message: format!(
                            "Bookmark '{}' points to a merge commit with {} parents. \
                             Merge commits are not supported in bookmark stacks. \
                             Please use a linear history for stacked MRs.",
                            bookmark.name,
                            change.parent_ids.len()
                        ),
                    });
                }

                // Check all ancestors for merge commits
                // We need to traverse the full history to catch merges at any depth
                Self::validate_no_merges_in_ancestors(
                    jj,
                    &bookmark.name,
                    &change.parent_ids,
                    &local_bookmarks,
                )?;

                // For each parent commit, traverse ancestry to find nearest bookmark
                for parent_id in &change.parent_ids {
                    if let Some(parent_bookmark_name) =
                        Self::find_nearest_bookmarked_ancestor(jj, parent_id, &local_bookmarks)?
                    {
                        adjacency_list.insert(bookmark.name.clone(), parent_bookmark_name);
                        break;
                    }
                }
            }
        }

        // Build stacks by traversing the graph
        let stacks = Self::build_stacks(&bookmarks, &adjacency_list, default_branch);

        Ok(Self {
            bookmarks,
            adjacency_list,
            stacks,
        })
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

        loop {
            // Avoid infinite loops
            if visited.contains(&current_id) {
                return Ok(None);
            }
            visited.insert(current_id.clone());

            // Check if current commit has a bookmark
            if let Some(bookmark_name) = bookmarked_commits.get(current_id.as_str()) {
                return Ok(Some(bookmark_name.to_string()));
            }

            // Get parent commits
            let changes = jj.get_changes(&current_id, &current_id)?;
            if let Some(change) = changes.first() {
                if change.parent_ids.is_empty() {
                    // Reached root with no bookmark
                    return Ok(None);
                }
                // Follow first parent
                current_id = change.parent_ids[0].clone();
            } else {
                // No change found
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
}
