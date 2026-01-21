use std::collections::{BTreeMap, BTreeSet};

use crate::{
    error::{Error, Result},
    jj::{BookmarkInfo, Change, Jujutsu},
};

/// A bookmark in jj.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bookmark<'a> {
    /// Information about the bookmark.
    pub info: &'a BookmarkInfo,

    /// The change that the bookmark is associated with. This change contains
    /// the actual BookmarkInfo.
    pub change: &'a Change,
}

impl<'a> Bookmark<'a> {
    pub fn from_change(change: &'a Change) -> impl IntoIterator<Item = Self> {
        change
            .bookmarks
            .iter()
            .map(move |info| Self { info, change })
    }

    pub fn from_changes(
        changes: impl IntoIterator<Item = &'a Change>,
    ) -> impl IntoIterator<Item = Self> {
        changes.into_iter().flat_map(Self::from_change)
    }

    /// Check if the bookmark is a local bookmark.
    pub fn is_local(&self) -> bool {
        self.info.is_local()
    }

    /// Check if the bookmark is a remote bookmark.
    pub fn is_remote(&self) -> bool {
        self.info.is_remote()
    }

    /// Get the name of the bookmark.
    pub fn name(&self) -> &str {
        self.info.name()
    }

    /// Get the full name of the bookmark, including the @<remote> suffix if it
    /// is a remote bookmark.
    pub fn full_name(&self) -> String {
        self.info.full_name()
    }
}

impl PartialEq<str> for &Bookmark<'_> {
    fn eq(&self, other: &str) -> bool {
        self.name() == other
    }
}

impl PartialEq<String> for &Bookmark<'_> {
    fn eq(&self, other: &String) -> bool {
        self.name() == other.as_str()
    }
}

/// A graph of jj changes that is independent of any other ChangeComponent.
/// The component is only connected to the trunk. Can be thought of as a "stack"
/// of changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeComponent<'a> {
    /// Bookmarks in this component. Each bookmark in this list is
    /// a "leaf" of the component.
    pub leaves: Vec<BookmarkWithPointers<'a>>,
}

impl ChangeComponent<'_> {
    /// Finds a bookmark in the component by name.
    pub fn find(&self, name: &str) -> Option<&BookmarkWithPointers<'_>> {
        self.leaves.iter().find_map(|b| b.find(name))
    }

    /// Check if the component contains a bookmark by name.
    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Get the downstack of a bookmark in the component
    pub fn downstack_of(&self, name: &str) -> Result<Vec<Bookmark<'_>>> {
        let bookmark = self.find(name).ok_or_else(|| Error::BookmarkNotFound {
            name: name.to_string(),
        })?;

        Ok(bookmark.downstack())
    }

    /// Get all bookmarks in the component.
    pub fn all_bookmarks(&self) -> Vec<&BookmarkWithPointers<'_>> {
        let mut all: Vec<&BookmarkWithPointers<'_>> = Vec::new();

        let mut to_process: Vec<_> = self.leaves.iter().collect();

        while let Some(bookmark) = to_process.pop() {
            if !all.iter().any(|b| b.name() == bookmark.name()) {
                all.push(bookmark);
            }

            to_process.extend(bookmark.parents.iter().filter_map(|parent| match parent {
                BookmarkRef::Bookmark(b) => Some(b),
                BookmarkRef::Trunk => None,
            }))
        }

        all
    }

    /// Get the total number of bookmarks in the component.
    pub fn len(&self) -> usize {
        self.all_bookmarks().len()
    }

    /// Check if the component is empty.
    pub fn is_empty(&self) -> bool {
        self.all_bookmarks().is_empty()
    }

    /// A tree component is a component where no change has multiple parents.
    pub fn is_tree(&self) -> bool {
        self.leaves.iter().all(|b| b.is_linear())
    }

    /// A linear component is a component where no change has multiple children
    /// nor parents.
    pub fn is_linear(&self) -> bool {
        match &self.leaves[..] {
            [] => true,
            [bookmark] => bookmark.is_linear(),
            _ => false,
        }
    }

    /// Check if the component is linear from a given bookmark, down to the
    /// trunk. Returns an Err if the bookmark is not found in the component.
    pub fn is_linear_from(&self, bookmark: &str) -> Result<bool> {
        let bookmark = self.find(bookmark).ok_or_else(|| Error::BookmarkNotFound {
            name: bookmark.to_string(),
        })?;

        Ok(bookmark.is_linear())
    }

    /// Sort bookmarks in the component in topological order (dependencies
    /// first). Returns bookmarks ordered such that parent bookmarks appear
    /// before their children.
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        let mut adjacency_list: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let all = self.all_bookmarks();

        for bookmark in &all {
            adjacency_list.insert(
                bookmark.name(),
                bookmark
                    .parents
                    .iter()
                    .filter_map(|p| match p {
                        BookmarkRef::Bookmark(b) => Some(b.name()),
                        BookmarkRef::Trunk => None,
                    })
                    .collect(),
            );
        }

        let mut reverse_adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new(); // parent -> children
        for (child, parents) in &adjacency_list {
            for parent in parents {
                reverse_adjacency.entry(parent).or_default().push(child);
            }
        }

        let mut in_degree: BTreeMap<_, _> = adjacency_list
            .into_iter()
            .map(|(name, parents)| (name, parents.len()))
            .collect();

        let mut queue: Vec<_> = in_degree
            .iter()
            .filter_map(
                |(name, degree)| {
                    if *degree == 0 { Some(*name) } else { None }
                },
            )
            .collect();

        let mut result = Vec::new();

        while let Some(current) = queue.pop() {
            result.push(current.to_string());

            // Reduce in-degree for all children
            if let Some(children) = reverse_adjacency.get(&current) {
                for child in children {
                    if let Some(degree) = in_degree.get_mut(child) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(child);
                        }
                    }
                }
            }
        }

        if result.len() != all.len() {
            return Err(Error::new("Cycle detected in bookmark graph"));
        }

        Ok(result)
    }
}

