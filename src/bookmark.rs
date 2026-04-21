use std::collections::{BTreeMap, BTreeSet, HashSet};

use itertools::Itertools;
use owo_colors::OwoColorize;

use crate::{
    error::{BookmarkNotFoundSnafu, Error, Result},
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

pub fn change_id_to_temp_bookmark_name(change_id: &str) -> String {
    format!("(new bookmark for {})", &change_id[..8])
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BookmarkOrPending<'a> {
    Bookmark(Bookmark<'a>),
    Pending {
        change: &'a Change,
        temp_bookmark_display_name: String,
    },
}

impl<'a> BookmarkOrPending<'a> {
    pub fn new_pending(change: &'a Change) -> Self {
        Self::Pending {
            temp_bookmark_display_name: change_id_to_temp_bookmark_name(&change.change_id),
            change,
        }
    }

    pub fn from_change(change: &'a Change) -> impl IntoIterator<Item = Self> {
        let mut real_bookmarks: Vec<_> = Bookmark::from_change(change)
            .into_iter()
            .map(Self::Bookmark)
            .collect();

        if change.pending_bookmark {
            real_bookmarks.push(Self::new_pending(change));
        }

        real_bookmarks
    }

    pub fn from_changes(
        changes: impl IntoIterator<Item = &'a Change>,
    ) -> impl IntoIterator<Item = Self> {
        changes.into_iter().flat_map(Self::from_change)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub fn is_bookmark(&self) -> bool {
        matches!(self, Self::Bookmark(_))
    }

    pub fn as_pending(&self) -> Option<&Change> {
        match self {
            Self::Pending { change, .. } => Some(change),
            Self::Bookmark(_) => None,
        }
    }

    pub fn change_id(&self) -> &str {
        match self {
            Self::Bookmark(bookmark) => bookmark.change.change_id.as_str(),
            Self::Pending { change, .. } => change.change_id.as_str(),
        }
    }

    pub fn is_local(&self) -> bool {
        match self {
            Self::Bookmark(bookmark) => bookmark.info.is_local(),
            Self::Pending { .. } => true,
        }
    }

    pub fn is_tracked(&self) -> bool {
        match self {
            Self::Bookmark(bookmark) => bookmark.info.is_tracked(),
            Self::Pending { .. } => true, /* Since a pending bookmark *will* be tracked, it's
                                           * effectively tracked. */
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Bookmark(bookmark) => bookmark.name(),
            Self::Pending {
                temp_bookmark_display_name,
                ..
            } => temp_bookmark_display_name,
        }
    }

    pub fn change(&self) -> &Change {
        match self {
            Self::Bookmark(bookmark) => bookmark.change,
            Self::Pending { change, .. } => change,
        }
    }
}

impl std::fmt::Display for BookmarkOrPending<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bookmark(bookmark) => write!(f, "{}", bookmark.name().magenta()),
            Self::Pending {
                temp_bookmark_display_name,
                ..
            } => {
                write!(f, "{}", temp_bookmark_display_name,)
            }
        }
    }
}

impl JJName for BookmarkOrPending<'_> {
    fn raw_name(&self) -> String {
        match self {
            Self::Bookmark(bookmark) => bookmark.raw_name(),
            Self::Pending { change, .. } => change.change_id.to_string(),
        }
    }

    fn name_for_jj(&self) -> String {
        match self {
            Self::Bookmark(bookmark) => bookmark.name_for_jj(),
            Self::Pending { change, .. } => change.change_id.to_string(),
        }
    }
}

pub trait JJName {
    /// Gets the raw (unquoted) name of the JJ item.
    fn raw_name(&self) -> String;

    /// Some bookmarks have special characters in their name that need to be
    /// escaped for jj. So always quote bookmarks.
    fn name_for_jj(&self) -> String;
}

impl<'a> JJName for Bookmark<'a> {
    fn raw_name(&self) -> String {
        self.name().to_string()
    }

    fn name_for_jj(&self) -> String {
        format!("\"{}\"", self.name())
    }
}

impl JJName for &str {
    fn raw_name(&self) -> String {
        self.to_string()
    }

    fn name_for_jj(&self) -> String {
        // Assume a raw string is a bookmark, not something like `trunk()`.
        format!("\"{}\"", self)
    }
}

