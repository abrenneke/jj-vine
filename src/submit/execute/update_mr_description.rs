use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    bookmark::{BookmarkGraph, BranchStack},
    config::StackFormat,
    description::{
        DescriptionFormatter,
        DescriptionManager,
        LinearListFormatter,
        generate_multi_stack_description,
    },
    error::{Error, Result},
    forge::ForgeMergeRequest,
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

#[async_trait]
impl<'a> ExecuteAction for UpdateMRDescriptionAction<'a> {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would try to {} MR description for {}",
                "update".yellow(),
                self.bookmark.magenta()
            );
            ctx.output.log_message(&msg);
            Ok(ActionResultData::DryRun)
        } else {
            let containing_stacks: Vec<&BranchStack> = self
                .bookmark_graph
                .stacks
                .iter()
                .filter(|stack| stack.bookmarks.iter().any(|bm| bm.name == self.bookmark))
                .sorted_by(|a, b| {
                    let a = a
                        .bookmarks
                        .iter()
                        .map(|bm| bm.name.clone())
                        .collect::<Vec<_>>();
                    let b = b
                        .bookmarks
                        .iter()
                        .map(|bm| bm.name.clone())
                        .collect::<Vec<_>>();
                    a.cmp(&b)
                })
                .collect();

            let mut to_check = HashSet::new();
            for stack in &containing_stacks {
                for bm in &stack.bookmarks {
                    to_check.insert(bm.name.clone());
                }
            }

            let handles = FuturesUnordered::new();
            for bm in to_check {
                handles.push(async move {
                    if let Ok(Some(mr)) = ctx.forge.find_merge_request_by_source_branch(&bm).await {
                        (bm, Some(mr))
                    } else {
                        (bm, None)
                    }
                });
            }
            let all_mrs: HashMap<String, ForgeMergeRequest> = handles
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .filter_map(|(bm, mr)| mr.map(|mr| (bm, mr)))
                .collect();

            if let Some(current_mr) = all_mrs.get(&self.bookmark) {
                let existing_description = current_mr.description().to_string();

                let formatter: Box<dyn DescriptionFormatter + Send + Sync> =
                    match ctx.config.stack_format {
                        StackFormat::Linear => Box::new(LinearListFormatter),
                    };
                let desc_manager = DescriptionManager::new(formatter);

                let parsed = desc_manager.parse_description(&existing_description);

                match generate_multi_stack_description(
                    &self.bookmark,
                    &containing_stacks,
                    &all_mrs,
                    &ctx.config.stack_format,
                    &ctx.config.default_branch,
                    ctx.forge,
                ) {
                    Ok(stack_content) => {
                        let new_description = desc_manager.build_description(
                            parsed.content_before.as_deref(),
                            parsed.content_after.as_deref(),
                            &stack_content,
                        );

                        if existing_description == new_description {
                            Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
                                mr: current_mr.clone(),
                                bookmark: self.bookmark.clone(),
                                update_type: MRUpdateType::Unchanged,
                            })))
                        } else {
                            match ctx
                                .forge
                                .update_merge_request_description(
                                    current_mr.iid().as_ref(),
                                    &new_description,
                                )
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
                    }
                    Err(e) => {
                        let error_msg = format!(
                            "Failed to generate description for {}: {}",
                            self.bookmark, e
                        );
                        ctx.output.log_message(&error_msg);
                        error!("{}", error_msg);
                        Err(Error::new(error_msg))
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
}
