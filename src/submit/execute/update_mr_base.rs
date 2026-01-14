use async_trait::async_trait;
use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    error::{Error, Result},
    submit::execute::{
        ActionResultData,
        ExecuteAction,
        ExecutionActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

pub struct UpdateMRBaseAction {
    pub bookmark: String,
    pub mr_iid: String,
    pub new_target_branch: String,
}

impl UpdateMRBaseAction {
    pub fn new(bookmark: String, mr_iid: String, new_target_branch: String) -> Self {
        Self {
            bookmark,
            mr_iid,
            new_target_branch,
        }
    }
}

#[async_trait]
impl ExecuteAction for UpdateMRBaseAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would {} MR {} base for {} to {}",
                "update".yellow(),
                format!("!{}", self.mr_iid).cyan(),
                self.bookmark.magenta(),
                self.new_target_branch.magenta()
            );
            ctx.output.log_message(&msg);
            Ok(ActionResultData::DryRun)
        } else {
            // Get old target before update
            let old_target = if let Ok(Some(existing_mr)) = ctx
                .forge
                .find_merge_request_by_source_branch(&self.bookmark)
                .await
            {
                existing_mr.target_branch().to_string()
            } else {
                "unknown".to_string()
            };

            match ctx
                .forge
                .update_merge_request_base(&self.mr_iid, &self.new_target_branch)
                .await
            {
                Ok(mr) => {
                    ctx.output
                        .log_completed(&format!("Updated MR {}", format!("!{}", mr.iid()).cyan()));
                    Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
                        mr,
                        bookmark: self.bookmark.clone(),
                        update_type: MRUpdateType::Repointed {
                            old_target,
                            new_target: self.new_target_branch.clone(),
                        },
                    })))
                }
                Err(e) => {
                    let error_msg =
                        format!("Failed to update MR base for {}: {}", self.bookmark, e);
                    ctx.output.log_message(&error_msg);
                    error!("{}", error_msg);
                    Err(Error::new(error_msg))
                }
            }
        }
    }
}