/// A reference to a bookmark or the trunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkRef<'a> {
    /// A regular bookmark.
    Bookmark(BookmarkWithPointers<'a>),

    /// Any change that is part of trunk().
    Trunk,
}

impl BookmarkRef<'_> {
    /// Finds a bookmark in the reference by name.
    pub fn find(&self, name: &str) -> Option<&BookmarkWithPointers<'_>> {
        match self {
            BookmarkRef::Bookmark(b) => b.find(name),
            BookmarkRef::Trunk => None,
        }
    }

    /// Get the downstack of the bookmark.
    pub fn downstack(&self) -> Vec<Bookmark<'_>> {
        match self {
            BookmarkRef::Bookmark(b) => b.downstack(),
            BookmarkRef::Trunk => Vec::new(),
        }
    }
}

/// A bookmark that also points to all its parents. If you follow the parents,
/// you can get to the root of the component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkWithPointers<'a> {
    /// The bookmark itself.
    pub bookmark: Bookmark<'a>,

    /// The parents of the bookmark. Is multiple if the bookmark is a merge
    /// commit, or if the bookmark has merge commits between it and any of
    /// its parent bookmarks.
    pub parents: Vec<BookmarkRef<'a>>,
}

impl BookmarkWithPointers<'_> {
    /// Get the name of the bookmark.
    pub fn name(&self) -> &str {
        self.bookmark.name()
    }

    /// Finds a bookmark in the bookmark or its parents by name.
    pub fn find(&self, name: &str) -> Option<&BookmarkWithPointers<'_>> {
        if self.name() == name {
            Some(self)
        } else {
            self.parents.iter().find_map(|p| p.find(name))
        }
    }

    /// Get the downstack of the bookmark.
    pub fn downstack(&self) -> Vec<Bookmark<'_>> {
        let mut downstack = vec![self.bookmark.clone()];
        for parent in &self.parents {
            downstack.extend(
                parent
                    .downstack()
                    .into_iter()
                    .filter(|b| !downstack.iter().any(|b2| b2.name() == b.name()))
                    .collect::<Vec<_>>(),
            );
        }
        downstack
    }

    /// A bookmark is linear if no ancestor has multiple parents.
    /// An ancestor is allowed to have multiple children (only linear from the
    /// perspective of this bookmark).
    pub fn is_linear(&self) -> bool {
        let mut current = self;
        loop {
            match &current.parents[..] {
                [] => {
                    return true;
                }
                [BookmarkRef::Bookmark(b)] => {
                    if b.parents.len() > 1 {
                        return false;
                    }
                    current = b;
                }
                [BookmarkRef::Trunk] => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }
    }
}

