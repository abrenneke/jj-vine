use std::collections::HashMap;

use enum_dispatch::enum_dispatch;
use itertools::Itertools;

use crate::{
    bookmark::{Bookmark, BookmarkRef, ChangeComponent},
    config::StackFormat,
    forge::{ForgeImpl, ForgeMergeRequest},
};

#[enum_dispatch]
pub trait FormatMergeRequest {
    /// Formats the ID of a merge request as a string for display in the
    /// description.
    fn format_merge_request_id(&self, mr_iid: &str) -> String;

    /// Gets e.g. "MR" or "PR" for the merge request.
    fn mr_name(&self) -> &'static str;
}

pub enum DescriptionFormatter {
    LinearList(LinearListFormatter),
}

impl DescriptionFormatter {
    pub fn format_stack(&self, context: &FormatContext) -> String {
        match self {
            DescriptionFormatter::LinearList(formatter) => formatter.format_stack(context),
        }
    }
}

pub struct LinearListFormatter;

impl LinearListFormatter {
    pub fn format_stack(&self, context: &FormatContext) -> String {
        let mut lines = Vec::new();

        let mr_name = context.format_merge_request.mr_name();

        match &context.component {
            component if component.len() == 1 => {
                return String::new();
            }
            component if component.is_linear() => {
                lines.push(format!(
                    "This {mr_name} is part of a stack containing {} {mr_name}s:\n",
                    component.len()
                ));

                lines.push(format!("1. `{}`", context.base_branch));

                let ordered_bookmarks = context
                    .component
                    .topological_sort()
                    .expect("Cycle detected in bookmark graph!");

                for (idx, bookmark_name) in ordered_bookmarks.iter().enumerate() {
                    let bookmark = context
                        .component
                        .find(bookmark_name)
                        .expect("Bookmark not found in component!");

                    lines.push(Self::format_bookmark(
                        &bookmark.bookmark,
                        idx + 1,
                        context,
                        None,
                    ));
                }
            }
            component if component.is_tree() => {
                lines.push(format!(
                    "This {mr_name} is part of a tree containing {} {mr_name}s:\n",
                    component.len()
                ));

                lines.push(format!("1. `{}`", context.base_branch));

                let ordered_bookmarks = component
                    .topological_sort()
                    .expect("Cycle detected in bookmark graph!");

                for (idx, bookmark_name) in ordered_bookmarks.iter().enumerate() {
                    let bookmark = component
                        .find(bookmark_name)
                        .expect("Bookmark not found in component!");
                    lines.push(Self::format_bookmark(
                        &bookmark.bookmark,
                        idx + 1,
                        context,
                        Some(&bookmark.parents),
                    ));
                }
            }
            component => {
                lines.push(format!(
                    "This {mr_name} is part of a complex set of {mr_name}s containing {} {mr_name}s:\n",
                    component.len()
                ));

                lines.push(format!("1. `{}`", context.base_branch));

                let ordered_bookmarks = component
                    .topological_sort()
                    .expect("Cycle detected in bookmark graph!");

                for (idx, bookmark_name) in ordered_bookmarks.iter().enumerate() {
                    let bookmark = component
                        .find(bookmark_name)
                        .expect("Bookmark not found in component!");
                    lines.push(Self::format_bookmark(
                        &bookmark.bookmark,
                        idx + 1,
                        context,
                        Some(&bookmark.parents),
                    ));
                }
            }
        }

        lines.join("\n")
    }

    fn format_bookmark(
        bookmark: &Bookmark<'_>,
        idx: usize,
        context: &FormatContext,
        parents: Option<&[BookmarkRef<'_>]>,
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
                                        .format_merge_request_id(&mr.iid())
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

        if bookmark == context.this_bookmark {
            let title = context
                .merge_request_lookup
                .get(bookmark.name())
                .expect("Self-bookmark should always have an MR")
                .title();
            format!(r#"{}. **"{title}"{into} ← this MR**"#, idx + 1)
        } else if let Some(mr) = context.merge_request_lookup.get(bookmark.name()) {
            format!(
                r#"{}. "{}" {}{}"#,
                idx + 1,
                mr.title(),
                context
                    .format_merge_request
                    .format_merge_request_id(&mr.iid()),
                into
            )
        } else {
            // Bookmark without MR (yet)
            format!("{}. {}", idx + 1, bookmark.name())
        }
    }
}

const START_MARKER: &str = "<!-- start jj-vine stack -->";
const END_MARKER: &str = "<!-- end jj-vine stack -->";

/// Context for building stack visualizations
pub struct FormatContext<'a, 'forge, 'lookup> {
    /// The component to format.
    pub component: ChangeComponent<'a>,

    /// The name of the bookmark of the current MR.
    pub this_bookmark: String,

    /// Lookup of merge requests by bookmark name
    pub merge_request_lookup: &'lookup HashMap<String, ForgeMergeRequest>,

    /// Base branch name (e.g., "main", "master")
    pub base_branch: String,

    /// Forge implementation to use for formatting merge request IDs
    pub format_merge_request: &'forge ForgeImpl,
}

/// Generate a new description with stack visualization and user content
pub fn insert_stack_into_description(
    stack_description: &str,
    existing_description: &str,
) -> String {
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

    result
}

/// Generates a description for a bookmark in a stack.
pub fn generate_stack_description(
    bookmark: &str,
    stack: &ChangeComponent,
    existing_mrs: &HashMap<String, ForgeMergeRequest>,
    format: &StackFormat,
    base_branch: &str,
    format_merge_request: &ForgeImpl,
) -> String {
    let formatter = match format {
        StackFormat::Linear => DescriptionFormatter::LinearList(LinearListFormatter),
    };

    formatter.format_stack(&FormatContext {
        component: (*stack).clone(),
        this_bookmark: bookmark.to_string(),
        merge_request_lookup: existing_mrs,
        base_branch: base_branch.to_string(),
        format_merge_request,
    })
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
        assert_str_eq!(
            insert_stack_into_description("", ""),
            format!("{START_MARKER}\n\n{END_MARKER}")
        );
    }

    #[test]
    fn test_parse_user_content_only() {
        assert_str_eq!(
            insert_stack_into_description("", "User's description here"),
            format!("User's description here\n\n{START_MARKER}\n\n{END_MARKER}")
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
    fn test_generate_linear_component() {
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
        let description = formatter.format_stack(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a stack containing 3 MRs:

1. `main`
2. "Feature A" #1
3. **"Feature B" ← this MR**
4. "Feature C" #3"#
        );
    }

    #[test]
    fn test_generate_tree_component() {
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
        let description = formatter.format_stack(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a tree containing 8 MRs:

1. `main`
2. "Feature A" #1 → `main`
3. "Feature D" #4 → #1
4. "Feature B" #2 → #1
5. **"Feature E" → #2 ← this MR**
6. "Feature C" #3 → #2
7. "Feature G" #7 → #3
8. "Feature H" #8 → #7
9. "Feature F" #6 → #3"#
        );
    }

    #[test]
    fn test_generate_complex_component() {
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
        let description = formatter.format_stack(&context);

        assert_str_eq!(
            description,
            r#"This MR is part of a complex set of MRs containing 10 MRs:

1. `main`
2. "Feature I" #9 → `main`
3. "Feature J" #10 → #9
4. "Feature A" #1 → `main`
5. "Feature B" #2 → #1
6. **"Feature E" → #2, #10 ← this MR**
7. "Feature D" #4 → #1, #2
8. "Feature C" #3 → #2
9. "Feature G" #7 → #3, #5, #10
10. "Feature H" #8 → #7
11. "Feature F" #6 → #3, #9"#
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
}
