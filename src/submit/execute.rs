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
    _config: &Config,
) -> Result<SubmissionResult> {
    let mut merge_requests = Vec::new();
    let mut errors = Vec::new();
    let mut failed_pushes = std::collections::HashSet::new();

    if plan.dry_run {
        output::output("DRY RUN - No changes will be made")?;
    }

    for action in &plan.actions {
        match action {
            Action::Push { bookmark, remote } => {
                if plan.dry_run {
                    output::output(&format!("Would push {} to {}", bookmark, remote))?;
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
                            failed_pushes.insert(bookmark.clone());
                        }
                    }
                }
            }

            Action::CreateMR {
                bookmark,
                target_branch,
                title,
                description,
            } => {
                // Skip MR creation if the push for this bookmark failed
                if failed_pushes.contains(bookmark) {
                    let error_msg =
                        format!("Skipping MR creation for {} because push failed", bookmark);
                    output::output(&error_msg)?;
                    errors.push(error_msg);
                    continue;
                }

                if plan.dry_run {
                    output::output(&format!(
                        "Would create MR: {} -> {} (title: {})",
                        bookmark, target_branch, title
                    ))?;
                } else {
                    output::output(&format!("Creating MR: {} -> {}", bookmark, target_branch))?;

                    let desc = if description.is_empty() {
                        None
                    } else {
                        Some(description.as_str())
                    };

                    match gitlab
                        .create_merge_request(bookmark, target_branch, title, desc)
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

            Action::UpdateMRDescription {
                bookmark,
                mr_iid,
                new_description,
            } => {
                if plan.dry_run {
                    output::output(&format!(
                        "Would update MR !{} description for {}",
                        mr_iid, bookmark
                    ))?;
                } else {
                    output::output(&format!(
                        "Updating MR !{} description for {}",
                        mr_iid, bookmark
                    ))?;

                    match gitlab.update_mr_description(*mr_iid, new_description).await {
                        Ok(mr) => {
                            output::output(&format!("Updated MR !{} description", mr.iid))?;
                            merge_requests.push(mr);
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to update MR !{} description: {}", mr_iid, e);
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

    // Regression test for push failure handling:
    // When a bookmark push fails, the corresponding MR creation should be skipped.
    // This is tested through integration tests with actual jj commands.
    // The fix adds failed_pushes HashSet tracking and checks it before CreateMR actions.
}
