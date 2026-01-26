use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    error::{Error, Result},
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeUser},
    submit::execute::{
        ActionResultData,
        ExecuteAction,
        ExecutionActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

pub struct CreateMRAction {
    /// The bookmark of the merge request
    pub bookmark: String,

    /// The target branch of the merge request
    pub target_branch: String,

    /// The title of the merge request
    pub title: String,

    /// The description of the merge request
    pub description: String,
}

impl CreateMRAction {
    pub fn new(
        bookmark: String,
        target_branch: String,
        title: String,
        description: String,
    ) -> Self {
        Self {
            bookmark,
            target_branch,
            title,
            description,
        }
    }
}

impl ExecuteAction for CreateMRAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would {} {} -> {} \"{}\"",
                "create".green(),
                self.bookmark.magenta(),
                self.target_branch.magenta(),
                self.title
            );
            ctx.output.log_message(&msg);

            return Ok(ActionResultData::DryRun);
        }

        let desc = if self.description.is_empty() {
            None
        } else {
            Some(self.description.as_str())
        };

        let assignee_ids = if ctx.config.assign_to_self {
            match ctx.forge.current_user().await {
                Ok(user) => Some(vec![user.id]),
                Err(e) => {
                    let warning =
                        format!("Warning: Failed to get current user for assignment: {}", e);
                    ctx.output.log_message(&warning.yellow().to_string());
                    None
                }
            }
        } else {
            None
        };

        let mut reviewer_usernames = Vec::new();
        for username in &ctx.config.default_reviewers {
            match ctx.forge.user_by_username(username).await {
                Ok(Some(ForgeUser { id, username })) => {
                    if let Some(identifier) = username.or(id) {
                        reviewer_usernames.push(identifier);
                    }
                }
                Ok(None) => {
                    let warning = format!("Warning: Reviewer '{}' not found", username);
                    ctx.output.log_message(&warning.yellow().to_string());
                }
                Err(e) => {
                    let warning =
                        format!("Warning: Failed to look up reviewer '{}': {}", username, e);
                    ctx.output.log_message(&warning.yellow().to_string());
                }
            }
        }

        match ctx
            .forge
            .create_merge_request(
                ForgeCreateMergeRequestOptions::builder()
                    .source_branch(self.bookmark.clone())
                    .target_branch(self.target_branch.clone())
                    .title(self.title.clone())
                    .remove_source_branch(ctx.config.delete_source_branch)
                    .squash(ctx.config.squash_commits)
                    .description(desc.map(|s| s.to_string()))
                    .assignee_usernames(
                        assignee_ids
                            .unwrap_or_default()
                            .into_iter()
                            .flatten()
                            .collect(),
                    )
                    .reviewer_usernames(reviewer_usernames)
                    .open_as_draft(ctx.config.open_as_draft)
                    .build(),
            )
            .await
        {
            Ok(mr) => {
                ctx.output.log_completed(&format!(
                    "Created MR {}: {}",
                    format!("!{}", mr.iid()).cyan(),
                    &mr.url(ctx.forge).dimmed()
                ));
                Ok(ActionResultData::MRCreated(Box::new(MRUpdate {
                    mr,
                    bookmark: self.bookmark.clone(),
                    update_type: MRUpdateType::Created,
                })))
            }
            Err(e) => {
                let error_msg = format!("Failed to create MR for {}: {}", self.bookmark, e);
                ctx.output.log_message(&error_msg);
                error!("{}", error_msg);
                Err(Error::new(error_msg))
            }
        }
    }
}
