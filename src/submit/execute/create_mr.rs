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

            match ctx
                .gitlab
                .create_merge_request(&self.bookmark, &self.target_branch, &self.title, desc)
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