impl JJName for &String {
    fn raw_name(&self) -> String {
        self.to_string()
    }

    fn name_for_jj(&self) -> String {
        format!("\"{}\"", self)
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
        let mut to_process: Vec<_> = self.leaves.iter().collect();
        while let Some(bookmark) = to_process.pop() {
            if bookmark.name() == name {
                return Some(bookmark);
            }

            to_process.extend(bookmark.parents.iter().filter_map(|parent| match parent {
                BookmarkRef::Bookmark(b) => Some(b),
                BookmarkRef::Trunk => None,
            }))
        }
        None
    }

    /// Check if the component contains a bookmark by name.
    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Get the downstack of a bookmark in the component
    pub fn downstack_of(&self, name: &str) -> Result<Vec<BookmarkOrPending<'_>>> {
        let bookmark = self.find(name).ok_or_else(|| {
            BookmarkNotFoundSnafu {
                name: name.to_string(),
            }
            .build()
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
        let bookmark = self.find(bookmark).ok_or_else(|| {
            BookmarkNotFoundSnafu {
                name: bookmark.to_string(),
            }
            .build()
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    pub fn downstack(&self) -> Vec<BookmarkOrPending<'_>> {
        match self {
            BookmarkRef::Bookmark(b) => b.downstack(),
            BookmarkRef::Trunk => Vec::new(),
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            BookmarkRef::Bookmark(b) => Some(b.name()),
            BookmarkRef::Trunk => None,
        }
    }

    pub fn has_parent(&self, name: &str) -> bool {
        match self {
            BookmarkRef::Bookmark(b) => b.has_parent_bookmark(name),
            BookmarkRef::Trunk => false,
        }
    }
}

impl<'a> JJName for BookmarkRef<'a> {
    fn raw_name(&self) -> String {
        match self {
            BookmarkRef::Bookmark(b) => b.raw_name(),
            BookmarkRef::Trunk => "trunk".to_string(),
        }
    }

    /// Gets the name of the bookmark or trunk as a string that can be used in
    /// a jj revset or command.
    fn name_for_jj(&self) -> String {
        match self {
            BookmarkRef::Bookmark(b) => b.name_for_jj(),
            BookmarkRef::Trunk => "trunk()".to_string(),
        }
    }
}

/// A bookmark that also points to all its parents. If you follow the parents,
/// you can get to the root of the component.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BookmarkWithPointers<'a> {
    /// The bookmark itself.
    pub bookmark: BookmarkOrPending<'a>,

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

    /// Gets the name of the parent bookmark, or the default branch if there is
    /// no parent or the parent is the trunk.
    pub fn parent_name(&self, default_branch: &str) -> String {
        // TODO let user pick target branch
        match self.parents.first() {
            Some(BookmarkRef::Bookmark(b)) => b.name().to_string(),
            Some(BookmarkRef::Trunk) | None => default_branch.to_string(),
        }
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
    pub fn downstack(&self) -> Vec<BookmarkOrPending<'_>> {
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

    pub fn has_parent_bookmark(&self, name: &str) -> bool {
        self.parents.iter().any(|p| match p {
            BookmarkRef::Bookmark(b) => b.name() == name,
            BookmarkRef::Trunk => false,
        })
    }

    pub fn has_parent_ref(&self, parent: &BookmarkRef<'_>) -> bool {
        self.parents.iter().any(|p| p == parent)
    }

    /// Gets the revisions that are unique to this bookmark (i.e. what would be
    /// merged into its parents).
    pub fn revisions(&self, jj: &Jujutsu) -> Result<Vec<Change>> {
        let revset = [BookmarkRef::Trunk]
            .into_iter()
            .chain(
                self.parents
                    .iter()
                    .filter(|p| matches!(p, BookmarkRef::Bookmark(..)))
                    .cloned(),
            )
            .map(|p| format!("({}..{})", p.name_for_jj(), self.name_for_jj()))
            .join(" & ");

        jj.log(revset)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.bookmark, BookmarkOrPending::Pending { .. })
    }
}

impl<'a> JJName for BookmarkWithPointers<'a> {
    fn raw_name(&self) -> String {
        self.bookmark.raw_name()
    }

    fn name_for_jj(&self) -> String {
        self.bookmark.name_for_jj()
    }
}

/// A set of jj changes that have been grouped into independent components,
/// which are not connected to any other component except for the trunk.
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkGraph<'a> {
    /// All bookmarks in the revset (keyed by name)
    bookmarks: BTreeMap<String, BookmarkOrPending<'a>>,

    /// Independent components of the bookmark graph
    components: Vec<ChangeComponent<'a>>,
}

impl<'a> BookmarkGraph<'a> {
    pub fn from_changes(
        jj: &Jujutsu,
        changes: impl IntoIterator<Item = &'a Change>,
        skip_untracked_local_bookmarks: bool,
    ) -> Result<Self> {
        let bookmarks: Vec<_> = BookmarkOrPending::from_changes(changes)
            .into_iter()
            .collect();
        Self::from_bookmarks(jj, bookmarks, skip_untracked_local_bookmarks)
    }

