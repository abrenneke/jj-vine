use bon::Builder;
use owo_colors::OwoColorize;
use snafu::whatever;
use tracing::error;

use crate::{
    description::{generate_stack_description, insert_stack_into_description},
    error::{Error, Result, make_whatever},
    forge::Forge,
    submit::execute::{
        ActionInfo,
        ActionResult,
        ActionResultData,
        ExecuteAction,
        ExecuteActionContext,
        MRUpdate,
        MRUpdateType,
        load_all_mrs::LoadAllMRsAction,
    },
};

/// Update MR description (after all MRs created)
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct UpdateMRTitleDescriptionAction {
    /// The new title for the MR. If None, the title will not be updated.
    pub title: Option<String>,

    pub generate_stack_in_description: bool,

    pub bookmark: String,

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
        match (&self.title, self.generate_stack_in_description) {
            (Some(title), true) => format!(
                "Update title of MR for {} to \"{}\" and regenerate stack in description",
                self.bookmark.magenta(),
                title.bold()
            ),
            (Some(title), false) => format!(
                "Update title of MR for {} to \"{}\"",
                self.bookmark.magenta(),
                title.bold()
            ),
            (None, true) => format!(
                "Regenerate stack in description of MR for {}",
                self.bookmark.magenta()
            ),
            (None, false) => format!(
                "ERROR: Neither title nor generate_stack_in_description is set for {}",
                self.id(),
            ),
        }
    }

    fn dependencies(&self) -> Vec<String> {
        self.dependencies
            .clone()
            .unwrap_or_default()
            .into_iter()
            .chain([LoadAllMRsAction.id()])
            .collect()
    }
}

impl ExecuteAction for UpdateMRTitleDescriptionAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        if ctx.execute.dry_run {
            let msg = format!(
                "Would try to {} MR description for {}",
                "update".yellow(),
                self.bookmark.magenta()
            );
            ctx.execute.output.log_message(&msg);
            return Ok(ActionResultData::DryRun);
        }

        let all_mrs = match ctx
            .current_results
            .iter()
            .find(|result| result.id == LoadAllMRsAction.id())
        {
            Some(ActionResult {
                data: Ok(ActionResultData::MRsLoaded(mrs)),
                ..
            }) => mrs,
            _ => whatever!("Failed to load MRs"),
        };

        let Some(current_mr) = all_mrs.get(self.bookmark.as_str()) else {
            return Err(Error::new(format!(
                "No MR found for {}",
                self.bookmark.magenta()
            )));
        };

        let default_branch = ctx.execute.jj.default_branch()?;

        let stack = ctx
            .execute
            .bookmark_graph
            .component_containing(self.bookmark.as_str())
            .ok_or_else::<Error, _>(|| {
                make_whatever!("Bookmark not found in component: {}", self.bookmark)
            })?;

        let stack_description = generate_stack_description(
            &self.bookmark,
            stack,
            all_mrs,
            &ctx.execute.config.description,
            default_branch,
            ctx.execute.forge,
        );

        let new_description =
            insert_stack_into_description(&stack_description, current_mr.description());

        let description_unchanged = current_mr.description() == new_description;

        if description_unchanged && self.title.is_none() {
            return Ok(ActionResultData::MRUpdated(MRUpdate {
                mr: current_mr.clone(),
                bookmark: self.bookmark.clone(),
                update_type: MRUpdateType::Unchanged,
            }));
        }

        match ctx
            .execute
            .forge
            .update_merge_request_info(
                current_mr.iid(),
                &new_description,
                self.title
                    .as_ref()
                    .map_or(current_mr.title(), |title| title),
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
                    bookmark: self.bookmark.clone(),
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
                let error_msg = format!(
                    "Failed to update MR description for {}: {}",
                    self.bookmark, e
                );
                ctx.execute.output.log_message(&error_msg);
                error!("{}", error_msg);
                Err(Error::new(error_msg))
            }
        }
    }
}
