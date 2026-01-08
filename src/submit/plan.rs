use crate::config::{Config, StackFormat};
use crate::description::{
    DescriptionFormatter, DescriptionManager, LinearListFormatter, StackBookmarkInfo, StackContext,
};
use crate::error::Result;
use crate::gitlab::{GitLabClient, MergeRequest};
use crate::jj::Jujutsu;
use crate::submit::analyze::SubmissionAnalysis;
use std::collections::HashMap;

/// Action to perform during execution
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Push a bookmark to remote
    Push { bookmark: String, remote: String },

    /// Create a new merge request
    CreateMR {
        bookmark: String,
        target_branch: String,
        title: String,
        description: String,
    },

    /// Update the target branch (base) of an existing MR
    UpdateMRBase {
        bookmark: String,
        mr_iid: u64,
        new_target_branch: String,
    },

    /// Update the description of an existing MR
    UpdateMRDescription {
        bookmark: String,
        mr_iid: u64,
        new_description: String,
    },
}

/// Plan for submission execution
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Actions to perform, in order
    pub actions: Vec<Action>,

    /// Whether this is a dry run (don't actually execute)
    pub dry_run: bool,
}

/// Create a submission plan based on analysis
///
/// Phase 2 of the three-phase submission process:
/// - Query GitLab for existing MRs
/// - Check remote bookmark status
/// - Determine what actions are needed
pub async fn plan(
    analysis: &SubmissionAnalysis,
    _jj: &Jujutsu,
    gitlab: &GitLabClient,
    config: &Config,
    dry_run: bool,
) -> Result<SubmissionPlan> {
    let mut actions = Vec::new();

    // Query all existing MRs upfront (needed for description generation)
    let mut existing_mrs = HashMap::new();
    for bookmark in &analysis.bookmarks_to_submit {
        if let Some(mr) = gitlab.find_mr_by_source_branch(bookmark).await? {
            existing_mrs.insert(bookmark.clone(), mr);
        }
    }

    // Generate descriptions if stack visualization is enabled
    let bookmark_descriptions = if config.enable_stack_visualization {
        generate_descriptions(
            &analysis.bookmarks_to_submit,
            &analysis.base_branch,
            &existing_mrs,
            &config.stack_format,
        )?
    } else {
        HashMap::new()
    };

    for (idx, bookmark) in analysis.bookmarks_to_submit.iter().enumerate() {
        // Always push the bookmark
        actions.push(Action::Push {
            bookmark: bookmark.clone(),
            remote: config.remote_name.clone(),
        });

        // Determine the target branch for this bookmark's MR
        let target_branch = if idx == 0 {
            // First bookmark in stack -> target the base branch
            analysis.base_branch.clone()
        } else {
            // Other bookmarks -> target the previous bookmark
            analysis.bookmarks_to_submit[idx - 1].clone()
        };

        // Get generated description (or empty if visualization disabled)
        let description = bookmark_descriptions
            .get(bookmark)
            .cloned()
            .unwrap_or_default();

        // Check if an MR already exists
        match existing_mrs.get(bookmark) {
            Some(existing_mr) => {
                // MR exists - check if we need to update the target branch
                if existing_mr.target_branch != target_branch {
                    actions.push(Action::UpdateMRBase {
                        bookmark: bookmark.clone(),
                        mr_iid: existing_mr.iid,
                        new_target_branch: target_branch.clone(),
                    });
                }

                // Check if we need to update the description
                if config.enable_stack_visualization {
                    let current_desc = existing_mr.description.as_deref().unwrap_or("");
                    if current_desc != description {
                        actions.push(Action::UpdateMRDescription {
                            bookmark: bookmark.clone(),
                            mr_iid: existing_mr.iid,
                            new_description: description,
                        });
                    }
                }
            }
            None => {
                // No MR exists - create one
                let title = bookmark.to_string();

                actions.push(Action::CreateMR {
                    bookmark: bookmark.clone(),
                    target_branch,
                    title,
                    description,
                });
            }
        }
    }

    Ok(SubmissionPlan { actions, dry_run })
}

