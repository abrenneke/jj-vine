use bon::Builder;
use owo_colors::OwoColorize;
use snafu::whatever;
use tracing::error;

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::Forge,
    submit::execute::{
        ActionInfo,
        ActionResultData,
        ExecuteAction,
        ExecuteActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

/// Update the target branch (base) of an existing MR
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct UpdateMRBaseAction {
    pub bookmark: String,
    pub mr_iid: String,
    pub new_target_branch: String,
    pub dependencies: Option<Vec<String>>,
}

impl ActionInfo for UpdateMRBaseAction {
    fn id(&self) -> String {
        format!("update_mr_base:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Updating MR bases".to_string()
    }

    fn text(&self) -> String {
        format!(
            "Updating MR {} base for {} to {}",
            self.mr_iid.cyan(),
            self.bookmark.magenta(),
            self.new_target_branch.magenta()
        )
    }

    fn substep_text(&self) -> String {
        self.bookmark.magenta().to_string()
    }

    fn plan_text(&self) -> String {
        format!(
            "Update base of MR {} for {} from existing target to {}",
            self.mr_iid.cyan(),
            self.bookmark.magenta(),
            self.new_target_branch.magenta()
        )
    }

    fn dependencies(&self) -> Vec<String> {
        self.dependencies.as_ref().cloned().unwrap_or_default()
    }
}

impl ExecuteAction for UpdateMRBaseAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        if ctx.execute.dry_run {
            let msg = format!(
                "Would {} {} {} base for {} to {}",
                "update".yellow(),
                ctx.execute.forge.mr_name(),
                format!("!{}", self.mr_iid).cyan(),
                self.bookmark.magenta(),
                self.new_target_branch.magenta()
            );
            ctx.execute.output.log_message(&msg);
            return Ok(ActionResultData::DryRun);
        }

        let Some(bookmark) = ctx
            .execute
            .bookmark_graph
            .find_bookmark_in_components(&self.bookmark)
        else {
            whatever!("Bookmark not found: {}", self.bookmark);
        };

        let default_branch = ctx.execute.jj.default_branch()?;

        // Get old target before update
        let Ok(Some(existing_mr)) = ctx
            .execute
            .forge
            .find_merge_request_by_source_branch_base_branch(
                bookmark.name(),
                &bookmark.parent_name(default_branch),
            )
            .await
        else {
            whatever!("Failed to find existing MR");
        };

        let old_target = existing_mr.target_branch().to_string();

        match ctx
            .execute
            .forge
            .update_merge_request_base((&self.mr_iid).into(), &self.new_target_branch)
            .await
        {
            Ok(mr) => {
                ctx.execute
                    .output
                    .log_completed(&format!("Updated MR {}", format!("!{}", mr.iid()).cyan()));
                Ok(ActionResultData::MRUpdated(MRUpdate {
                    mr,
                    bookmark: self.bookmark.clone(),
                    update_type: MRUpdateType::new_updated()
                        .old_target(old_target)
                        .new_target(self.new_target_branch.clone())
                        .call(),
                    warnings: None,
                }))
            }
            Err(e) => {
                let error_msg = format!("Failed to update MR base for {}: {}", self.bookmark, e);
                ctx.execute.output.log_message(&error_msg);
                error!("{}", error_msg);
                Err(Error::new(error_msg))
            }
        }
    }
}