/// A set of jj changes that have been grouped into independent components,
/// which are not connected to any other component except for the trunk.
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkGraph<'a> {
    /// All bookmarks in the revset (keyed by name)
    bookmarks: BTreeMap<String, Bookmark<'a>>,

    /// Independent components of the bookmark graph
    components: Vec<ChangeComponent<'a>>,
}

impl<'a> BookmarkGraph<'a> {
    /// Build a bookmark graph from a list of bookmarks.
    pub fn from_bookmarks(
        jj: &Jujutsu,
        bookmarks: impl IntoIterator<Item = Bookmark<'a>>,
        skip_untracked_local_bookmarks: bool,
    ) -> Result<Self> {
        let local_bookmarks: Vec<_> = bookmarks
            .into_iter()
            .filter(|b| b.is_local() && (!skip_untracked_local_bookmarks || b.info.is_tracked()))
            .collect();

        let mut bookmark_lookup: BTreeMap<_, _> = local_bookmarks
            .iter()
            .map(|b| (b.name().to_string(), b.clone()))
            .collect();

        let mut adjacency_list = BTreeMap::new();

        for bookmark in &local_bookmarks {
            if jj.any_in_revset(format!("({}) & trunk()", bookmark.change.change_id))? {
                bookmark_lookup.remove(bookmark.name());
                continue;
            }

            let parent_bookmarks = Self::find_nearest_bookmarked_ancestors(
                jj,
                bookmark.change,
                skip_untracked_local_bookmarks,
            )?;

            adjacency_list
                .entry(bookmark.name().to_string())
                .or_insert(BTreeSet::new())
                .extend(
                    parent_bookmarks
                        .iter()
                        .flat_map(|b| b.bookmarks.iter().map(|b| b.full_name().to_string())),
                );
        }

        Ok(Self::from_lookups(bookmark_lookup, adjacency_list))
    }

