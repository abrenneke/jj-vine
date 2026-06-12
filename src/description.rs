use core::{fmt::Write as _, hash::BuildHasher};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    hash::RandomState,
    path::Path,
};

use enum_dispatch::enum_dispatch;
use itertools::Itertools as _;

use crate::{
    bookmark::{BookmarkRef, ChangeComponent},
    config::{DescriptionConfig, DescriptionDiagramFormat, DescriptionMode},
    error::Result,
    forge::{AnyForgeMergeRequest, BorrowId, Forge as _, ForgeImpl, MergeRequestLike as _},
    jj::Change,
    utils::toposort,
};

#[enum_dispatch]
pub trait FormatMergeRequest {
    type Id: BorrowId;

    /// Formats the ID of a merge request as a string for display in the
    /// description.
    fn format_merge_request_id<'a>(&'a self, mr_iid: <Self::Id as BorrowId>::Id<'a>) -> String;

    /// Gets e.g. "MR" or "PR" for the merge request.
    fn mr_name(&self) -> &'static str;

    /// Returns true if the forge expands the MR/PR ID to the full title.
    fn id_expands_title(&self) -> bool {
        false
    }

    /// Gets a URL that shows a diff comparison between two identifiers (e.g.
    /// commit shas, bookmarks/branches, tags).
    fn mr_diff_url(
        &self,
        from: &BookmarkRef,
        to: &BookmarkRef,
        default_branch: &str,
    ) -> Result<String>;
}

pub enum Formatter {
    None,
    LinearList(LinearListFormatter),
    Tree(TreeFormatter),
}

impl Formatter {
    #[must_use]
    pub fn format_single(&self, context: &FormatContext<impl BuildHasher>) -> String {
        match self {
            Formatter::None => String::new(),
            Formatter::LinearList(formatter) => formatter.format_single(context),
            Formatter::Tree(formatter) => formatter.format_single(context),
        }
    }

    #[must_use]
    pub fn format_linear(&self, context: &FormatContext<impl BuildHasher>) -> String {
        match self {
            Formatter::None => String::new(),
            Formatter::LinearList(formatter) => formatter.format_linear(context),
            Formatter::Tree(formatter) => formatter.format_linear(context),
        }
    }

    #[must_use]
    pub fn format_tree(&self, context: &FormatContext<impl BuildHasher>) -> String {
        match self {
            Formatter::None => String::new(),
            Formatter::LinearList(formatter) => formatter.format_tree(context),
            Formatter::Tree(formatter) => formatter.format_tree(context),
        }
    }

    #[must_use]
    pub fn format_graph(&self, context: &FormatContext<impl BuildHasher>) -> String {
        match self {
            Formatter::None => String::new(),
            Formatter::LinearList(formatter) => formatter.format_graph(context),
            Formatter::Tree(formatter) => formatter.format_graph(context),
        }
    }
}

pub struct LinearListFormatter;

