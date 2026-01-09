use crate::config::Config;
use crate::error::Result;
use crate::gitlab::{GitLabClient, MergeRequest};
use crate::jj::Jujutsu;
use crate::submit::plan::{Action, SubmissionPlan};
use tracing::{error as log_error, info};

/// Result of executing a submission plan
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// All MRs (created, updated, and unchanged)
    pub merge_requests: Vec<MergeRequest>,

    /// Number of MRs created
    pub mrs_created: usize,

    /// Number of MRs updated
    pub mrs_updated: usize,

    /// Number of MRs unchanged
    pub mrs_unchanged: usize,

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
    let mut failed_pushes = std::collections::HashSet::new();
    let mut mrs_created = 0;
    let mut mrs_updated = 0;
    let mut mrs_unchanged = 0;

    if plan.dry_run {
        info!("DRY RUN - No changes will be made");
    }

    for action in &plan.actions {
        match action {
            Action::Push { bookmark, remote } => {
                if plan.dry_run {
                    info!("Would push {} to {}", bookmark, remote);
                } else {
                    info!("Pushing {} to {}...", bookmark, remote);

                    match jj.push_bookmark(bookmark, remote) {
                        Ok(_) => {
                            info!("Pushed {}", bookmark);
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to push {}: {}", bookmark, e);
                            log_error!(error_msg);
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
                    info!(error_msg);
                    errors.push(error_msg);
                    continue;
                }

                if plan.dry_run {
                    info!(
                        "Would create MR: {} -> {} (title: {})",
                        bookmark, target_branch, title
                    );
                } else {
                    info!("Creating MR: {} -> {}", bookmark, target_branch);

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
                            info!("Created MR !{}: {}", mr.iid, mr.web_url);
                            mrs_created += 1;
                            merge_requests.push(mr);
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to create MR for {}: {}", bookmark, e);
                            log_error!(error_msg);
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
                    info!(
                        "Would update MR !{} base for {} to {}",
                        mr_iid, bookmark, new_target_branch
                    );
                } else {
                    info!(
                        "Updating MR !{} base for {} to {}",
                        mr_iid, bookmark, new_target_branch
                    );

                    match gitlab.update_mr_base(*mr_iid, new_target_branch).await {
                        Ok(mr) => {
                            info!("Updated MR !{}", mr.iid);
                            merge_requests.push(mr);
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to update MR base for {}: {}", bookmark, e);
                            log_error!(error_msg);
                            errors.push(error_msg);
                        }
                    }
                }
            }

            Action::UpdateMRDescription {
                bookmark,
                bookmark_graph,
                bookmarks_being_submitted: _,
            } => {
                if plan.dry_run {
                    info!("Would update MR description for {}", bookmark);
                } else {
                    info!("Updating MR description for {}", bookmark);

                    // Find stacks that contain this bookmark
                    let containing_stacks: Vec<&crate::bookmark::BranchStack> = bookmark_graph
                        .stacks
                        .iter()
                        .filter(|stack| stack.bookmarks.contains(&bookmark.to_string()))
                        .collect();

                    // Query MRs only for bookmarks in these stacks
                    let mut all_mrs = std::collections::HashMap::new();
                    for stack in &containing_stacks {
                        for bm in &stack.bookmarks {
                            if !all_mrs.contains_key(bm)
                                && let Ok(Some(mr)) = gitlab.find_mr_by_source_branch(bm).await
                            {
                                all_mrs.insert(bm.clone(), mr);
                            }
                        }
                    }

                    // Get current MR (which contains existing description)
                    if let Some(current_mr) = all_mrs.get(bookmark) {
                        let existing_description = current_mr.description.as_deref().unwrap_or("");

                        // Create DescriptionManager
                        let formatter: Box<dyn crate::description::DescriptionFormatter> =
                            match config.stack_format {
                                crate::config::StackFormat::Linear => {
                                    Box::new(crate::description::LinearListFormatter)
                                }
                            };
                        let desc_manager = crate::description::DescriptionManager::new(formatter);

                        // Parse existing description to extract user content before and after
                        let parsed = desc_manager.parse_description(existing_description);

                        // Generate new stack section (without markers)
                        match crate::submit::plan::generate_multi_stack_description(
                            bookmark,
                            &containing_stacks,
                            &all_mrs,
                            &config.stack_format,
                            &config.default_branch,
                        ) {
                            Ok(stack_content) => {
                                // Build complete description with preserved user content
                                let new_description = desc_manager.build_description(
                                    parsed.content_before.as_deref(),
                                    parsed.content_after.as_deref(),
                                    &stack_content,
                                );

                                // Diff check - only update if changed
                                if existing_description == new_description {
                                    info!(
                                        "Skipping MR !{} description (unchanged)",
                                        current_mr.iid
                                    );
                                    mrs_unchanged += 1;
                                    merge_requests.push(current_mr.clone());
                                } else {
                                    match gitlab
                                        .update_mr_description(current_mr.iid, &new_description)
                                        .await
                                    {
                                        Ok(updated_mr) => {
                                            info!("Updated MR !{} description", updated_mr.iid);
                                            mrs_updated += 1;
                                            merge_requests.push(updated_mr);
                                        }
                                        Err(e) => {
                                            let error_msg = format!(
                                                "Failed to update MR description for {}: {}",
                                                bookmark, e
                                            );
                                            log_error!(error_msg);
                                            errors.push(error_msg);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let error_msg = format!(
                                    "Failed to generate description for {}: {}",
                                    bookmark, e
                                );
                                log_error!(error_msg);
                                errors.push(error_msg);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(SubmissionResult {
        merge_requests,
        mrs_created,
        mrs_updated,
        mrs_unchanged,
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
            mrs_created: 0,
            mrs_updated: 0,
            mrs_unchanged: 0,
            errors: Vec::new(),
        };

        assert_eq!(result.merge_requests.len(), 0);
        assert_eq!(result.mrs_created, 0);
        assert_eq!(result.mrs_updated, 0);
        assert_eq!(result.mrs_unchanged, 0);
        assert_eq!(result.errors.len(), 0);
    }

    // Regression test for push failure handling:
    // When a bookmark push fails, the corresponding MR creation should be skipped.
    // This is tested through integration tests with actual jj commands.
    // The fix adds failed_pushes HashSet tracking and checks it before CreateMR actions.
}
