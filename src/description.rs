use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::Path,
};

use enum_dispatch::enum_dispatch;
use itertools::Itertools;

use crate::{
    bookmark::{BookmarkRef, ChangeComponent},
    config::{DescriptionConfig, DescriptionDiagramFormat, DescriptionMode},
    forge::{AnyForgeMergeRequest, BorrowId, ForgeImpl, ForgeMergeRequest},
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
}

pub enum DescriptionFormatter {
    None,
    LinearList(LinearListFormatter),
    Tree(TreeFormatter),
}

impl DescriptionFormatter {
    pub fn format_single(&self, context: &FormatContext) -> String {
        match self {
            DescriptionFormatter::None => String::new(),
            DescriptionFormatter::LinearList(formatter) => formatter.format_single(context),
            DescriptionFormatter::Tree(formatter) => formatter.format_single(context),
        }
    }

    pub fn format_linear(&self, context: &FormatContext) -> String {
        match self {
            DescriptionFormatter::None => String::new(),
            DescriptionFormatter::LinearList(formatter) => formatter.format_linear(context),
            DescriptionFormatter::Tree(formatter) => formatter.format_linear(context),
        }
    }

    pub fn format_tree(&self, context: &FormatContext) -> String {
        match self {
            DescriptionFormatter::None => String::new(),
            DescriptionFormatter::LinearList(formatter) => formatter.format_tree(context),
            DescriptionFormatter::Tree(formatter) => formatter.format_tree(context),
        }
    }

    pub fn format_graph(&self, context: &FormatContext) -> String {
        match self {
            DescriptionFormatter::None => String::new(),
            DescriptionFormatter::LinearList(formatter) => formatter.format_graph(context),
            DescriptionFormatter::Tree(formatter) => formatter.format_graph(context),
        }
    }
}

pub struct LinearListFormatter;

impl LinearListFormatter {
    pub fn format_single(&self, context: &FormatContext) -> String {
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

    pub fn format_linear(&self, context: &FormatContext) -> String {
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

    pub fn format_tree(&self, context: &FormatContext) -> String {
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

    pub fn format_graph(&self, context: &FormatContext) -> String {
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
        context: &FormatContext,
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

        let list_indicator = format!("{}.", idx + 1);

        match bookmark {
            BookmarkRef::Bookmark(bookmark) => {
                if bookmark.bookmark.name() == context.this_bookmark {
                    let title = context
                        .merge_request_lookup
                        .get(bookmark.name())
                        .expect("Self-bookmark should always have an MR")
                        .title();
                    format!(
                        r#"{list_indicator} **"{title}" (this {}){into}**"#,
                        context.format_merge_request.mr_name()
                    )
                } else if let Some(mr) = context.merge_request_lookup.get(bookmark.name()) {
                    format!(
                        r#"{list_indicator} {} "{}"{}"#,
                        context
                            .format_merge_request
                            .format_merge_request_id(mr.iid()),
                        mr.title(),
                        into
                    )
                } else {
                    // Bookmark without MR (yet)
                    format!("{list_indicator} `{}`", bookmark.name())
                }
            }
            BookmarkRef::Trunk => {
                format!("{list_indicator} `{}`", context.base_branch)
            }
        }
    }
}

pub struct TreeFormatter;

impl TreeFormatter {
    pub fn format_single(&self, context: &FormatContext) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing 1 {mr_name}:\n",
        ));

        self.format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    pub fn format_linear(&self, context: &FormatContext) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a stack containing {} {mr_name}s:\n",
            context.component.len()
        ));

        self.format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    pub fn format_tree(&self, context: &FormatContext) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a tree containing {} {mr_name}s:\n",
            context.component.len()
        ));

        self.format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    pub fn format_graph(&self, context: &FormatContext) -> String {
        let mut lines = Vec::new();
        let mr_name = context.format_merge_request.mr_name();

        lines.push(format!(
            "This {mr_name} is part of a complex set of {mr_name}s containing {} {mr_name}s:\n",
            context.component.len()
        ));

        self.format_tree_recursive(&BookmarkRef::Trunk, None, 0, context, &mut lines, 0, 0);

        lines.join("\n")
    }

    #[allow(clippy::too_many_arguments)] // It's fine
    fn format_tree_recursive(
        &self,
        item: &BookmarkRef,
        parent: Option<&BookmarkRef>,
        depth: usize,
        context: &FormatContext,
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
            self.format_tree_recursive(
                &BookmarkRef::Bookmark((*child).clone()),
                Some(item),
                depth + 1,
                context,
                lines,
                children.len(),
                idx,
            );
        }
    }

    fn format_bookmark_tree(
        bookmark: &BookmarkRef<'_>,
        parent: Option<&BookmarkRef>,
        idx: usize,
        depth: usize,
        context: &FormatContext,
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
            format!("{}.", idx + 1)
        } else {
            "-".to_string()
        };

        match bookmark {
            BookmarkRef::Bookmark(bookmark) => {
                if bookmark.bookmark.name() == context.this_bookmark {
                    let title = context
                        .merge_request_lookup
                        .get(bookmark.name())
                        .expect("Self-bookmark should always have an MR")
                        .title();
                    format!(
                        r#"{indent}{list_indicator} **"{title}" (this {}){also}**"#,
                        context.format_merge_request.mr_name()
                    )
                } else if let Some(mr) = context.merge_request_lookup.get(bookmark.name()) {
                    format!(
                        r#"{indent}{list_indicator} {} "{}"{}"#,
                        context
                            .format_merge_request
                            .format_merge_request_id(mr.iid()),
                        mr.title(),
                        also
                    )
                } else {
                    // Bookmark without MR (yet)
                    format!("{indent}{list_indicator} `{}`", bookmark.name())
                }
            }
            BookmarkRef::Trunk => {
                format!("{indent}{list_indicator} `{}`", context.base_branch)
            }
        }
    }
}