impl LinearListFormatter {
    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_single(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing 1 {mr_name}:\n",
        ));

        let ordered_bookmarks: Vec<_> = context
            .component
            .topological_sort()
            .expect("Cycle detected in bookmark graph!")
            .into_iter()
            .map(|name| {
                BookmarkRef::Bookmark(
                    context
                        .component
                        .find(&name)
                        .expect("Bookmark not found in component!")
                        .clone(),
                )
            })
            .collect();

        for (idx, bookmark) in [BookmarkRef::Trunk]
            .iter()
            .chain(ordered_bookmarks.iter())
            .enumerate()
        {
            lines.push(Self::format_bookmark(bookmark, idx, context, None, 1));
        }

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_linear(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing {} {mr_name}s:\n",
            context.component.len()
        ));

        let ordered_bookmarks: Vec<_> = context
            .component
            .topological_sort()
            .expect("Cycle detected in bookmark graph!")
            .into_iter()
            .map(|name| {
                BookmarkRef::Bookmark(
                    context
                        .component
                        .find(&name)
                        .expect("Bookmark not found in component!")
                        .clone(),
                )
            })
            .collect();

        for (idx, bookmark) in [BookmarkRef::Trunk]
            .iter()
            .chain(ordered_bookmarks.iter())
            .enumerate()
        {
            lines.push(Self::format_bookmark(bookmark, idx, context, None, 1));
        }

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_tree(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a tree containing {} {mr_name}s:\n",
            context.component.len()
        ));

        let ordered_bookmarks: Vec<_> = context
            .component
            .topological_sort()
            .expect("Cycle detected in bookmark graph!")
            .into_iter()
            .map(|name| {
                BookmarkRef::Bookmark(
                    context
                        .component
                        .find(&name)
                        .expect("Bookmark not found in component!")
                        .clone(),
                )
            })
            .collect();

        for (idx, bookmark) in [BookmarkRef::Trunk]
            .iter()
            .chain(ordered_bookmarks.iter())
            .enumerate()
        {
            let (num_siblings, parents) = match bookmark {
                BookmarkRef::Bookmark(bookmark) => {
                    let parent = match &bookmark.parents[..] {
                        [] => &BookmarkRef::Trunk,
                        [parent] => parent,
                        _ => panic!(
                            "Bookmark in tree should have exactly one parent. Has: {:?}",
                            bookmark.parents
                        ),
                    };
                    let num_siblings = context
                        .component
                        .all_bookmarks()
                        .into_iter()
                        .filter(|b| b.has_parent_ref(parent))
                        .count();
                    (num_siblings, Some(&bookmark.parents[..]))
                }
                BookmarkRef::Trunk => (0, None),
            };

            lines.push(Self::format_bookmark(
                bookmark,
                idx,
                context,
                parents,
                num_siblings,
            ));
        }

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_graph(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a complex set of {mr_name}s containing {} {mr_name}s:\n",
            context.component.len()
        ));

        let ordered_bookmarks: Vec<_> = context
            .component
            .topological_sort()
            .expect("Cycle detected in bookmark graph!")
            .into_iter()
            .map(|name| {
                BookmarkRef::Bookmark(
                    context
                        .component
                        .find(&name)
                        .expect("Bookmark not found in component!")
                        .clone(),
                )
            })
            .collect();

        let mut seen = HashSet::new();

        for (idx, bookmark) in [BookmarkRef::Trunk]
            .iter()
            .chain(ordered_bookmarks.iter())
            .enumerate()
        {
            if seen.contains(bookmark) {
                continue;
            }
            seen.insert(bookmark);

            let (num_siblings, parents) = match bookmark {
                BookmarkRef::Bookmark(bookmark) => {
                    let num_siblings = context
                        .component
                        .all_bookmarks()
                        .into_iter()
                        .filter(|b| {
                            b.parents.iter().any(|p| match p {
                                BookmarkRef::Bookmark(p) => bookmark.has_parent_bookmark(p.name()),
                                BookmarkRef::Trunk => false,
                            })
                        })
                        .count();
                    (num_siblings, Some(&bookmark.parents[..]))
                }
                BookmarkRef::Trunk => (0, None),
            };

            lines.push(Self::format_bookmark(
                bookmark,
                idx,
                context,
                parents,
                num_siblings,
            ));
        }

        lines.join("\n")
    }

    fn format_bookmark(
        bookmark: &BookmarkRef<'_>,
        idx: usize,
        context: &FormatContext<impl BuildHasher>,
        parents: Option<&[BookmarkRef<'_>]>,
        _num_siblings: usize,
    ) -> String {
        let into = if let Some(parents) = parents {
            format!(
                " → {}",
                match parents[..] {
                    [] => format!("`{}`", context.base_branch),
                    [..] => {
                        parents
                            .iter()
                            .map(|parent| match parent {
                                BookmarkRef::Bookmark(bookmark) => {
                                    let mr = context
                                        .merge_request_lookup
                                        .get(bookmark.name())
                                        .expect("Parent bookmark should always have an MR");
                                    context
                                        .format_merge_request
                                        .format_merge_request_id(mr.iid())
                                }
                                BookmarkRef::Trunk => format!("`{}`", context.base_branch),
                            })
                            .join(", ")
                    }
                }
            )
        } else {
            String::new()
        };

        let list_indicator = format!("{}.", idx.saturating_add(1));

        format_bookmark_entry(bookmark, "", &list_indicator, &into, context, None)
    }
}

pub struct TreeFormatter;

impl TreeFormatter {
    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_single(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing 1 {mr_name}:\n",
        ));

        Self::format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_linear(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing {} {mr_name}s:\n",
            context.component.len()
        ));

        Self::format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_tree(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a tree containing {} {mr_name}s:\n",
            context.component.len()
        ));

        Self::format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    /// # Panics
    ///
    /// Panics if the context is malformed.
    #[must_use]
    pub fn format_graph(&self, context: &FormatContext<impl BuildHasher>) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a complex set of {mr_name}s containing {} {mr_name}s:\n",
            context.component.len()
        ));

        Self::format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    fn format_tree_recursive(
        item: &BookmarkRef,
        parent: Option<&BookmarkRef>,
        depth: usize,
        context: &FormatContext<impl BuildHasher>,
        lines: &mut Vec<String>,
        num_siblings: usize,
        idx: usize,
    ) {
        lines.push(Self::format_bookmark_tree(
            item,
            parent,
            idx,
            depth,
            context,
            num_siblings,
        ));

        let children: Vec<_> = context
            .component
            .all_bookmarks()
            .into_iter()
            .filter(|b| match b.parents[..] {
                [] => *item == BookmarkRef::Trunk,
                [..] => b.parents.iter().any(|p| match (item, p) {
                    (BookmarkRef::Trunk, BookmarkRef::Trunk) => true,
                    (BookmarkRef::Bookmark(parent_b), BookmarkRef::Bookmark(child_p)) => {
                        parent_b.name() == child_p.name()
                    }
                    _ => false,
                }),
            })
            .collect();

        for (idx, child) in children.iter().enumerate() {
            Self::format_tree_recursive(
                &BookmarkRef::Bookmark((*child).clone()),
                Some(item),
                depth.strict_add(1),
                context,
                lines,
                children.len(),
                idx,
            );
        }
    }

    #[expect(clippy::single_call_fn, reason = "breaking things up")]
    fn format_bookmark_tree(
        bookmark: &BookmarkRef<'_>,
        parent: Option<&BookmarkRef>,
        idx: usize,
        depth: usize,
        context: &FormatContext<impl BuildHasher>,
        num_siblings: usize,
    ) -> String {
        let parents = match bookmark {
            BookmarkRef::Bookmark(bookmark) => &bookmark.parents[..],
            BookmarkRef::Trunk => &[],
        };

        let indent = "    ".repeat(depth);

        let also = if parents.len() > 1 {
            format!(
                " (→ {} also)",
                parents
                    .iter()
                    .filter(|possible_parent| Some(possible_parent) != parent.as_ref())
                    .map(|parent| match parent {
                        BookmarkRef::Bookmark(bookmark) => {
                            let mr = context
                                .merge_request_lookup
                                .get(bookmark.name())
                                .expect("Parent bookmark should always have an MR");
                            context
                                .format_merge_request
                                .format_merge_request_id(mr.iid())
                        }
                        BookmarkRef::Trunk => format!("`{}`", context.base_branch),
                    })
                    .join(", ")
            )
        } else {
            String::new()
        };

        let list_indicator = if num_siblings > 1 {
            format!("{}.", idx.strict_add(1))
        } else {
            "-".to_owned()
        };

        format_bookmark_entry(bookmark, &indent, &list_indicator, &also, context, parent)
    }
}