/// Generate MR descriptions for all bookmarks in the stack
fn generate_descriptions(
    bookmarks: &[String],
    base_branch: &str,
    existing_mrs: &HashMap<String, MergeRequest>,
    stack_format: &StackFormat,
) -> Result<HashMap<String, String>> {
    // Create formatter based on config
    let formatter: Box<dyn DescriptionFormatter> = match stack_format {
        StackFormat::Linear => Box::new(LinearListFormatter),
    };

    let manager = DescriptionManager::new(formatter);
    let mut descriptions = HashMap::new();

    // Build stack context with all bookmarks
    let stack_context = StackContext {
        bookmarks: bookmarks
            .iter()
            .map(|b| StackBookmarkInfo {
                name: b.clone(),
                mr_iid: existing_mrs.get(b).map(|mr| mr.iid),
                mr_url: existing_mrs.get(b).map(|mr| mr.web_url.clone()),
            })
            .collect(),
        base_branch: base_branch.to_string(),
    };

    // Generate description for each bookmark
    for bookmark in bookmarks {
        // Parse existing description to extract user content
        let user_content = if let Some(mr) = existing_mrs.get(bookmark) {
            if let Some(desc) = &mr.description {
                let parsed = manager.parse_description(desc);
                parsed.user_content
            } else {
                None
            }
        } else {
            None
        };

        // Generate new description with stack visualization + user content
        let description =
            manager.generate_description(user_content.as_deref(), &stack_context, bookmark);

        descriptions.insert(bookmark.clone(), description);
    }

    Ok(descriptions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_push() {
        let action = Action::Push {
            bookmark: "feature".to_string(),
            remote: "origin".to_string(),
        };

        assert!(
            matches!(&action, Action::Push { .. }),
            "Expected Push action, got {:?}",
            action
        );

        if let Action::Push { bookmark, remote } = action {
            assert_eq!(bookmark, "feature");
            assert_eq!(remote, "origin");
        }
    }

    #[test]
    fn test_action_create_mr() {
        let action = Action::CreateMR {
            bookmark: "feature".to_string(),
            target_branch: "main".to_string(),
            title: "[jj-mrs] feature".to_string(),
            description: "Stack visualization here".to_string(),
        };

        assert!(
            matches!(&action, Action::CreateMR { .. }),
            "Expected CreateMR action, got {:?}",
            action
        );

        if let Action::CreateMR {
            bookmark,
            target_branch,
            title,
            description,
        } = action
        {
            assert_eq!(bookmark, "feature");
            assert_eq!(target_branch, "main");
            assert_eq!(title, "[jj-mrs] feature");
            assert_eq!(description, "Stack visualization here");
        }
    }

    #[test]
    fn test_action_create_mr_with_description() {
        let action = Action::CreateMR {
            bookmark: "test".to_string(),
            target_branch: "main".to_string(),
            title: "Test".to_string(),
            description: "Stack info".to_string(),
        };
        assert!(matches!(action, Action::CreateMR { .. }));
    }

    #[test]
    fn test_submission_plan_struct() {
        let plan = SubmissionPlan {
            actions: vec![
                Action::Push {
                    bookmark: "feature".to_string(),
                    remote: "origin".to_string(),
                },
                Action::CreateMR {
                    bookmark: "feature".to_string(),
                    target_branch: "main".to_string(),
                    title: "[jj-mrs] feature".to_string(),
                    description: "".to_string(),
                },
            ],
            dry_run: false,
        };

        assert_eq!(plan.actions.len(), 2);
        assert!(!plan.dry_run);
    }

    #[test]
    fn test_action_update_mr_description() {
        let action = Action::UpdateMRDescription {
            bookmark: "test".to_string(),
            mr_iid: 123,
            new_description: "New stack".to_string(),
        };
        assert!(matches!(action, Action::UpdateMRDescription { .. }));
    }
}
