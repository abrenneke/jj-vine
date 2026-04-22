use bon::Builder;
use owo_colors::OwoColorize;
use snafu::whatever;
use tracing::error;

use crate::{
    description::{FormatMergeRequest, generate_stack_description, insert_stack_into_description},
    error::{Error, Result},
    forge::{Forge, ForgeUpdateMergeRequestInfoOptions},
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

/// Update MR description (after all MRs created)
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct UpdateMRTitleDescriptionAction {
    /// The new title for the MR. If None, the title will not be updated.
    pub title: Option<String>,

    /// The new user-content description for the MR. If None, the description
    /// will not be updated. This does not affect whether the stack is shown
    /// or regenerated, it is only for syncing the description automatically.
    pub description: Option<String>,

    pub generate_stack_in_description: bool,

    pub bookmark: BookmarkNameOrPendingChangeId,

    pub dependencies: Option<Vec<String>>,
}

impl ActionInfo for UpdateMRTitleDescriptionAction {
    fn id(&self) -> String {
        format!("update_mr_title_description:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Updating MR descriptions".to_string()
    }

    fn text(&self) -> String {
        format!("Updating MR {} description", self.bookmark.magenta())
    }

    fn substep_text(&self) -> String {
        self.bookmark.magenta().to_string()
    }

    fn plan_text(&self) -> String {
        match (
            &self.title,
            self.generate_stack_in_description,
            &self.description,
        ) {
            (Some(title), true, Some(description)) => format!(
                "Update title of MR for {} to \"{}\" and update description ({} lines) & stack",
                self.bookmark.magenta(),
                title.bold(),
                description.lines().count()
            ),
            (Some(title), true, None) => format!(
                "Update title of MR for {} to \"{}\" and regenerate stack in description",
                self.bookmark.magenta(),
                title.bold()
            ),
            (Some(title), false, None) => format!(
                "Update title of MR for {} to \"{}\"",
                self.bookmark.magenta(),
                title.bold()
            ),
            (Some(title), false, Some(description)) => format!(
                "Update title of MR for {} to \"{}\" and update description ({} lines)",
                self.bookmark.magenta(),
                title.bold(),
                description.lines().count()
            ),
            (None, true, None) => format!(
                "Regenerate stack in description of MR for {}",
                self.bookmark.magenta()
            ),
            (None, true, Some(description)) => format!(
                "Update description ({} lines) & stack of MR for {}",
                description.lines().count(),
                self.bookmark.magenta()
            ),
            (None, false, Some(description)) => {
                format!(
                    "Update description of MR for {} ({} lines)",
                    self.bookmark.magenta(),
                    description.lines().count()
                )
            }
            (None, false, None) => format!(
                "ERROR: Neither title nor generate_stack_in_description nor description is set for {}",
                self.id(),
            ),
        }
    }

    fn dependencies(&self) -> Vec<String> {
        self.dependencies.clone().unwrap_or_default()
    }
}

impl ExecuteAction for UpdateMRTitleDescriptionAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        let bookmark = ctx.find_bookmark_name_required(&self.bookmark)?;

        if ctx.execute.dry_run {
            ctx.execute.output.log_message(&format!(
                "Would try to {} {} description for {}",
                "update".yellow(),
                ctx.execute.forge.mr_name(),
                bookmark.magenta()
            ));
            return Ok(ActionResultData::DryRun);
        }

        let all_mrs = ctx.all_mrs();

        let Some(current_mr) = all_mrs.get(bookmark.as_str()) else {
            whatever!("No MR found for {}", bookmark.magenta());
        };

        let default_branch = ctx.execute.jj.default_branch()?;

        let Some(stack) = ctx.execute.bookmark_graph.component_containing(&bookmark) else {
            whatever!("Bookmark not found in component: {}", self.bookmark)
        };

        let stack_description = generate_stack_description(
            &bookmark,
            stack,
            &all_mrs,
            &ctx.execute.config.description,
            default_branch,
            ctx.execute.forge,
        );

        let description_user_part = if let Some(description) = &self.description {
            description // Stack part will be inserted after
        } else {
            current_mr.description()
        };

        let new_description =
            insert_stack_into_description(&stack_description, description_user_part);

        let description_unchanged = current_mr.description() == new_description;

        if description_unchanged && self.title.is_none() {
            return Ok(ActionResultData::MRUpdated(MRUpdate {
                mr: current_mr.clone(),
                bookmark: bookmark.clone(),
                update_type: MRUpdateType::Unchanged,
            }));
        }

        match ctx
            .execute
            .forge
            .update_merge_request_info(
                current_mr.iid(),
                ForgeUpdateMergeRequestInfoOptions::builder()
                    .description(new_description.to_string())
                    .maybe_title(self.title.clone())
                    .current_is_draft(current_mr.is_draft())
                    .current_title(current_mr.title().to_string())
                    .build(),
            )
            .await
        {
            Ok(updated_mr) => {
                ctx.execute.output.log_completed(&format!(
                    "Updated MR {} description",
                    format!("!{}", updated_mr.iid()).cyan()
                ));

                Ok(ActionResultData::MRUpdated(MRUpdate {
                    mr: updated_mr,
                    bookmark: bookmark.clone(),
                    update_type: MRUpdateType::new_updated()
                        .old_description(current_mr.description().to_string())
                        .maybe_new_description(
                            description_unchanged.then(|| new_description.to_string()),
                        )
                        .old_title(current_mr.title().to_string())
                        .maybe_new_title(self.title.as_ref().cloned())
                        .call(),
                }))
            }
            Err(e) => {
                let error_msg = format!("Failed to update MR description for {}: {}", bookmark, e);
                ctx.execute.output.log_message(&error_msg);
                error!("{}", error_msg);
                Err(Error::new(error_msg))
            }
        }
    }
}
