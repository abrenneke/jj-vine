use crate::config::Config;
use crate::error::Result;
use crate::gitlab::GitLabClient;
use crate::jj::Jujutsu;
use crate::submit::analyze::SubmissionAnalysis;

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
    },

    /// Update the target branch (base) of an existing MR
    UpdateMRBase {
        bookmark: String,
        mr_iid: u64,
        new_target_branch: String,
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

    for (idx, bookmark) in analysis.bookmarks_to_submit.iter().enumerate() {
        let remote_branch = config.apply_branch_prefix(bookmark);

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
            let prev_bookmark = &analysis.bookmarks_to_submit[idx - 1];
            config.apply_branch_prefix(prev_bookmark)
        };

        // Check if an MR already exists
        match gitlab.find_mr_by_source_branch(&remote_branch).await? {
            Some(existing_mr) => {
                // MR exists - check if we need to update the target branch
                if existing_mr.target_branch != target_branch {
                    actions.push(Action::UpdateMRBase {
                        bookmark: bookmark.clone(),
                        mr_iid: existing_mr.iid,
                        new_target_branch: target_branch,
                    });
                }
                // If target branch is correct, no action needed
            }
            None => {
                // No MR exists - create one
                let title = format!("[jj-mrs] {}", bookmark);

                actions.push(Action::CreateMR {
                    bookmark: bookmark.clone(),
                    target_branch,
                    title,
                });
            }
        }
    }

    Ok(SubmissionPlan { actions, dry_run })
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
        } = action
        {
            assert_eq!(bookmark, "feature");
            assert_eq!(target_branch, "main");
            assert_eq!(title, "[jj-mrs] feature");
        }
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
                },
            ],
            dry_run: false,
        };

        assert_eq!(plan.actions.len(), 2);
        assert!(!plan.dry_run);
    }
}