    /// Build independent components from the bookmark graph
    pub fn from_lookups(
        bookmark_lookup: BTreeMap<String, Bookmark<'a>>,
        adjacency_list: BTreeMap<String, BTreeSet<String>>,
    ) -> Self {
        let parents: BTreeSet<_> = adjacency_list.values().flatten().cloned().collect();
        let leaves: BTreeSet<_> = bookmark_lookup
            .keys()
            .filter(|k| !parents.contains(*k))
            .collect();

        fn get_roots(
            name: &str,
            adjacency_list: &BTreeMap<String, BTreeSet<String>>,
        ) -> BTreeSet<String> {
            let mut roots = BTreeSet::new();
            let empty_set = BTreeSet::new();
            let parents = adjacency_list.get(name).unwrap_or(&empty_set);

            if parents.is_empty() {
                return BTreeSet::from([name.to_string()]);
            }

            roots.extend(
                parents
                    .iter()
                    .flat_map(|p| get_roots(p.as_ref(), adjacency_list)),
            );

            roots
        }

        // Get a mapping from every root to the leaves that are on it - there may be
        // overlaps, where a leaf is on multiple roots.
        let mut components_overlapping = BTreeMap::new();
        for leaf in leaves {
            for root in get_roots(leaf, &adjacency_list) {
                components_overlapping
                    .entry(root)
                    .or_insert(BTreeSet::new())
                    .insert(leaf.clone());
            }
        }

        // Deduplicate
        let mut components: Vec<(BTreeSet<String>, BTreeSet<String>)> = Vec::new();
        for (root, leaves) in &components_overlapping {
            // Find existing component that contains either the root or any leaf
            let existing_component =
                components
                    .iter_mut()
                    .find(|(component_roots, component_leaves)| {
                        component_roots.contains(root) || !component_leaves.is_disjoint(leaves)
                    });

            if let Some((existing_roots, existing_leaves)) = existing_component {
                existing_roots.insert(root.clone());
                existing_leaves.extend(leaves.iter().cloned());
            } else {
                components.push((BTreeSet::from([root.clone()]), leaves.clone()));
            }
        }

        fn get_pointer<'b>(
            bookmark_lookup: &BTreeMap<String, Bookmark<'b>>,
            adjacency_list: &BTreeMap<String, BTreeSet<String>>,
            name: &str,
        ) -> BookmarkWithPointers<'b> {
            BookmarkWithPointers {
                bookmark: bookmark_lookup
                    .get(name)
                    .unwrap_or_else(|| panic!("Bookmark {} not found in bookmark_lookup", name))
                    .clone(),
                parents: adjacency_list
                    .get(name)
                    .map(|parents| {
                        parents
                            .iter()
                            .map(|parent| get_pointer(bookmark_lookup, adjacency_list, parent))
                            .map(BookmarkRef::Bookmark)
                            .collect()
                    })
                    .unwrap_or(vec![BookmarkRef::Trunk]),
            }
        }

        let components = components
            .into_iter()
            .map(|(_roots, leaves)| ChangeComponent {
                leaves: leaves
                    .iter()
                    .map(|leaf| get_pointer(&bookmark_lookup, &adjacency_list, leaf))
                    .collect(),
            })
            .collect();

        Self {
            bookmarks: bookmark_lookup,
            components,
        }
    }

    /// Get all bookmarks in the graph.
    pub fn bookmarks(&self) -> impl Iterator<Item = Bookmark<'_>> {
        self.bookmarks.values().map(Bookmark::clone)
    }

    /// Get all components in the graph.
    pub fn components(&self) -> &[ChangeComponent<'_>] {
        &self.components
    }

    /// Gets a bookmark by name. Note that this is not the same as finding a
    /// bookmark in a component - this does not contain any parent information.
    pub fn bookmark(&self, name: &str) -> Option<Bookmark<'_>> {
        self.bookmarks.get(name).cloned()
    }

    /// Find a bookmark in one of the components, by name.
    pub fn find_bookmark_in_components(
        &self,
        bookmark_name: &str,
    ) -> Option<&BookmarkWithPointers<'_>> {
        let component = self.component_containing(bookmark_name)?;
        component.find(bookmark_name)
    }

    /// Find the component containing a specific bookmark
    pub fn component_containing(&self, bookmark_name: &str) -> Option<&ChangeComponent<'_>> {
        self.components
            .iter()
            .find(|component| component.find(bookmark_name).is_some())
    }

    /// Get all bookmarks in the downstack of a given bookmark (inclusive)
    /// E.g. if the component is [main, feature-a, feature-b, feature-c], and
    /// the bookmark is feature-b, the downstack is [feature-b, feature-a,
    /// main].
    pub fn downstack_of(&self, bookmark_name: &str) -> Result<Vec<Bookmark<'_>>> {
        let component =
            self.component_containing(bookmark_name)
                .ok_or_else(|| Error::BookmarkNotFound {
                    name: bookmark_name.to_string(),
                })?;

        component.downstack_of(bookmark_name)
    }

    /// Find the nearest bookmarked ancestors starting from a given commit
    fn find_nearest_bookmarked_ancestors(
        jj: &Jujutsu,
        from: &Change,
        skip_untracked_local_bookmarks: bool,
    ) -> Result<Vec<Change>> {
        let mut ancestors = Vec::new();

        let parents = jj.log(format!("{}- ~ ::trunk()", from.commit_id))?;

        for parent in parents {
            let bookmarks: Vec<_> = if skip_untracked_local_bookmarks {
                parent
                    .bookmarks
                    .iter()
                    .filter(|bookmark| bookmark.is_tracked())
                    .collect()
            } else {
                parent.bookmarks.iter().collect()
            };

            if bookmarks.is_empty() {
                ancestors.extend(Self::find_nearest_bookmarked_ancestors(
                    jj,
                    &parent,
                    skip_untracked_local_bookmarks,
                )?);
            } else {
                ancestors.push(parent);
            }
        }

        Ok(ancestors)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::jj::ChangeMap;

    #[test]
    fn test_build_components_simple_linear() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("root"),
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert_eq!(graph.components.len(), 1);
        assert_eq!(graph.components[0].leaves.len(), 1);
        assert_eq!(graph.components[0].leaves[0].name(), "feature-c");
    }

    #[test]
    fn test_build_components_multiple() {
        let mut changes = ChangeMap::new();

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]));

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-d"),
            Change::mock_from_bookmark("feature-e"),
            Change::mock_from_bookmark("feature-f"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert_eq!(graph.components.len(), 2);
        assert_eq!(graph.components[0].leaves.len(), 1);
        assert_eq!(graph.components[1].leaves.len(), 1);
        assert_eq!(graph.components[0].leaves[0].name(), "feature-c");
        assert_eq!(graph.components[1].leaves[0].name(), "feature-f");
    }

    #[test]
    fn test_find_bookmark_in_components() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.find_bookmark_in_components("feature-a").is_some());
        assert!(graph.find_bookmark_in_components("feature-b").is_some());
        assert!(graph.find_bookmark_in_components("feature-c").is_some());
        assert!(graph.find_bookmark_in_components("feature-d").is_none());
    }

    #[test]
    fn test_get_downstack() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        let downstack = graph.downstack_of("feature-b").unwrap();
        assert_eq!(
            downstack,
            vec![
                graph.bookmark("feature-b").unwrap(),
                graph.bookmark("feature-a").unwrap(),
            ]
        );

        let downstack_c = graph.downstack_of("feature-c").unwrap();
        assert_eq!(
            downstack_c,
            vec![
                graph.bookmark("feature-c").unwrap(),
                graph.bookmark("feature-b").unwrap(),
                graph.bookmark("feature-a").unwrap(),
            ]
        );
    }

    #[test]
    fn test_get_parent() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert_eq!(
            graph
                .find_bookmark_in_components("feature-a")
                .unwrap()
                .parents,
            vec![]
        );

        assert_eq!(
            graph
                .find_bookmark_in_components("feature-b")
                .unwrap()
                .parents,
            vec![BookmarkRef::Bookmark(
                graph
                    .find_bookmark_in_components("feature-a")
                    .unwrap()
                    .clone()
            )]
        );

        assert_eq!(
            graph
                .find_bookmark_in_components("feature-c")
                .unwrap()
                .parents,
            vec![BookmarkRef::Bookmark(
                graph
                    .find_bookmark_in_components("feature-b")
                    .unwrap()
                    .clone()
            )]
        );
    }

    #[test]
    fn test_build_components_with_branching() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-c").with_mock_parent_bookmarks(["feature-b"]),
            Change::mock_from_bookmark("feature-d"),
        ]));

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-e").with_mock_parent_bookmarks(["feature-b"]),
            Change::mock_from_bookmark("feature-f"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert_eq!(graph.components.len(), 1);
        assert_eq!(graph.components[0].leaves.len(), 2);
        assert_eq!(graph.components[0].leaves[0].name(), "feature-d");
        assert_eq!(graph.components[0].leaves[1].name(), "feature-f");
    }

    #[test]
    fn test_tree_components() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_tree());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
            Change::mock_from_bookmark("feature-e"),
            Change::mock_from_bookmark("feature-f"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_tree());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.insert(
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-e").with_mock_parent_bookmarks(["feature-b"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-f").with_mock_parent_bookmarks(["feature-c"]),
        );

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_tree());
    }

    #[test]
    fn test_tree_components_false() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(!graph.components[0].is_tree());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
            Change::mock_from_bookmark("feature-e"),
            Change::mock_from_bookmark("feature-f").with_mock_parent_bookmarks(["feature-c"]),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(!graph.components[0].is_tree());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.insert(
            Change::mock_from_bookmark("feature-d")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-e")
                .with_mock_parent_bookmarks(["feature-b", "feature-c"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-f")
                .with_mock_parent_bookmarks(["feature-c", "feature-a"]),
        );

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(!graph.components[0].is_tree());
    }

    #[test]
    fn test_linear_components() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_linear());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-c"]),
            Change::mock_from_bookmark("feature-e"),
            Change::mock_from_bookmark("feature-f"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_linear());
    }

    #[test]
    fn test_linear_components_false() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
            Change::mock_from_bookmark("feature-e"),
            Change::mock_from_bookmark("feature-f"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(!graph.components[0].is_linear());

        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.insert(
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-e").with_mock_parent_bookmarks(["feature-b"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-f").with_mock_parent_bookmarks(["feature-c"]),
        );

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(!graph.components[0].is_linear());
    }

    #[test]
    fn test_is_linear_from() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.insert(
            Change::mock_from_bookmark("feature-d").with_mock_parent_bookmarks(["feature-a"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-e").with_mock_parent_bookmarks(["feature-b"]),
        );
        changes.insert(
            Change::mock_from_bookmark("feature-f").with_mock_parent_bookmarks(["feature-c"]),
        );

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_linear_from("feature-d").unwrap());
        assert!(graph.components[0].is_linear_from("feature-e").unwrap());
        assert!(graph.components[0].is_linear_from("feature-f").unwrap());

        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        assert!(graph.components[0].is_linear_from("feature-a").unwrap());
        assert!(graph.components[0].is_linear_from("feature-b").unwrap());
        assert!(!graph.components[0].is_linear_from("feature-c").unwrap());
    }
}