fn format_bookmark_entry(
    bookmark: &BookmarkRef<'_>,
    prefix: &str,
    list_indicator: &str,
    suffix: &str,
    context: &FormatContext<impl BuildHasher>,
    base: Option<&BookmarkRef>,
) -> String {
    let compares = render_compare_links(bookmark, base, context);

    match bookmark {
        BookmarkRef::Bookmark(bookmark) => {
            if bookmark.bookmark.name() == context.this_bookmark {
                let title = context
                    .merge_request_lookup
                    .get(bookmark.name())
                    .expect("Self-bookmark should always have an MR")
                    .title();

                let mr_name = context.format_merge_request.mr_name();

                format!(
                    r#"{prefix}{list_indicator} **"{title}" (this {mr_name}){suffix}**{compares}"#
                )
            } else if let Some(mr) = context.merge_request_lookup.get(bookmark.name()) {
                let id = context
                    .format_merge_request
                    .format_merge_request_id(mr.iid());

                let title = if context.format_merge_request.id_expands_title() {
                    String::new()
                } else {
                    format!(r#" "{}""#, mr.title())
                };

                format!("{prefix}{list_indicator} {id}{title}{suffix}{compares}")
            } else {
                // Bookmark without MR (yet)
                format!("{prefix}{list_indicator} `{}`", bookmark.name())
            }
        }
        BookmarkRef::Trunk => {
            format!("{prefix}{list_indicator} `{}`", context.base_branch)
        }
    }
}

pub const START_MARKER: &str = "<!-- start jj-vine stack -->";
pub const END_MARKER: &str = "<!-- end jj-vine stack -->";

/// Context for building stack visualizations.
pub struct FormatContext<'a, 'forge, 'lookup, S: BuildHasher = RandomState> {
    /// The component to format.
    pub component: ChangeComponent<'a>,

    /// The name of the bookmark of the current MR.
    pub this_bookmark: String,

    /// Lookup of merge requests by bookmark name.
    pub merge_request_lookup: &'lookup HashMap<String, AnyForgeMergeRequest, S>,

    /// Base branch name (e.g., "main", "master").
    pub base_branch: String,

    /// Forge implementation to use for formatting merge request IDs.
    pub format_merge_request: &'forge ForgeImpl,
}

/// Generate a new description with stack visualization and user content.
#[must_use]
#[expect(clippy::module_name_repetitions, reason = "is sentence")]
pub fn insert_stack_into_description<'a>(
    stack_description: &str,
    existing_description: &'a str,
) -> Cow<'a, str> {
    if stack_description.is_empty() {
        return Cow::Borrowed(existing_description);
    }

    let mut result = String::new();

    #[expect(clippy::string_slice, reason = "index found via find()")]
    let (before, after) = match (
        existing_description.find(START_MARKER),
        existing_description.find(END_MARKER),
    ) {
        (Some(start), Some(end)) if start < end => (
            existing_description[..start].trim(),
            existing_description[end.strict_add(END_MARKER.len())..].trim(),
        ),
        _ => (existing_description.trim(), ""),
    };

    if !before.is_empty() {
        writeln!(result, "{before}\n").unwrap();
    }

    write!(result, "{START_MARKER}\n{stack_description}\n{END_MARKER}").unwrap();

    if !after.is_empty() {
        write!(result, "\n\n{after}").unwrap();
    }

    Cow::Owned(result)
}

