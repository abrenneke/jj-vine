use std::collections::HashMap;

use futures::{StreamExt, stream::FuturesUnordered};
use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    bookmark::BookmarkGraph,
    description::{generate_stack_description, insert_stack_into_description},
    error::{Error, Result},
    forge::Forge,
    submit::execute::{
        ActionResultData,
        ExecuteAction,
        ExecutionActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

pub struct UpdateMRDescriptionAction<'a> {
    pub bookmark: String,
    pub bookmark_graph: BookmarkGraph<'a>,
}

impl<'a> UpdateMRDescriptionAction<'a> {
    pub fn new(bookmark: String, bookmark_graph: BookmarkGraph<'a>) -> Self {
        Self {
            bookmark,
            bookmark_graph,
        }
    }
}

impl<'a> ExecuteAction for UpdateMRDescriptionAction<'a> {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would try to {} MR description for {}",
                "update".yellow(),
                self.bookmark.magenta()
            );
            ctx.output.log_message(&msg);
            return Ok(ActionResultData::DryRun);
        }

        let stack = self
            .bookmark_graph
            .component_containing(&self.bookmark)
            .ok_or_else(|| Error::BookmarkNotFound {
                name: self.bookmark.clone(),
            })?;
        // TODO deterministic
        // .sorted_by(|a, b| {
        //     let a = a
        //         .leaves
        //         .iter()
        //         .map(|bm| bm.name.clone())
        //         .collect::<Vec<_>>();
        //     let b = b
        //         .leaves
        //         .iter()
        //         .map(|bm| bm.name.clone())
        //         .collect::<Vec<_>>();
        //     a.cmp(&b)
        // })

        let handles = FuturesUnordered::new();
        for bookmark in stack.all_bookmarks() {
            handles.push(async {
                Ok((
                    bookmark.name().to_string(),
                    ctx.forge
                        .find_merge_request_by_source_branch(bookmark.name())
                        .await?,
                ))
            });
        }

        let all_mrs: HashMap<_, _> = handles
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(bookmark, mr)| mr.map(|mr| (bookmark, mr)))
            .collect();

        if let Some(current_mr) = all_mrs.get(self.bookmark.as_str()) {
            let stack_description = generate_stack_description(
                &self.bookmark,
                stack,
                &all_mrs,
                &ctx.config.stack_format,
                &ctx.config.default_branch,
                ctx.forge,
            );

            let new_description =
                insert_stack_into_description(&stack_description, current_mr.description());

            if current_mr.description() == new_description {
                Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
                    mr: current_mr.clone(),
                    bookmark: self.bookmark.clone(),
                    update_type: MRUpdateType::Unchanged,
                })))
            } else {
                match ctx
                    .forge
                    .update_merge_request_description(current_mr.iid().as_ref(), &new_description)
                    .await
                {
                    Ok(updated_mr) => {
                        ctx.output.log_completed(&format!(
                            "Updated MR {} description",
                            format!("!{}", updated_mr.iid()).cyan()
                        ));
                        Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
                            mr: updated_mr,
                            bookmark: self.bookmark.clone(),
                            update_type: MRUpdateType::DescriptionUpdated,
                        })))
                    }
                    Err(e) => {
                        let error_msg = format!(
                            "Failed to update MR description for {}: {}",
                            self.bookmark, e
                        );
                        ctx.output.log_message(&error_msg);
                        error!("{}", error_msg);
                        Err(Error::new(error_msg))
                    }
                }
            }
        } else {
            Err(Error::new(format!(
                "No MR found for {}",
                self.bookmark.magenta()
            )))
        }
    }
}
