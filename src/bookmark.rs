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
    pub async fn build(jj: &Jujutsu) -> Result<Self> {
        // Get all bookmarks
        let bookmarks_list = jj.get_bookmarks()?;

        // Filter to only local bookmarks (not remote-tracking ones)
        let local_bookmarks: Vec<_> = bookmarks_list
            .into_iter()
            .filter(|b| b.is_local)
            .collect();

        // Build a map of bookmarks by name
        let mut bookmarks = HashMap::new();
        for bookmark in &local_bookmarks {
            bookmarks.insert(bookmark.name.clone(), bookmark.clone());
        }

        // Build adjacency list by finding parent relationships
        let mut adjacency_list = HashMap::new();

        for bookmark in &local_bookmarks {
            // Get the parent commits of this bookmark
            let changes = jj.get_changes(&bookmark.commit_id, &bookmark.commit_id)?;

            if let Some(change) = changes.first() {
                // For each parent commit, check if it has a bookmark
                for parent_id in &change.parent_ids {
                    // Find if any bookmark points to this parent
                    for potential_parent in &local_bookmarks {
                        if potential_parent.commit_id == *parent_id {
                            // Found a parent bookmark
                            adjacency_list.insert(
                                bookmark.name.clone(),
                                potential_parent.name.clone(),
                            );
                            break;
                        }
                    }
                }
            }
        }

        // Build stacks by traversing the graph
        let stacks = Self::build_stacks(&bookmarks, &adjacency_list);

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
    ) -> Vec<BranchStack> {
        let mut stacks = Vec::new();
        let mut visited = HashSet::new();

        // Find all root bookmarks (bookmarks with no parent in the graph)
        let children: HashSet<_> = adjacency_list.keys().cloned().collect();

        // Roots are bookmarks that are NOT children (i.e., they don't have parents)
        // OR bookmarks that don't appear in the adjacency list at all
        for name in bookmarks.keys() {
            if visited.contains(name) {
                continue;
            }

            let is_root = !children.contains(name);

            if is_root {
                // Build a stack starting from this root
                let mut stack_bookmarks = vec![name.clone()];
                visited.insert(name.clone());

                // Follow the chain of children
                let mut current = name.clone();
                loop {
                    // Find a child of current
                    let mut found_child = None;
                    for (child, parent) in adjacency_list.iter() {
                        if parent == &current && !visited.contains(child) {
                            found_child = Some(child.clone());
                            break;
                        }
                    }

                    match found_child {
                        Some(child) => {
                            stack_bookmarks.push(child.clone());
                            visited.insert(child.clone());
                            current = child;
                        }
                        None => break,
                    }
                }

                stacks.push(BranchStack {
                    bookmarks: stack_bookmarks,
                    base: "main".to_string(), // Default base, should be determined from config
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
        let stack = self
            .find_stack_for_bookmark(bookmark_name)
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

        let stacks = BookmarkGraph::build_stacks(&bookmarks, &adjacency_list);

        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].bookmarks, vec!["feature-a", "feature-b"]);
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

        let stacks = BookmarkGraph::build_stacks(&bookmarks, &adjacency_list);

        assert_eq!(stacks.len(), 2);
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

        assert_eq!(graph.get_parent("feature-b"), Some(&"feature-a".to_string()));
        assert_eq!(graph.get_parent("feature-a"), None);
    }
}