/// Generates a description for a bookmark in a stack.
#[expect(clippy::module_name_repetitions, reason = "is sentence")]
pub fn generate_stack_description(
    bookmark: &str,
    component: &ChangeComponent,
    existing_mrs: &HashMap<String, AnyForgeMergeRequest, impl BuildHasher>,
    config: &DescriptionConfig,
    base_branch: &str,
    format_merge_request: &ForgeImpl,
) -> String {
    let formatter = |format: DescriptionDiagramFormat| match format {
        DescriptionDiagramFormat::None => Formatter::None,
        DescriptionDiagramFormat::Linear => Formatter::LinearList(LinearListFormatter),
        DescriptionDiagramFormat::Tree => Formatter::Tree(TreeFormatter),
    };

    let context = FormatContext {
        component: (*component).clone(),
        this_bookmark: bookmark.to_owned(),
        merge_request_lookup: existing_mrs,
        base_branch: base_branch.to_owned(),
        format_merge_request,
    };

    match component {
        component if component.len() == 1 => {
            formatter(config.diagram.single).format_single(&context)
        }
        component if component.is_linear() => {
            formatter(config.diagram.linear).format_linear(&context)
        }
        component if component.is_tree() => formatter(config.diagram.tree).format_tree(&context),
        _ => formatter(config.diagram.complex).format_graph(&context),
    }
}

#[must_use]
#[expect(clippy::module_name_repetitions, reason = "is sentence")]
pub fn remove_jj_vine_stack_from_description(description: &str) -> String {
    #[expect(clippy::string_slice, reason = "index found via find()")]
    let (before, after) = match (description.find(START_MARKER), description.find(END_MARKER)) {
        (Some(start), Some(end)) if start < end => (
            description[..start].trim(),
            description[end.strict_add(END_MARKER.len())..].trim(),
        ),
        _ => (description.trim(), ""),
    };

    format!("{before}\n\n{after}").trim().to_owned()
}

/// Generates the description for a branch based on the commits in the
/// branch and the configuration.
#[expect(clippy::module_name_repetitions, reason = "is sentence")]
pub fn generate_description(
    config: &DescriptionConfig,
    branch_commits: impl AsRef<[Change]>,
    repository_root: &Path,
) -> String {
    if !config.enabled {
        return String::new();
    }

    let mut branch_commits = toposort(
        branch_commits.as_ref(),
        |c| c.commit_id.clone(),
        |c| c.parent_commit_ids.clone(),
    );

    // We want the head commit first, not any leaf
    branch_commits.reverse();

    match &branch_commits[..] {
        [] => String::new(),
        [single_commit] => match &config.single_revision {
            DescriptionMode::None => String::new(),
            DescriptionMode::NotFirstLine => single_commit
                .description
                .lines()
                .skip(1)
                .join("\n")
                .trim()
                .to_owned(),
            DescriptionMode::FullMessage => single_commit.description.clone(),
            DescriptionMode::CommitListFirstLine => format!(
                "- `{}` {}",
                single_commit.commit_id.chars().take(8).collect::<String>(),
                single_commit
                    .description
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
            ),
            DescriptionMode::CommitListFull => format!(
                "- `{}` {}",
                single_commit.commit_id.chars().take(8).collect::<String>(),
                single_commit
                    .description
                    .trim()
                    .lines()
                    .map(|l| format!("{l}\\"))
                    .join("\n")
                    .trim_end_matches('\\')
            ),
            DescriptionMode::File(path) => read_file(repository_root, Path::new(path)),
        },
        [head_commit, ..] => match &config.multiple_revisions {
            DescriptionMode::None => String::new(),
            DescriptionMode::NotFirstLine => head_commit
                .description
                .lines()
                .skip(1)
                .join("\n")
                .trim()
                .to_owned(),
            DescriptionMode::FullMessage => head_commit.description.clone(),
            DescriptionMode::CommitListFirstLine => branch_commits
                .iter()
                .map(|c| {
                    format!(
                        "- `{}` {}",
                        c.commit_id.chars().take(8).collect::<String>(),
                        c.description.lines().next().unwrap_or_default().trim()
                    )
                })
                .join("\n"),
            DescriptionMode::CommitListFull => branch_commits
                .iter()
                .map(|c| {
                    format!(
                        "- `{}` {}",
                        c.commit_id.chars().take(8).collect::<String>(),
                        c.description
                            .trim()
                            .lines()
                            .map(|l| format!("{l}\\"))
                            .join("\n")
                            .trim_end_matches('\\')
                    )
                })
                .join("\n"),
            DescriptionMode::File(path) => read_file(repository_root, Path::new(path)),
        },
    }
}

fn read_file(repository_root: &Path, path: &Path) -> String {
    std::fs::read_to_string(repository_root.join(path)).unwrap_or_default()
}