pub const START_MARKER: &str = "<!-- start jj-vine stack -->";
pub const END_MARKER: &str = "<!-- end jj-vine stack -->";

/// Context for building stack visualizations
pub struct FormatContext<'a, 'forge, 'lookup> {
    /// The component to format.
    pub component: ChangeComponent<'a>,

    /// The name of the bookmark of the current MR.
    pub this_bookmark: String,

    /// Lookup of merge requests by bookmark name
    pub merge_request_lookup: &'lookup HashMap<String, AnyForgeMergeRequest>,

    /// Base branch name (e.g., "main", "master")
    pub base_branch: String,

    /// Forge implementation to use for formatting merge request IDs
    pub format_merge_request: &'forge ForgeImpl,
}

/// Generate a new description with stack visualization and user content
pub fn insert_stack_into_description<'a>(
    stack_description: &str,
    existing_description: &'a str,
) -> Cow<'a, str> {
    if stack_description.is_empty() {
        return Cow::Borrowed(existing_description);
    }

    let mut result = String::new();

    let (before, after) = match (
        existing_description.find(START_MARKER),
        existing_description.find(END_MARKER),
    ) {
        (Some(start), Some(end)) if start < end => (
            existing_description[..start].trim(),
            existing_description[end + END_MARKER.len()..].trim(),
        ),
        _ => (existing_description.trim(), ""),
    };

    if !before.is_empty() {
        result.push_str(&format!("{before}\n\n"));
    }

    result.push_str(&format!(
        "{START_MARKER}\n{}\n{END_MARKER}",
        stack_description
    ));

    if !after.is_empty() {
        result.push_str(&format!("\n\n{after}"));
    }

    Cow::Owned(result)
}