    /// Build a bookmark graph from a list of bookmarks.
    pub fn from_bookmarks(
        jj: &Jujutsu,
        bookmarks: impl IntoIterator<Item = BookmarkOrPending<'a>>,
        skip_untracked_local_bookmarks: bool,
    ) -> Result<Self> {
        let local_bookmarks: Vec<_> = bookmarks
            .into_iter()
            .filter(|b| b.is_local() && (!skip_untracked_local_bookmarks || b.is_tracked()))
            .collect();

        let mut bookmark_lookup: BTreeMap<_, _> = local_bookmarks
            .iter()
            .map(|b| (b.name().to_string(), b.clone()))
            .collect();

        let pending_bookmarks: HashSet<String> = local_bookmarks
            .iter()
            .filter_map(|b| b.as_pending().map(|c| c.change_id.clone()))
            .collect();

        let mut adjacency_list = BTreeMap::new();

        for bookmark in &local_bookmarks {
            if jj.any_in_revset(format!("({}) & trunk()", bookmark.change_id()))? {
                bookmark_lookup.remove(bookmark.name());
                continue;
            }

            let parent_bookmarks = Self::find_nearest_bookmarked_ancestors(
                jj,
                bookmark.change(),
                skip_untracked_local_bookmarks,
                &pending_bookmarks,
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
        bookmark_lookup: BTreeMap<String, BookmarkOrPending<'a>>,
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
            bookmark_lookup: &BTreeMap<String, BookmarkOrPending<'b>>,
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
    pub fn bookmarks(&self) -> impl Iterator<Item = BookmarkOrPending<'_>> {
        self.bookmarks.values().cloned()
    }

    pub fn bookmarks_with_pointers(&self) -> impl Iterator<Item = &BookmarkWithPointers<'_>> {
        self.components
            .iter()
            .flat_map(|component| component.all_bookmarks())
    }

    /// Get all components in the graph.
    pub fn components(&self) -> &[ChangeComponent<'_>] {
        &self.components
    }

    /// Gets a bookmark by name. Note that this is not the same as finding a
    /// bookmark in a component - this does not contain any parent information.
    pub fn bookmark(&self, name: &str) -> Option<BookmarkOrPending<'_>> {
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
    pub fn downstack_of(&self, bookmark_name: &str) -> Result<Vec<BookmarkOrPending<'_>>> {
        let component = self.component_containing(bookmark_name).ok_or_else(|| {
            BookmarkNotFoundSnafu {
                name: bookmark_name.to_string(),
            }
            .build()
        })?;

        component.downstack_of(bookmark_name)
    }

    /// Find the nearest bookmarked ancestors starting from a given commit
    fn find_nearest_bookmarked_ancestors(
        jj: &Jujutsu,
        from: &Change,
        skip_untracked_local_bookmarks: bool,
        pending_bookmarks: &HashSet<String>,
    ) -> Result<Vec<Change>> {
        let mut ancestors = Vec::new();

        let parents = jj.log(format!("{}- ~ ::trunk()", from.commit_id))?;

        for parent in parents {
            let bookmarks: Vec<_> = parent
                .bookmarks
                .iter()
                .filter(|bookmark| !skip_untracked_local_bookmarks || bookmark.is_tracked())
                .collect();

            if bookmarks.is_empty() && !pending_bookmarks.contains(&parent.change_id) {
                ancestors.extend(Self::find_nearest_bookmarked_ancestors(
                    jj,
                    &parent,
                    skip_untracked_local_bookmarks,
                    pending_bookmarks,
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
