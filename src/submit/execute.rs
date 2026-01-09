use crate::config::Config;
use crate::error::Result;
use crate::gitlab::{GitLabClient, MergeRequest};
use crate::jj::Jujutsu;
use crate::output::Output;
use crate::submit::plan::{Action, SubmissionPlan};
use console::style;
use std::sync::Arc;
use tracing::error;

/// Result of executing a submission plan
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// All MRs (created, updated, and unchanged)
    pub merge_requests: Vec<MRUpdate>,

    /// Any errors that occurred (non-fatal)
    pub errors: Vec<String>,

    /// Bookmarks that were successfully pushed
    pub bookmarks_pushed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MRUpdate {
    pub mr: MergeRequest,
    pub bookmark: String,
    pub update_type: MRUpdateType,
}

/// Type of MR update
#[derive(Debug, Clone)]
pub enum MRUpdateType {
    /// MR was unchanged
    Unchanged,

    /// MR was created
    Created,

    /// Target branch was changed (repointed)
    Repointed {
        old_target: String,
        new_target: String,
    },

    /// Description was updated
    DescriptionUpdated,

    /// Both target and description updated
    Both {
        old_target: String,
        new_target: String,
    },
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
    output: Arc<Output>,
) -> Result<SubmissionResult> {
    let mut merge_requests = Vec::new();
    let mut errors = Vec::new();
    let mut failed_pushes = std::collections::HashSet::new();
    let mut bookmarks_pushed = Vec::new();

    if plan.dry_run {
        output.log_message("DRY RUN - No changes will be made");
    }

    output.log_current("Preparing submission...");

    for action in &plan.actions {
        match action {
            Action::Push { bookmark, remote } => {
                if plan.dry_run {
                    output.log_current(format!("Would push {} to {}", bookmark, remote));
                    output.log_message(format!("Would push {} to {}", bookmark, remote));
                } else {
                    output.log_current(format!("Pushing {}...", style(bookmark).magenta()));

                    match jj.push_bookmark(bookmark, remote) {
                        Ok(_) => {
                            output.log_completed(format!("Pushed {}", style(bookmark).magenta()));
                            bookmarks_pushed.push(bookmark.clone());
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to push {}: {}", bookmark, e);
                            output.log_message(&error_msg);
                            error!("{}", error_msg);
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
                    output.log_message(&error_msg);
                    errors.push(error_msg);
                    continue;
                }

                if plan.dry_run {
                    let msg = format!(
                        "Would create MR: {} -> {} (title: {})",
                        bookmark, target_branch, title
                    );
                    output.log_current(&msg);
                    output.log_message(&msg);
                } else {
                    output.log_current(format!(
                        "Creating MR: {} -> {}",
                        style(bookmark).green(),
                        target_branch
                    ));

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
                            output.log_completed(format!(
                                "Created MR {}: {}",
                                style(format!("!{}", mr.iid)).cyan(),
                                style(&mr.web_url).dim()
                            ));
                            merge_requests.push(MRUpdate {
                                mr,
                                bookmark: bookmark.clone(),
                                update_type: MRUpdateType::Created,
                            });
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to create MR for {}: {}", bookmark, e);
                            output.log_message(&error_msg);
                            error!("{}", error_msg);
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
                    let msg = format!(
                        "Would update MR !{} base for {} to {}",
                        mr_iid, bookmark, new_target_branch
                    );
                    output.log_current(&msg);
                    output.log_message(&msg);
                } else {
                    output.log_current(format!(
                        "Updating MR {} base...",
                        style(format!("!{}", mr_iid)).cyan()
                    ));

                    // Get old target before update
                    let old_target = if let Ok(Some(existing_mr)) =
                        gitlab.find_mr_by_source_branch(bookmark).await
                    {
                        existing_mr.target_branch.clone()
                    } else {
                        "unknown".to_string()
                    };

                    match gitlab.update_mr_base(*mr_iid, new_target_branch).await {
                        Ok(mr) => {
                            output.log_completed(format!(
                                "Updated MR {}",
                                style(format!("!{}", mr.iid)).cyan()
                            ));
                            merge_requests.push(MRUpdate {
                                mr,
                                bookmark: bookmark.clone(),
                                update_type: MRUpdateType::Repointed {
                                    old_target,
                                    new_target: new_target_branch.clone(),
                                },
                            });
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to update MR base for {}: {}", bookmark, e);
                            output.log_message(&error_msg);
                            error!("{}", error_msg);
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
                    let msg = format!("Would update MR description for {}", bookmark);
                    output.log_current(&msg);
                    output.log_message(&msg);
                } else {
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
                        match crate::description::generate_multi_stack_description(
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
                                    // Don't show any output for unchanged descriptions
                                    merge_requests.push(MRUpdate {
                                        mr: current_mr.clone(),
                                        bookmark: bookmark.clone(),
                                        update_type: MRUpdateType::Unchanged,
                                    });
                                } else {
                                    output.log_current(format!(
                                        "Updating MR {} description...",
                                        style(format!("!{}", current_mr.iid)).cyan()
                                    ));

                                    match gitlab
                                        .update_mr_description(current_mr.iid, &new_description)
                                        .await
                                    {
                                        Ok(updated_mr) => {
                                            output.log_completed(format!(
                                                "Updated MR {} description",
                                                style(format!("!{}", updated_mr.iid)).cyan()
                                            ));
                                            merge_requests.push(MRUpdate {
                                                mr: updated_mr,
                                                bookmark: bookmark.clone(),
                                                update_type: MRUpdateType::DescriptionUpdated,
                                            });
                                        }
                                        Err(e) => {
                                            let error_msg = format!(
                                                "Failed to update MR description for {}: {}",
                                                bookmark, e
                                            );
                                            output.log_message(&error_msg);
                                            error!("{}", error_msg);
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
                                output.log_message(&error_msg);
                                error!("{}", error_msg);
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
        errors,
        bookmarks_pushed,
    })
}
