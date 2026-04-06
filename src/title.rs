use regex::Regex;
use snafu::whatever;

use crate::{
    bookmark::{BookmarkWithPointers, ChangeComponent},
    config::{TitleConfig, TitleFormat},
    error::{Error, Result},
    jj::{Change, Jujutsu},
};

pub fn get_mr_title(
    jj: &Jujutsu,
    config: &TitleConfig,
    bookmark: BookmarkWithPointers<'_>,
    component: &ChangeComponent<'_>,
    revisions: Vec<Change>,
) -> Result<String> {
    let default_branch = jj.default_branch()?;

    Ok(get_mr_title_from_revisions(
        config,
        bookmark,
        component,
        default_branch,
        revisions,
    ))
}

fn get_mr_title_from_revisions(
    config: &TitleConfig,
    bookmark: BookmarkWithPointers<'_>,
    component: &ChangeComponent<'_>,
    default_branch: &str,
    revisions: Vec<Change>,
) -> String {
    match &revisions[..] {
        [] => bookmark.name().to_string(),
        [revision] => match &config.single_revision {
            TitleFormat::FirstRevisionFirstLine => revision.description_first_line().to_string(),
            TitleFormat::FirstRevisionFullMessage => revision.description.to_string(),
            TitleFormat::HeadRevisionFirstLine => revision.description_first_line().to_string(),
            TitleFormat::HeadRevisionFullMessage => revision.description.to_string(),
            TitleFormat::BookmarkName => bookmark.name().to_string(),
            TitleFormat::Other(template) => format_title_with_template(
                revision,
                revision,
                &bookmark,
                component,
                default_branch,
                template,
            ),
        },
        [head, .., root] => match &config.multiple_revisions {
            TitleFormat::FirstRevisionFirstLine => root.description_first_line().to_string(),
            TitleFormat::FirstRevisionFullMessage => root.description.to_string(),
            TitleFormat::HeadRevisionFirstLine => head.description_first_line().to_string(),
            TitleFormat::HeadRevisionFullMessage => head.description.to_string(),
            TitleFormat::BookmarkName => bookmark.name().to_string(),
            TitleFormat::Other(template) => format_title_with_template(
                root,
                head,
                &bookmark,
                component,
                default_branch,
                template,
            ),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Replacement {
    /// The jj commit/revision ID of the first (bottommost) revision.
    FirstRevisionId,

    /// The jj change ID of the first (bottommost) revision.
    FirstRevisionChangeId,

    /// The full description of the first (bottommost) revision.
    FirstRevisionDescription,

    /// The first line of the description of the first (bottommost) revision.
    FirstRevisionDescriptionFirstLine,

    /// The description of the first (bottommost) revision, excluding the first
    /// line (trimmed).
    FirstRevisionDescriptionNotFirstLine,

    /// The jj commit/revision ID of the head revision.
    HeadRevisionId,

    /// The jj change ID of the head revision.
    HeadRevisionChangeId,

    /// The full description of the head revision.
    HeadRevisionDescription,

    /// The first line of the description of the head revision.
    HeadRevisionDescriptionFirstLine,

    /// The description of the head revision, excluding the first line
    /// (trimmed).
    HeadRevisionDescriptionNotFirstLine,

    /// The name of the bookmark.
    BookmarkName,

    /// The name of the parent bookmark (i.e. the target branch).
    ParentBookmarkName,

    /// The 1-based index of the change in the stack. For use with e.g.
    /// `{stackIndex}/{stackCount}`
    StackIndex,

    /// The total number of changes in the stack.
    StackCount,
}

impl std::str::FromStr for Replacement {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "first.id" => Ok(Replacement::FirstRevisionId),
            "first.change_id" => Ok(Replacement::FirstRevisionChangeId),
            "first.description" => Ok(Replacement::FirstRevisionDescription),
            "first.description_first_line" => Ok(Replacement::FirstRevisionDescriptionFirstLine),
            "first.description_not_first_line" => {
                Ok(Replacement::FirstRevisionDescriptionNotFirstLine)
            }
            "head.id" => Ok(Replacement::HeadRevisionId),
            "head.change_id" => Ok(Replacement::HeadRevisionChangeId),
            "head.description" => Ok(Replacement::HeadRevisionDescription),
            "head.description_first_line" => Ok(Replacement::HeadRevisionDescriptionFirstLine),
            "head.description_not_first_line" => {
                Ok(Replacement::HeadRevisionDescriptionNotFirstLine)
            }
            "stack_index" => Ok(Replacement::StackIndex),
            "stack_count" => Ok(Replacement::StackCount),
            "bookmark_name" => Ok(Replacement::BookmarkName),
            "parent_bookmark_name" => Ok(Replacement::ParentBookmarkName),
            _ => whatever!("Invalid replacement: {}", s),
        }
    }
}

fn format_title_with_template(
    first_revision: &Change,
    head_revision: &Change,
    bookmark: &BookmarkWithPointers<'_>,
    component: &ChangeComponent<'_>,
    default_branch: &str,
    template: &str,
) -> String {
    let mut title = template.to_string();

    for needs_replacement in Regex::new(r"\{[\w.]+\}").unwrap().find_iter(template) {
        let enum_value: Result<Replacement, _> = needs_replacement
            .as_str()
            .trim_matches('{')
            .trim_matches('}')
            .parse();

        match enum_value {
            Ok(Replacement::FirstRevisionId) => {
                title = title.replace(needs_replacement.as_str(), &first_revision.commit_id);
            }
            Ok(Replacement::FirstRevisionChangeId) => {
                title = title.replace(needs_replacement.as_str(), &first_revision.change_id);
            }
            Ok(Replacement::FirstRevisionDescription) => {
                title = title.replace(needs_replacement.as_str(), &first_revision.description);
            }
            Ok(Replacement::FirstRevisionDescriptionFirstLine) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    first_revision.description_first_line(),
                );
            }
            Ok(Replacement::FirstRevisionDescriptionNotFirstLine) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    first_revision.description_not_first_line(),
                );
            }
            Ok(Replacement::HeadRevisionId) => {
                title = title.replace(needs_replacement.as_str(), &head_revision.commit_id);
            }
            Ok(Replacement::HeadRevisionChangeId) => {
                title = title.replace(needs_replacement.as_str(), &head_revision.change_id);
            }
            Ok(Replacement::HeadRevisionDescription) => {
                title = title.replace(needs_replacement.as_str(), &head_revision.description);
            }
            Ok(Replacement::HeadRevisionDescriptionFirstLine) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    head_revision.description_first_line(),
                );
            }
            Ok(Replacement::HeadRevisionDescriptionNotFirstLine) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    head_revision.description_not_first_line(),
                );
            }
            Ok(Replacement::StackIndex) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    &bookmark.downstack().len().to_string(),
                );
            }
            Ok(Replacement::StackCount) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    &component.all_bookmarks().len().to_string(),
                );
            }
            Ok(Replacement::BookmarkName) => {
                title = title.replace(needs_replacement.as_str(), bookmark.name());
            }
            Ok(Replacement::ParentBookmarkName) => {
                title = title.replace(
                    needs_replacement.as_str(),
                    &bookmark.parent_name(default_branch),
                );
            }
            Err(_) => {
                // ignore
            }
        }
    }

    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bookmark::BookmarkGraph, utils::toposort};

    fn get_mock_title(config: TitleConfig, changes: impl IntoIterator<Item = Change>) -> String {
        let changes = Change::mock_stack_map(changes);
        let graph = BookmarkGraph::from_lookups(
            changes.create_bookmark_map(),
            changes.create_adjacency_list(),
        );
        let component = graph.components().first().unwrap();
        let bookmark = component.leaves.first().unwrap().clone();
        let revisions: Vec<_> = toposort(
            changes.values().cloned(),
            |c| c.commit_id.to_string(),
            |c| c.parent_commit_ids.clone(),
        )
        .into_iter()
        .rev() // head first like jj outputs
        .collect();

        get_mr_title_from_revisions(&config, bookmark, component, "main", revisions)
    }

    #[test]
    fn test_single_revision_first_revision_first_line() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::FirstRevisionFirstLine,
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1");
    }

    #[test]
    fn test_single_revision_first_revision_full_message() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::FirstRevisionFullMessage,
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1\nLine 2");
    }

    #[test]
    fn test_single_revision_head_revision_first_line() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::HeadRevisionFirstLine,
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1");
    }

    #[test]
    fn test_single_revision_head_revision_full_message() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::HeadRevisionFullMessage,
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1\nLine 2");
    }

    #[test]
    fn test_single_revision_bookmark_name() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::BookmarkName,
                ..Default::default()
            },
            [Change::mock_from_bookmark("commit-a")],
        );

        assert_eq!(title, "commit-a");
    }

    #[test]
    fn test_single_revision_custom_template_first_description_first_line() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other("{first.description_first_line}".to_string()),
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1");
    }

    #[test]
    fn test_single_revision_custom_template_first_description() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other("{first.description}".to_string()),
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 1\nLine 2");
    }

    #[test]
    fn test_single_revision_custom_template_first_description_not_first_line() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other(
                    "{first.description_not_first_line}".to_string(),
                ),
                ..Default::default()
            },
            [Change {
                description: "Line 1\nLine 2\nLine 3".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Line 2\nLine 3");
    }

    #[test]
    fn test_single_revision_custom_template_ids() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other(
                    "{first.id} {first.change_id} {head.id} {head.change_id}".to_string(),
                ),
                ..Default::default()
            },
            [Change::mock_from_bookmark("commit-a")],
        );

        assert_eq!(
            title,
            "commit_commit-a change_commit-a commit_commit-a change_commit-a"
        );
    }

    #[test]
    fn test_single_revision_custom_template_with_literal_text() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other(
                    "MR: {first.description_first_line} (rev {first.id})".to_string(),
                ),
                ..Default::default()
            },
            [Change {
                description: "Add feature".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "MR: Add feature (rev commit_commit-a)");
    }

    #[test]
    fn test_single_revisions_custom_template_stack_index_and_count() {
        let changes = Change::mock_stack_map([
            Change::mock_from_bookmark("commit-a"),
            Change::mock_from_bookmark("commit-b"),
        ]);

        let bookmark_map = changes.create_bookmark_map();
        let graph =
            BookmarkGraph::from_lookups(bookmark_map.clone(), changes.create_adjacency_list());
        let component = &graph.components()[0];
        let bookmark_a = component.find("commit-a").unwrap();
        let bookmark_b = component.find("commit-b").unwrap();

        let revisions_a = vec![changes.get("commit_commit-a").unwrap().clone()];
        let revisions_b = vec![changes.get("commit_commit-b").unwrap().clone()];

        let config = TitleConfig {
            single_revision: TitleFormat::Other("{stack_index}/{stack_count}".to_string()),
            ..Default::default()
        };

        let title = get_mr_title_from_revisions(
            &config,
            bookmark_a.clone(),
            component,
            "main",
            revisions_a,
        );
        assert_eq!(title, "1/2");

        let title = get_mr_title_from_revisions(
            &config,
            bookmark_b.clone(),
            component,
            "main",
            revisions_b,
        );
        assert_eq!(title, "2/2");
    }

    #[test]
    fn test_multiple_revisions_first_revision_first_line() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::FirstRevisionFirstLine,
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change::mock_from_change_id("commit-b"),
                Change {
                    description: "Head desc\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-c")
                },
            ],
        );

        assert_eq!(title, "Root desc");
    }

    #[test]
    fn test_multiple_revisions_first_revision_full_message() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::FirstRevisionFullMessage,
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head desc\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "Root desc\nRoot body");
    }

    #[test]
    fn test_multiple_revisions_head_revision_first_line() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::HeadRevisionFirstLine,
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head desc\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "Head desc");
    }

    #[test]
    fn test_multiple_revisions_head_revision_full_message() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::HeadRevisionFullMessage,
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head desc\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "Head desc\nHead body");
    }

    #[test]
    fn test_multiple_revisions_bookmark_name() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::BookmarkName,
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head desc".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "commit-b");
    }

    #[test]
    fn test_multiple_revisions_custom_template_first_and_head() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::Other(
                    "{first.description_first_line} -> {head.description_first_line}".to_string(),
                ),
                ..Default::default()
            },
            [
                Change {
                    description: "Root desc\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head desc\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "Root desc -> Head desc");
    }

    #[test]
    fn test_multiple_revisions_custom_template_ids() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::Other(
                    "{first.id} {first.change_id} | {head.id} {head.change_id}".to_string(),
                ),
                ..Default::default()
            },
            [
                Change::mock_from_change_id("commit-a"),
                Change::mock_from_bookmark("commit-b"),
            ],
        );

        assert_eq!(
            title,
            "commit_commit-a commit-a | commit_commit-b change_commit-b"
        );
    }

    #[test]
    fn test_multiple_revisions_custom_template_description_not_first_line() {
        let title = get_mock_title(
            TitleConfig {
                multiple_revisions: TitleFormat::Other(
                    "{first.description_not_first_line} | {head.description_not_first_line}"
                        .to_string(),
                ),
                ..Default::default()
            },
            [
                Change {
                    description: "Root title\nRoot body".to_string(),
                    ..Change::mock_from_change_id("commit-a")
                },
                Change {
                    description: "Head title\nHead body".to_string(),
                    ..Change::mock_from_bookmark("commit-b")
                },
            ],
        );

        assert_eq!(title, "Root body | Head body");
    }

    #[test]
    fn test_multiple_revisions_custom_template_stack_index_and_count() {
        let changes = Change::mock_stack_map([
            Change::mock_from_change_id("commit-a"),
            Change::mock_from_bookmark("commit-b"),
            Change::mock_from_change_id("commit-c"),
            Change::mock_from_bookmark("commit-d"),
        ]);

        let bookmark_map = changes.create_bookmark_map();
        let graph =
            BookmarkGraph::from_lookups(bookmark_map.clone(), changes.create_adjacency_list());
        let component = &graph.components()[0];
        let bookmark_b = component.find("commit-b").unwrap();
        let bookmark_d = component.find("commit-d").unwrap();
        let revisions_b = vec![
            changes.get("commit_commit-b").unwrap().clone(),
            changes.get("commit_commit-a").unwrap().clone(),
        ];
        let revisions_d = vec![
            changes.get("commit_commit-d").unwrap().clone(),
            changes.get("commit_commit-c").unwrap().clone(),
        ];

        let config = TitleConfig {
            multiple_revisions: TitleFormat::Other("{stack_index}/{stack_count}".to_string()),
            ..Default::default()
        };

        let title = get_mr_title_from_revisions(
            &config,
            bookmark_b.clone(),
            component,
            "main",
            revisions_b,
        );
        assert_eq!(title, "1/2");

        let title = get_mr_title_from_revisions(
            &config,
            bookmark_d.clone(),
            component,
            "main",
            revisions_d,
        );
        assert_eq!(title, "2/2");
    }

    #[test]
    fn test_empty_revisions_returns_bookmark_name() {
        let changes = Change::mock_stack_map([Change::mock_from_bookmark("commit-a")]);
        let bookmark_map = changes.create_bookmark_map();
        let graph =
            BookmarkGraph::from_lookups(bookmark_map.clone(), changes.create_adjacency_list());
        let component = &graph.components()[0];
        let bookmark = component.leaves[0].clone();

        let title = get_mr_title_from_revisions(
            &TitleConfig::default(),
            bookmark,
            component,
            "main",
            vec![],
        );

        assert_eq!(title, "commit-a");
    }

    #[test]
    fn test_template_unknown_replacement_left_as_is() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other(
                    "{first.description_first_line} {unknown_token}".to_string(),
                ),
                ..Default::default()
            },
            [Change::mock_from_bookmark("commit-a")],
        );

        assert_eq!(title, "description_commit-a {unknown_token}");
    }

    #[test]
    fn test_template_no_replacements() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other("Static title".to_string()),
                ..Default::default()
            },
            [Change::mock_from_bookmark("commit-a")],
        );

        assert_eq!(title, "Static title");
    }

    #[test]
    fn test_template_multiple_same_replacement() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::Other(
                    "{first.description_first_line} - {first.description_first_line}".to_string(),
                ),
                ..Default::default()
            },
            [Change {
                description: "Title".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Title - Title");
    }

    #[test]
    fn test_single_revision_description_only_one_line() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::FirstRevisionFirstLine,
                ..Default::default()
            },
            [Change {
                description: "Only one line".to_string(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "Only one line");
    }

    #[test]
    fn test_single_revision_empty_description() {
        let title = get_mock_title(
            TitleConfig {
                single_revision: TitleFormat::FirstRevisionFirstLine,
                ..Default::default()
            },
            [Change {
                description: String::new(),
                ..Change::mock_from_bookmark("commit-a")
            }],
        );

        assert_eq!(title, "");
    }
}
