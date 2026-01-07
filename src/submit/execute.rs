use crate::config::Config;
use crate::error::Result;
use crate::gitlab::{GitLabClient, MergeRequest};
use crate::jj::Jujutsu;
use crate::output;
use crate::submit::plan::{Action, SubmissionPlan};

/// Result of executing a submission plan
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// MRs that were created or updated
    pub merge_requests: Vec<MergeRequest>,

    /// Any errors that occurred (non-fatal)
    pub errors: Vec<String>,
}

/// Execute a submission plan
///
/// Phase 3 of the three-phase submission process:
/// - Push bookmarks to remote
/// - Create or update merge requests
pub async fn execute(
    plan: &SubmissionPlan,
    jj: &Jujutsu,
    gitlab: &GitLabClient,
    config: &Config,
) -> Result<SubmissionResult> {
    let mut merge_requests = Vec::new();
    let mut errors = Vec::new();

    if plan.dry_run {
        output::output("DRY RUN - No changes will be made")?;
    }

    for action in &plan.actions {
        match action {
            Action::Push { bookmark, remote } => {
                let remote_branch = config.apply_branch_prefix(bookmark);

                if plan.dry_run {
                    output::output(&format!("Would push {} to {}/{}", bookmark, remote, remote_branch))?;
                } else {
                    output::output(&format!("Pushing {} to {}...", bookmark, remote))?;

                    match jj.push_bookmark(bookmark, remote) {
                        Ok(_) => {
                            output::output(&format!("Pushed {}", bookmark))?;
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to push {}: {}", bookmark, e);
                            output::error(&error_msg)?;
                            errors.push(error_msg);
                        }
                    }
                }
            }

            Action::CreateMR {
                bookmark,
                target_branch,
                title,
            } => {
                let source_branch = config.apply_branch_prefix(bookmark);

                if plan.dry_run {
                    output::output(&format!(
                        "Would create MR: {} -> {} (title: {})",
                        source_branch, target_branch, title
                    ))?;
                } else {
                    output::output(&format!("Creating MR: {} -> {}", source_branch, target_branch))?;

                    match gitlab
                        .create_merge_request(&source_branch, target_branch, title, None)
                        .await
                    {
                        Ok(mr) => {
                            output::output(&format!("Created MR !{}: {}", mr.iid, mr.web_url))?;
                            merge_requests.push(mr);
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to create MR for {}: {}", bookmark, e);
                            output::error(&error_msg)?;
                            errors.push(error_msg);
                        }
                    }
                }
            }

            Action::UpdateMRBase {
                bookmark,
                mr_iid,
                new_target_branch,
            } => {
                if plan.dry_run {
                    output::output(&format!(
                        "Would update MR !{} base for {} to {}",
                        mr_iid, bookmark, new_target_branch
                    ))?;
                } else {
                    output::output(&format!(
                        "Updating MR !{} base for {} to {}",
                        mr_iid, bookmark, new_target_branch
                    ))?;

                    match gitlab.update_mr_base(*mr_iid, new_target_branch).await {
                        Ok(mr) => {
                            output::output(&format!("Updated MR !{}", mr.iid))?;
                            merge_requests.push(mr);
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to update MR base for {}: {}", bookmark, e);
                            output::error(&error_msg)?;
                            errors.push(error_msg);
                        }
                    }
                }
            }
        }
    }

    Ok(SubmissionResult {
        merge_requests,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submission_result_struct() {
        let result = SubmissionResult {
            merge_requests: Vec::new(),
            errors: Vec::new(),
        };

        assert_eq!(result.merge_requests.len(), 0);
        assert_eq!(result.errors.len(), 0);
    }
}
