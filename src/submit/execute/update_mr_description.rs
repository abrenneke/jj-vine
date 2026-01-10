use async_trait::async_trait;
use itertools::Itertools;
use owo_colors::OwoColorize;
use tracing::error;

use crate::bookmark::BookmarkGraph;
use crate::error::{Error, Result};
use crate::submit::execute::{
    ActionResultData, ExecuteAction, ExecutionActionContext, MRUpdate, MRUpdateType,
};

pub struct UpdateMRDescriptionAction {
    pub bookmark: String,
    pub bookmark_graph: BookmarkGraph,
}

impl UpdateMRDescriptionAction {
    pub fn new(bookmark: String, bookmark_graph: BookmarkGraph) -> Self {
        Self {
            bookmark,
            bookmark_graph,
        }
    }
}

#[async_trait]
impl ExecuteAction for UpdateMRDescriptionAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            let msg = format!(
                "Would try to {} MR description for {}",
                "update".yellow(),
                self.bookmark.magenta()
            );
            ctx.output.log_current(&msg);
            ctx.output.log_message(&msg);
            Ok(ActionResultData::DryRun)
        } else {
            let containing_stacks: Vec<&crate::bookmark::BranchStack> = self
                .bookmark_graph
                .stacks
                .iter()
                .filter(|stack| stack.bookmarks.contains(&self.bookmark.to_string()))
                .sorted_by(|a, b| a.bookmarks.cmp(&b.bookmarks))
                .collect();

            let mut all_mrs = std::collections::HashMap::new();
            for stack in &containing_stacks {
                for bm in &stack.bookmarks {
                    if !all_mrs.contains_key(bm) {
                        ctx.output
                            .log_current(&format!("Checking description for {}", bm.magenta()));

                        if let Ok(Some(mr)) = ctx.gitlab.find_mr_by_source_branch(bm).await {
                            all_mrs.insert(bm.clone(), mr);
                        }
                    }
                }
            }

            if let Some(current_mr) = all_mrs.get(&self.bookmark) {
                let existing_description = current_mr.description.as_deref().unwrap_or("");

                let formatter: Box<dyn crate::description::DescriptionFormatter + Send + Sync> =
                    match ctx.config.stack_format {
                        crate::config::StackFormat::Linear => {
                            Box::new(crate::description::LinearListFormatter)
                        }
                    };
                let desc_manager = crate::description::DescriptionManager::new(formatter);

                let parsed = desc_manager.parse_description(existing_description);

                match crate::description::generate_multi_stack_description(
                    &self.bookmark,
                    &containing_stacks,
                    &all_mrs,
                    &ctx.config.stack_format,
                    &ctx.config.default_branch,
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
                            ctx.output.log_current(&format!(
                                "Updating MR {} description",
                                format!("!{}", current_mr.iid).cyan()
                            ));

                            match ctx
                                .gitlab
                                .update_mr_description(current_mr.iid, &new_description)
                                .await
                            {
                                Ok(updated_mr) => {
                                    ctx.output.log_completed(&format!(
                                        "Updated MR {} description",
                                        format!("!{}", updated_mr.iid).cyan()
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
