use async_trait::async_trait;
use owo_colors::OwoColorize;
use tracing::error;

use crate::error::{Error, Result};
use crate::submit::execute::{
    ActionResultData, ExecuteAction, ExecutionActionContext, MRUpdate, MRUpdateType,
};

pub struct CreateMRAction {
    pub bookmark: String,
    pub target_branch: String,
    pub title: String,
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

#[async_trait]
impl ExecuteAction for CreateMRAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would {} {} -> {} \"{}\"",
                "create".green(),
                self.bookmark.magenta(),
                self.target_branch.magenta(),
                self.title
            );
            ctx.output.log_message(&msg);

            Ok(ActionResultData::DryRun)
        } else {
            let desc = if self.description.is_empty() {
                None
            } else {
                Some(self.description.as_str())
            };

            let assignee_ids = if ctx.config.assign_to_self {
                match ctx.gitlab.get_current_user().await {
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

            let reviewer_ids = if !ctx.config.default_reviewers.is_empty() {
                let mut ids = Vec::new();
                for username in &ctx.config.default_reviewers {
                    match ctx.gitlab.get_user_by_username(username).await {
                        Ok(Some(user)) => ids.push(user.id),
                        Ok(None) => {
                            let warning = format!("Warning: Reviewer '{}' not found", username);
                            ctx.output.log_message(&warning.yellow().to_string());
                        }
                        Err(e) => {
                            let warning = format!(
                                "Warning: Failed to look up reviewer '{}': {}",
                                username, e
                            );
                            ctx.output.log_message(&warning.yellow().to_string());
                        }
                    }
                }
                if ids.is_empty() { None } else { Some(ids) }
            } else {
                None
            };

            match ctx
                .gitlab
                .create_merge_request(
                    &self.bookmark,
                    &self.target_branch,
                    &self.title,
                    desc,
                    ctx.config.delete_source_branch,
                    ctx.config.squash_commits,
                    assignee_ids.as_deref(),
                    reviewer_ids.as_deref(),
                )
                .await
            {
                Ok(mr) => {
                    ctx.output.log_completed(&format!(
                        "Created MR {}: {}",
                        format!("!{}", mr.iid).cyan(),
                        &mr.web_url.dimmed()
                    ));
                    Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
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
}