/// Generates a description for a bookmark in a stack.
pub fn generate_stack_description(
    bookmark: &str,
    component: &ChangeComponent,
    existing_mrs: &HashMap<String, AnyForgeMergeRequest>,
    config: &DescriptionConfig,
    base_branch: &str,
    format_merge_request: &ForgeImpl,
) -> String {
    let formatter = |format: DescriptionDiagramFormat| match format {
        DescriptionDiagramFormat::None => DescriptionFormatter::None,
        DescriptionDiagramFormat::Linear => DescriptionFormatter::LinearList(LinearListFormatter),
        DescriptionDiagramFormat::Tree => DescriptionFormatter::Tree(TreeFormatter),
    };

    let context = FormatContext {
        component: (*component).clone(),
        this_bookmark: bookmark.to_string(),
        merge_request_lookup: existing_mrs,
        base_branch: base_branch.to_string(),
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

pub fn remove_jj_vine_stack_from_description(description: &str) -> String {
    let (before, after) = match (description.find(START_MARKER), description.find(END_MARKER)) {
        (Some(start), Some(end)) if start < end => (
            description[..start].trim(),
            description[end + END_MARKER.len()..].trim(),
        ),
        _ => (description.trim(), ""),
    };

    format!("{before}\n\n{after}").trim().to_string()
}

/// Generates the description for a branch based on the commits in the
/// branch and the configuration.
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
                .to_string(),
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
                .to_string(),
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use super::*;
    use crate::{
        bookmark::BookmarkGraph,
        forge::test::{MergeRequest, TestForge},
        jj::Change,
    };

    #[test]
    fn test_parse_empty_description() {
        assert_str_eq!(insert_stack_into_description("", ""), format!(""));
    }

    #[test]
    fn test_parse_user_content_only() {
        assert_str_eq!(
            insert_stack_into_description("", "User's description here"),
            format!("User's description here")
        );
    }

    #[test]
    fn test_parse_preserves_user_content_after_markers() {
        assert_str_eq!(
            insert_stack_into_description("Stack info", "User content"),
            format!("User content\n\n{START_MARKER}\nStack info\n{END_MARKER}")
        );
    }

    #[test]
    fn test_linear_generate_linear_component() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_linear_generate_tree_component() {
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
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-d".to_string(),
                    MergeRequest::builder()
                        .id("4".to_string())
                        .title("Feature D".to_string())
                        .source_branch("feature-d".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-e".to_string(),
                    MergeRequest::builder()
                        .id("5".to_string())
                        .title("Feature E".to_string())
                        .source_branch("feature-e".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-f".to_string(),
                    MergeRequest::builder()
                        .id("6".to_string())
                        .title("Feature F".to_string())
                        .source_branch("feature-f".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-g".to_string(),
                    MergeRequest::builder()
                        .id("7".to_string())
                        .title("Feature G".to_string())
                        .source_branch("feature-g".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-h".to_string(),
                    MergeRequest::builder()
                        .id("8".to_string())
                        .title("Feature H".to_string())
                        .source_branch("feature-h".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_linear_generate_complex_component() {
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
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::LinearList(LinearListFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-d".to_string(),
                    MergeRequest::builder()
                        .id("4".to_string())
                        .title("Feature D".to_string())
                        .source_branch("feature-d".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-e".to_string(),
                    MergeRequest::builder()
                        .id("5".to_string())
                        .title("Feature E".to_string())
                        .source_branch("feature-e".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-f".to_string(),
                    MergeRequest::builder()
                        .id("6".to_string())
                        .title("Feature F".to_string())
                        .source_branch("feature-f".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-g".to_string(),
                    MergeRequest::builder()
                        .id("7".to_string())
                        .title("Feature G".to_string())
                        .source_branch("feature-g".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-h".to_string(),
                    MergeRequest::builder()
                        .id("8".to_string())
                        .title("Feature H".to_string())
                        .source_branch("feature-h".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-i".to_string(),
                    MergeRequest::builder()
                        .id("9".to_string())
                        .title("Feature I".to_string())
                        .source_branch("feature-i".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-j".to_string(),
                    MergeRequest::builder()
                        .id("10".to_string())
                        .title("Feature J".to_string())
                        .source_branch("feature-j".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_tree_generate_linear_component() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("feature-a"),
            Change::mock_from_bookmark("feature-b"),
            Change::mock_from_bookmark("feature-c"),
        ]);

        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-b".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_tree_generate_tree_component() {
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
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-d".to_string(),
                    MergeRequest::builder()
                        .id("4".to_string())
                        .title("Feature D".to_string())
                        .source_branch("feature-d".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-e".to_string(),
                    MergeRequest::builder()
                        .id("5".to_string())
                        .title("Feature E".to_string())
                        .source_branch("feature-e".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-f".to_string(),
                    MergeRequest::builder()
                        .id("6".to_string())
                        .title("Feature F".to_string())
                        .source_branch("feature-f".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-g".to_string(),
                    MergeRequest::builder()
                        .id("7".to_string())
                        .title("Feature G".to_string())
                        .source_branch("feature-g".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-h".to_string(),
                    MergeRequest::builder()
                        .id("8".to_string())
                        .title("Feature H".to_string())
                        .source_branch("feature-h".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_tree_generate_complex_component() {
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
            changes.create_adjacency_list(),
        );

        let component = &graph.components()[0];
        let formatter = DescriptionFormatter::Tree(TreeFormatter);

        let forge = TestForge::builder()
            .merge_requests(HashMap::from([
                (
                    "feature-a".to_string(),
                    MergeRequest::builder()
                        .id("1".to_string())
                        .title("Feature A".to_string())
                        .source_branch("feature-a".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-b".to_string(),
                    MergeRequest::builder()
                        .id("2".to_string())
                        .title("Feature B".to_string())
                        .source_branch("feature-b".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-c".to_string(),
                    MergeRequest::builder()
                        .id("3".to_string())
                        .title("Feature C".to_string())
                        .source_branch("feature-c".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-d".to_string(),
                    MergeRequest::builder()
                        .id("4".to_string())
                        .title("Feature D".to_string())
                        .source_branch("feature-d".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-e".to_string(),
                    MergeRequest::builder()
                        .id("5".to_string())
                        .title("Feature E".to_string())
                        .source_branch("feature-e".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-f".to_string(),
                    MergeRequest::builder()
                        .id("6".to_string())
                        .title("Feature F".to_string())
                        .source_branch("feature-f".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-g".to_string(),
                    MergeRequest::builder()
                        .id("7".to_string())
                        .title("Feature G".to_string())
                        .source_branch("feature-g".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-h".to_string(),
                    MergeRequest::builder()
                        .id("8".to_string())
                        .title("Feature H".to_string())
                        .source_branch("feature-h".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-i".to_string(),
                    MergeRequest::builder()
                        .id("9".to_string())
                        .title("Feature I".to_string())
                        .source_branch("feature-i".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
                (
                    "feature-j".to_string(),
                    MergeRequest::builder()
                        .id("10".to_string())
                        .title("Feature J".to_string())
                        .source_branch("feature-j".to_string())
                        .target_branch("main".to_string())
                        .build(),
                ),
            ]))
            .build();

        let context = FormatContext {
            component: component.clone(),
            this_bookmark: "feature-e".to_string(),
            merge_request_lookup: &forge.merge_request_lookup(),
            base_branch: "main".to_string(),
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
    fn test_round_trip_preserves_user_content() {
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
    fn test_generate_no_trailing_whitespace_when_no_user_content() {
        assert_str_eq!(
            insert_stack_into_description("New stack", ""),
            format!("{START_MARKER}\nNew stack\n{END_MARKER}")
        );
    }

    #[test]
    fn test_generate_initial_description_none() {
        assert_str_eq!(
            generate_description(&DescriptionConfig::default(), &[], Path::new("")),
            ""
        );
    }

    fn mock_commit(commit_id: &str, parents: impl AsRef<[&'static str]>) -> Change {
        Change {
            commit_id: commit_id.to_string(),
            parent_commit_ids: parents.as_ref().iter().map(|p| p.to_string()).collect(),
            change_id: format!("change_{}", commit_id),
            description: "Message\n\nBody".to_string(),
            bookmarks: vec![],
        }
    }

    #[test]
    fn test_generate_initial_description_single_revision_none() {
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
    fn test_generate_initial_description_single_revision_not_first_line() {
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
    fn test_generate_initial_description_single_revision_full_message() {
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
    fn test_generate_initial_description_single_revision_commit_list_first_line() {
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
    fn test_generate_initial_description_single_revision_commit_list_full() {
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
    fn test_generate_initial_description_multiple_revisions_none() {
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
    fn test_generate_initial_description_multiple_revisions_not_first_line() {
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
    fn test_generate_initial_description_multiple_revisions_full_message() {
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
    fn test_generate_initial_description_multiple_revisions_commit_list_first_line() {
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
    fn test_generate_initial_description_multiple_revisions_commit_list_full() {
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
    fn test_generate_initial_description_disabled() {
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
}