/// Renders 1 or more links that compare a bookmark with its parent. Used
/// for forks because forges hate stacked PRs.
fn render_compare_links(
    source: &BookmarkRef,
    base: Option<&BookmarkRef>,
    context: &FormatContext<impl BuildHasher>,
) -> String {
    // If not a fork, then the normal PR diff view will work just fine.
    if !context.format_merge_request.is_fork() {
        return String::new();
    }

    match source {
        BookmarkRef::Trunk => String::new(),
        BookmarkRef::Bookmark(bookmark) => {
            let parents: Vec<&BookmarkRef> = match base {
                Some(base) => vec![base],
                None => bookmark.parents.iter().collect::<Vec<_>>(),
            };

            match &parents[..] {
                [] => String::new(),
                [parent] => context
                    .format_merge_request
                    .mr_diff_url(source, parent, &context.base_branch)
                    .map(|mr_diff_url| format!(" ([Compare]({mr_diff_url}))"))
                    .unwrap_or_default(),
                parents => parents
                    .iter()
                    .map(|parent| -> Result<String> {
                        let url = context.format_merge_request.mr_diff_url(
                            source,
                            parent,
                            &context.base_branch,
                        )?;

                        let id_or_name = parent
                            .name()
                            .and_then(|name| context.merge_request_lookup.get(name))
                            .map(|mr| {
                                context
                                    .format_merge_request
                                    .format_merge_request_id(mr.iid())
                            })
                            .or(parent.name().map(ToOwned::to_owned))
                            .unwrap_or(context.base_branch.clone());

                        Ok(format!("([Compare with {id_or_name}]({url}))"))
                    })
                    .collect::<Result<Vec<String>>>()
                    .map(|links| format!(" {}", links.join(" ")))
                    .unwrap_or_default(),
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::too_many_lines, reason = "they're fine")]
mod tests {
    use pretty_assertions::assert_str_eq;

    use super::*;
    use crate::{
        bookmark::BookmarkGraph,
        forge::test::{MergeRequest, TestForge},
        jj::Change,
    };

    #[test]
    fn parse_empty_description() {
        assert_str_eq!(insert_stack_into_description("", ""), "");
    }

    #[test]
    fn parse_user_content_only() {
        assert_str_eq!(
            insert_stack_into_description("", "User's description here"),
            "User's description here"
        );
    }

    #[test]
    fn parse_preserves_user_content_after_markers() {
        assert_str_eq!(
            insert_stack_into_description("Stack info", "User content"),
            format!("User content\n\n{START_MARKER}\nStack info\n{END_MARKER}")
        );
    }

    #[test]
    fn linear_generate_linear_component() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_linear(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

1. `main`
2. #1 "Feature A"
3. **"Feature B" (this MR)**
4. #3 "Feature C""#
        );
    }

    #[test]
    fn linear_generate_linear_component_fork() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .base_url("https://forge.local".to_owned())
            .target_project_id("proj-1".to_owned())
            .source_project_id("proj-2".to_owned())
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_linear(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

1. `main`
2. #1 "Feature A"
3. **"Feature B" (this MR)** ([Compare](https://forge.local/proj-1/compare/feature-a..proj-2:feature-b))
4. #3 "Feature C" ([Compare](https://forge.local/proj-1/compare/feature-b..proj-2:feature-c))"#
        );
    }

    #[test]
    fn linear_generate_tree_component() {
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

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks(["feature-c"]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_tree(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a tree containing 8 MRs:

1. `main`
2. #1 "Feature A" → `main`
3. #4 "Feature D" → #1
4. #2 "Feature B" → #1
5. **"Feature E" (this MR) → #2**
6. #3 "Feature C" → #2
7. #7 "Feature G" → #3
8. #8 "Feature H" → #7
9. #6 "Feature F" → #3"#
        );
    }

    #[test]
    fn linear_generate_complex_component() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-i"),
            Change::mock_from_bookmark("feature-j"),
        ]));

        changes.insert(
            Change::mock_from_bookmark("feature-d")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-e")
                .with_mock_parent_bookmarks(["feature-b", "feature-j"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-f")
                .with_mock_parent_bookmarks(["feature-c", "feature-i"]),
        );

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks([
                "feature-c",
                "feature-j",
                "feature-e",
            ]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-i".to_owned(),
                    MergeRequest::builder()
                        .id("9".to_owned())
                        .title("Feature I".to_owned())
                        .source_branch("feature-i".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-j".to_owned(),
                    MergeRequest::builder()
                        .id("10".to_owned())
                        .title("Feature J".to_owned())
                        .source_branch("feature-j".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_graph(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a complex set of MRs containing 10 MRs:

1. `main`
2. #9 "Feature I" → `main`
3. #10 "Feature J" → #9
4. #1 "Feature A" → `main`
5. #2 "Feature B" → #1
6. **"Feature E" (this MR) → #2, #10**
7. #4 "Feature D" → #1, #2
8. #3 "Feature C" → #2
9. #7 "Feature G" → #3, #5, #10
10. #8 "Feature H" → #7
11. #6 "Feature F" → #3, #9"#
        );
    }

    #[test]
    fn linear_generate_complex_component_fork() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-i"),
            Change::mock_from_bookmark("feature-j"),
        ]));

        changes.insert(
            Change::mock_from_bookmark("feature-d")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-e")
                .with_mock_parent_bookmarks(["feature-b", "feature-j"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-f")
                .with_mock_parent_bookmarks(["feature-c", "feature-i"]),
        );

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks([
                "feature-c",
                "feature-j",
                "feature-e",
            ]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .base_url("https://forge.local".to_owned())
            .target_project_id("proj-1".to_owned())
            .source_project_id("proj-2".to_owned())
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-i".to_owned(),
                    MergeRequest::builder()
                        .id("9".to_owned())
                        .title("Feature I".to_owned())
                        .source_branch("feature-i".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-j".to_owned(),
                    MergeRequest::builder()
                        .id("10".to_owned())
                        .title("Feature J".to_owned())
                        .source_branch("feature-j".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_graph(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a complex set of MRs containing 10 MRs:

1. `main`
2. #9 "Feature I" → `main`
3. #10 "Feature J" → #9 ([Compare](https://forge.local/proj-1/compare/feature-i..proj-2:feature-j))
4. #1 "Feature A" → `main`
5. #2 "Feature B" → #1 ([Compare](https://forge.local/proj-1/compare/feature-a..proj-2:feature-b))
6. **"Feature E" (this MR) → #2, #10** ([Compare with #2](https://forge.local/proj-1/compare/feature-b..proj-2:feature-e)) ([Compare with #10](https://forge.local/proj-1/compare/feature-j..proj-2:feature-e))
7. #4 "Feature D" → #1, #2 ([Compare with #1](https://forge.local/proj-1/compare/feature-a..proj-2:feature-d)) ([Compare with #2](https://forge.local/proj-1/compare/feature-b..proj-2:feature-d))
8. #3 "Feature C" → #2 ([Compare](https://forge.local/proj-1/compare/feature-b..proj-2:feature-c))
9. #7 "Feature G" → #3, #5, #10 ([Compare with #3](https://forge.local/proj-1/compare/feature-c..proj-2:feature-g)) ([Compare with #5](https://forge.local/proj-1/compare/feature-e..proj-2:feature-g)) ([Compare with #10](https://forge.local/proj-1/compare/feature-j..proj-2:feature-g))
10. #8 "Feature H" → #7 ([Compare](https://forge.local/proj-1/compare/feature-g..proj-2:feature-h))
11. #6 "Feature F" → #3, #9 ([Compare with #3](https://forge.local/proj-1/compare/feature-c..proj-2:feature-f)) ([Compare with #9](https://forge.local/proj-1/compare/feature-i..proj-2:feature-f))"#
        );
    }

    #[test]
    fn tree_generate_linear_component() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_linear(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

- `main`
    - #1 "Feature A"
        - **"Feature B" (this MR)**
            - #3 "Feature C""#
        );
    }

    #[test]
    fn tree_generate_tree_component() {
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

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks(["feature-c"]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_tree(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a tree containing 8 MRs:

- `main`
    - #1 "Feature A"
        1. #2 "Feature B"
            1. #3 "Feature C"
                1. #7 "Feature G"
                    - #8 "Feature H"
                2. #6 "Feature F"
            2. **"Feature E" (this MR)**
        2. #4 "Feature D""#
        );
    }

    #[test]
    fn tree_generate_complex_component() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-i"),
            Change::mock_from_bookmark("feature-j"),
        ]));

        changes.insert(
            Change::mock_from_bookmark("feature-d")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-e")
                .with_mock_parent_bookmarks(["feature-b", "feature-j"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-f")
                .with_mock_parent_bookmarks(["feature-c", "feature-i"]),
        );

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks([
                "feature-c",
                "feature-j",
                "feature-e",
            ]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-i".to_owned(),
                    MergeRequest::builder()
                        .id("9".to_owned())
                        .title("Feature I".to_owned())
                        .source_branch("feature-i".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-j".to_owned(),
                    MergeRequest::builder()
                        .id("10".to_owned())
                        .title("Feature J".to_owned())
                        .source_branch("feature-j".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_graph(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a complex set of MRs containing 10 MRs:

- `main`
    1. #9 "Feature I"
        1. #10 "Feature J"
            1. #7 "Feature G" (→ #3, #5 also)
                - #8 "Feature H"
            2. **"Feature E" (this MR) (→ #2 also)**
                - #7 "Feature G" (→ #3, #10 also)
                    - #8 "Feature H"
        2. #6 "Feature F" (→ #3 also)
    2. #1 "Feature A"
        1. #2 "Feature B"
            1. **"Feature E" (this MR) (→ #10 also)**
                - #7 "Feature G" (→ #3, #10 also)
                    - #8 "Feature H"
            2. #3 "Feature C"
                1. #7 "Feature G" (→ #5, #10 also)
                    - #8 "Feature H"
                2. #6 "Feature F" (→ #9 also)
            3. #4 "Feature D" (→ #1 also)
        2. #4 "Feature D" (→ #2 also)"#
        );
    }

    #[test]
    fn tree_generate_complex_component_fork() {
        let mut changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-i"),
            Change::mock_from_bookmark("feature-j"),
        ]));

        changes.insert(
            Change::mock_from_bookmark("feature-d")
                .with_mock_parent_bookmarks(["feature-a", "feature-b"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-e")
                .with_mock_parent_bookmarks(["feature-b", "feature-j"]),
        );

        changes.insert(
            Change::mock_from_bookmark("feature-f")
                .with_mock_parent_bookmarks(["feature-c", "feature-i"]),
        );

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks([
                "feature-c",
                "feature-j",
                "feature-e",
            ]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .base_url("https://forge.local".to_owned())
            .target_project_id("proj-1".to_owned())
            .source_project_id("proj-2".to_owned())
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-i".to_owned(),
                    MergeRequest::builder()
                        .id("9".to_owned())
                        .title("Feature I".to_owned())
                        .source_branch("feature-i".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-j".to_owned(),
                    MergeRequest::builder()
                        .id("10".to_owned())
                        .title("Feature J".to_owned())
                        .source_branch("feature-j".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_graph(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a complex set of MRs containing 10 MRs:

- `main`
    1. #9 "Feature I" ([Compare](https://forge.local/proj-1/compare/main..proj-2:feature-i))
        1. #10 "Feature J" ([Compare](https://forge.local/proj-1/compare/feature-i..proj-2:feature-j))
            1. #7 "Feature G" (→ #3, #5 also) ([Compare](https://forge.local/proj-1/compare/feature-j..proj-2:feature-g))
                - #8 "Feature H" ([Compare](https://forge.local/proj-1/compare/feature-g..proj-2:feature-h))
            2. **"Feature E" (this MR) (→ #2 also)** ([Compare](https://forge.local/proj-1/compare/feature-j..proj-2:feature-e))
                - #7 "Feature G" (→ #3, #10 also) ([Compare](https://forge.local/proj-1/compare/feature-e..proj-2:feature-g))
                    - #8 "Feature H" ([Compare](https://forge.local/proj-1/compare/feature-g..proj-2:feature-h))
        2. #6 "Feature F" (→ #3 also) ([Compare](https://forge.local/proj-1/compare/feature-i..proj-2:feature-f))
    2. #1 "Feature A" ([Compare](https://forge.local/proj-1/compare/main..proj-2:feature-a))
        1. #2 "Feature B" ([Compare](https://forge.local/proj-1/compare/feature-a..proj-2:feature-b))
            1. **"Feature E" (this MR) (→ #10 also)** ([Compare](https://forge.local/proj-1/compare/feature-b..proj-2:feature-e))
                - #7 "Feature G" (→ #3, #10 also) ([Compare](https://forge.local/proj-1/compare/feature-e..proj-2:feature-g))
                    - #8 "Feature H" ([Compare](https://forge.local/proj-1/compare/feature-g..proj-2:feature-h))
            2. #3 "Feature C" ([Compare](https://forge.local/proj-1/compare/feature-b..proj-2:feature-c))
                1. #7 "Feature G" (→ #5, #10 also) ([Compare](https://forge.local/proj-1/compare/feature-c..proj-2:feature-g))
                    - #8 "Feature H" ([Compare](https://forge.local/proj-1/compare/feature-g..proj-2:feature-h))
                2. #6 "Feature F" (→ #9 also) ([Compare](https://forge.local/proj-1/compare/feature-c..proj-2:feature-f))
            3. #4 "Feature D" (→ #1 also) ([Compare](https://forge.local/proj-1/compare/feature-b..proj-2:feature-d))
        2. #4 "Feature D" (→ #2 also) ([Compare](https://forge.local/proj-1/compare/feature-a..proj-2:feature-d))"#
        );
    }

    #[test]
    fn round_trip_preserves_user_content() {
        assert_str_eq!(
            insert_stack_into_description(
                "New stack",
                &format!(
                    "My notes before\n\n{START_MARKER}\nOld stack\n{END_MARKER}\n\nMy notes after"
                )
            ),
            format!("My notes before\n\n{START_MARKER}\nNew stack\n{END_MARKER}\n\nMy notes after")
        );
    }

    #[test]
    fn generate_no_trailing_whitespace_when_no_user_content() {
        assert_str_eq!(
            insert_stack_into_description("New stack", ""),
            format!("{START_MARKER}\nNew stack\n{END_MARKER}")
        );
    }

    #[test]
    fn generate_initial_description_none() {
        assert_str_eq!(
            generate_description(&DescriptionConfig::default(), &[], Path::new("")),
            ""
        );
    }

    fn mock_commit(commit_id: &str, parents: impl AsRef<[&'static str]>) -> Change {
        Change {
            commit_id: commit_id.to_owned(),
            parent_commit_ids: parents.as_ref().iter().map(ToString::to_string).collect(),
            change_id: format!("change_{commit_id}"),
            description: "Message\n\nBody".to_owned(),
            bookmarks: vec![],
            pending_bookmark: false,
        }
    }

    #[test]
    fn generate_initial_description_single_revision_none() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    single_revision: DescriptionMode::None,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            ""
        );
    }

    #[test]
    fn generate_initial_description_single_revision_not_first_line() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    single_revision: DescriptionMode::NotFirstLine,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            "Body"
        );
    }

    #[test]
    fn generate_initial_description_single_revision_full_message() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    single_revision: DescriptionMode::FullMessage,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            "Message\n\nBody"
        );
    }

    #[test]
    fn generate_initial_description_single_revision_commit_list_first_line() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    single_revision: DescriptionMode::CommitListFirstLine,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            "- `commit-1` Message"
        );
    }

    #[test]
    fn generate_initial_description_single_revision_commit_list_full() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    single_revision: DescriptionMode::CommitListFull,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            "- `commit-1` Message\\\n\\\nBody"
        );
    }

    #[test]
    fn generate_initial_description_multiple_revisions_none() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    multiple_revisions: DescriptionMode::None,
                    ..DescriptionConfig::default()
                },
                &[
                    mock_commit("commit-1", []),
                    mock_commit("commit-2", ["commit-1"]),
                ],
                Path::new("")
            ),
            ""
        );
    }

    #[test]
    fn generate_initial_description_multiple_revisions_not_first_line() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    multiple_revisions: DescriptionMode::NotFirstLine,
                    ..DescriptionConfig::default()
                },
                &[
                    mock_commit("commit-1", []),
                    mock_commit("commit-2", ["commit-1"]),
                ],
                Path::new("")
            ),
            "Body"
        );
    }

    #[test]
    fn generate_initial_description_multiple_revisions_full_message() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    multiple_revisions: DescriptionMode::FullMessage,
                    ..DescriptionConfig::default()
                },
                &[
                    mock_commit("commit-1", []),
                    mock_commit("commit-2", ["commit-1"]),
                ],
                Path::new("")
            ),
            "Message\n\nBody"
        );
    }

    #[test]
    fn generate_initial_description_multiple_revisions_commit_list_first_line() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    multiple_revisions: DescriptionMode::CommitListFirstLine,
                    ..DescriptionConfig::default()
                },
                &[
                    mock_commit("commit-1", []),
                    mock_commit("commit-2", ["commit-1"]),
                ],
                Path::new("")
            ),
            "- `commit-2` Message\n- `commit-1` Message"
        );
    }

    #[test]
    fn generate_initial_description_multiple_revisions_commit_list_full() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    multiple_revisions: DescriptionMode::CommitListFull,
                    ..DescriptionConfig::default()
                },
                &[
                    mock_commit("commit-1", []),
                    mock_commit("commit-2", ["commit-1"]),
                ],
                Path::new("")
            ),
            "- `commit-2` Message\\\n\\\nBody\n- `commit-1` Message\\\n\\\nBody"
        );
    }

    #[test]
    fn generate_initial_description_disabled() {
        assert_str_eq!(
            generate_description(
                &DescriptionConfig {
                    enabled: false,
                    ..DescriptionConfig::default()
                },
                &[mock_commit("commit-1", []),],
                Path::new("")
            ),
            ""
        );
    }

    #[test]
    fn linear_generate_linear_component_github_style() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .id_expands_title(true)
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_linear(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

1. `main`
2. #1
3. **"Feature B" (this MR)**
4. #3"#
        );
    }

    #[test]
    fn linear_generate_tree_component_github_style() {
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

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks(["feature-c"]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .id_expands_title(true)
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_tree(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a tree containing 8 MRs:

1. `main`
2. #1 → `main`
3. #4 → #1
4. #2 → #1
5. **"Feature E" (this MR) → #2**
6. #3 → #2
7. #7 → #3
8. #8 → #7
9. #6 → #3"#
        );
    }

    #[test]
    fn tree_generate_linear_component_github_style() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .id_expands_title(true)
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_linear(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

- `main`
    - #1
        - **"Feature B" (this MR)**
            - #3"#
        );
    }

    #[test]
    fn tree_generate_tree_component_github_style() {
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

        changes.extend(Change::mock_stack_map([
            Change::mock_from_bookmark("feature-g").with_mock_parent_bookmarks(["feature-c"]),
            Change::mock_from_bookmark("feature-h"),
        ]));

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            &changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = Formatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_owned(),
                    MergeRequest::builder()
                        .id("1".to_owned())
                        .title("Feature A".to_owned())
                        .source_branch("feature-a".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-b".to_owned(),
                    MergeRequest::builder()
                        .id("2".to_owned())
                        .title("Feature B".to_owned())
                        .source_branch("feature-b".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-c".to_owned(),
                    MergeRequest::builder()
                        .id("3".to_owned())
                        .title("Feature C".to_owned())
                        .source_branch("feature-c".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-d".to_owned(),
                    MergeRequest::builder()
                        .id("4".to_owned())
                        .title("Feature D".to_owned())
                        .source_branch("feature-d".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-e".to_owned(),
                    MergeRequest::builder()
                        .id("5".to_owned())
                        .title("Feature E".to_owned())
                        .source_branch("feature-e".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-f".to_owned(),
                    MergeRequest::builder()
                        .id("6".to_owned())
                        .title("Feature F".to_owned())
                        .source_branch("feature-f".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-g".to_owned(),
                    MergeRequest::builder()
                        .id("7".to_owned())
                        .title("Feature G".to_owned())
                        .source_branch("feature-g".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
                (
                    "feature-h".to_owned(),
                    MergeRequest::builder()
                        .id("8".to_owned())
                        .title("Feature H".to_owned())
                        .source_branch("feature-h".to_owned())
                        .target_branch("main".to_owned())
                        .build(),
                ),
            ]))
            .id_expands_title(true)
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_owned(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_owned(),
            format_merge_request: &ForgeImpl::Test(forge),
        };
        let description = formatter.format_tree(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a tree containing 8 MRs:

- `main`
    - #1
        1. #2
            1. #3
                1. #7
                    - #8
                2. #6
            2. **"Feature E" (this MR)**
        2. #4"#
        );
    }
}
