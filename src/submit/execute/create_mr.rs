use bon::Builder;
use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    bookmark::change_id_to_temp_bookmark_name,
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{Forge, ForgeCreateMergeRequestOptions},
    submit::execute::{
        ActionInfo,
        ActionResultData,
        BookmarkNameOrPendingChangeId,
        ExecuteAction,
        ExecuteActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

/// Create a new merge request
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct CreateMRAction {
    /// The bookmark of the merge request
    pub bookmark: BookmarkNameOrPendingChangeId,

    /// The target branch of the merge request
    pub target_branch: String,

    /// The title of the merge request
    pub title: String,

    /// The description of the merge request
    pub description: String,

    pub dependencies: Option<Vec<String>>,
}

impl ActionInfo for CreateMRAction {
    fn id(&self) -> String {
        format!("create_mr:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Creating MRs".to_string()
    }

    fn text(&self) -> String {
        format!("Creating MR for {}", self.bookmark.magenta())
    }

    fn substep_text(&self) -> String {
        self.bookmark.magenta().to_string()
    }

    fn plan_text(&self) -> String {
        match self.description.lines().count() {
            0 => format!(
                "Create MR for {} -> {} named \"{}\" with no description",
                self.bookmark.magenta(),
                self.target_branch.magenta(),
                self.title.bold()
            ),
            lines => format!(
                "Create MR for {} -> {} named \"{}\" and a description {} lines long",
                self.bookmark.magenta(),
                self.target_branch.magenta(),
                self.title.bold(),
                lines
            ),
        }
    }

    fn dependencies(&self) -> Vec<String> {
        self.dependencies.as_ref().cloned().unwrap_or_default()
    }
}

impl ExecuteAction for CreateMRAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        let bookmark = ctx.find_bookmark_name_required(&self.bookmark)?;

        let title =
            if let BookmarkNameOrPendingChangeId::PendingChangeId(change_id) = &self.bookmark {
                // Kinda hacky but 🤷 the temp name is plenty unique
                self.title
                    .replace(&change_id_to_temp_bookmark_name(change_id), &bookmark)
            } else {
                self.title.clone()
            };

        if ctx.execute.dry_run {
            let msg = format!(
                "Would {} {} -> {} \"{}\"",
                format!("create {}", ctx.execute.forge.mr_name()).green(),
                self.bookmark.magenta(),
                self.target_branch.magenta(),
                title
            );
            ctx.execute.output.log_message(&msg);

            return Ok(ActionResultData::DryRun);
        }

        let desc = if self.description.is_empty() {
            None
        } else {
            Some(self.description.as_str())
        };

        let assignees = if ctx.execute.config.assign_to_self {
            match ctx.execute.forge.current_user().await {
                Ok(user) => Some(vec![user]),
                Err(e) => {
                    let warning =
                        format!("Warning: Failed to get current user for assignment: {}", e);
                    ctx.execute
                        .output
                        .log_message(&warning.yellow().to_string());
                    None
                }
            }
        } else {
            None
        };

        let mut reviewers = Vec::new();
        for username in &ctx.execute.config.default_reviewers {
            match ctx.execute.forge.user_by_username(username).await {
                Ok(Some(user)) => {
                    reviewers.push(user);
                }
                Ok(None) => {
                    let warning = format!("Warning: Reviewer '{}' not found", username);
                    ctx.execute
                        .output
                        .log_message(&warning.yellow().to_string());
                }
                Err(e) => {
                    let warning =
                        format!("Warning: Failed to look up reviewer '{}': {}", username, e);
                    ctx.execute
                        .output
                        .log_message(&warning.yellow().to_string());
                }
            }
        }

        match ctx
            .execute
            .forge
            .create_merge_request(
                ForgeCreateMergeRequestOptions::builder()
                    .source_branch(bookmark.clone())
                    .target_branch(self.target_branch.clone())
                    .title(title)
                    .remove_source_branch(ctx.execute.config.delete_source_branch)
                    .squash(ctx.execute.config.squash_commits)
                    .description(desc.map(|s| s.to_string()))
                    .assignees(assignees.unwrap_or_default())
                    .reviewers(reviewers)
                    .open_as_draft(ctx.execute.config.open_as_draft)
                    .build(),
            )
            .await
        {
            Ok(mr) => {
                ctx.execute.output.log_completed(&format!(
                    "Created MR {}: {}",
                    format!("!{}", mr.iid()).cyan(),
                    &mr.url().dimmed()
                ));
                Ok(ActionResultData::MRCreated(MRUpdate {
                    mr,
                    bookmark,
                    update_type: MRUpdateType::Created,
                    warnings: None,
                }))
            }
            Err(e) => {
                let error_msg = format!("Failed to create MR for {}: {}", self.bookmark, e);
                ctx.execute.output.log_message(&error_msg);
                error!("{}", error_msg);
                Err(Error::new(error_msg))
            }
        }
    }
}
