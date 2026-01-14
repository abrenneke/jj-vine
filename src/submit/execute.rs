mod create_mr;
mod push;
mod update_mr_base;
mod update_mr_description;

use async_trait::async_trait;
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use tracing::debug;

use crate::{
    config::Config,
    error::{Error, Result},
    forge::{Forge, ForgeMergeRequest},
    jj::Jujutsu,
    output::Output,
    submit::{
        execute::{
            create_mr::CreateMRAction,
            push::PushAction,
            update_mr_base::UpdateMRBaseAction,
            update_mr_description::UpdateMRDescriptionAction,
        },
        plan::{Action, SubmissionPlan},
    },
};

/// Result of executing a submission plan
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// All MRs (created, updated, and unchanged)
    pub merge_requests: Vec<MRUpdate>,

    /// Any errors that occurred (non-fatal)
    pub errors: Vec<String>,

    /// Bookmarks that were successfully pushed
    pub bookmarks_pushed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MRUpdate {
    pub mr: ForgeMergeRequest,
    pub bookmark: String,
    pub update_type: MRUpdateType,
}

#[derive(Debug, Clone)]
pub enum ActionResultData {
    Pushed { bookmark: String, pushed: bool },
    MRUpdated(Box<MRUpdate>),
    DryRun,
}

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub id: usize,
    pub data: Result<ActionResultData>,
}

#[async_trait]
pub trait ExecuteAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData>;
}

pub struct ExecutionActionContext<'a, 'b> {
    pub plan: &'a SubmissionPlan<'b>,
    pub jj: &'a Jujutsu,
    pub forge: &'a dyn Forge,
    pub config: &'a Config,
    pub output: &'a dyn Output,
}

/// Type of MR update
#[derive(Debug, Clone)]
pub enum MRUpdateType {
    /// MR was unchanged
    Unchanged,

    /// MR was created
    Created,

    /// Target branch was changed (repointed)
    Repointed {
        old_target: String,
        new_target: String,
    },

    /// Description was updated
    DescriptionUpdated,

    /// Both target and description updated
    Both {
        old_target: String,
        new_target: String,
    },
}

/// Execute a submission plan
///
/// Phase 3 of the three-phase submission process:
/// - Push bookmarks to remote
/// - Create or update merge requests
pub async fn execute(
    plan: &SubmissionPlan<'_>,
    jj: &Jujutsu,
    forge: &dyn Forge,
    config: &Config,
    output: &dyn Output,
) -> Result<SubmissionResult> {
    let mut merge_requests = Vec::new();
    let mut errors = Vec::new();
    let mut bookmarks_pushed = Vec::new();
    let mut current_results: Vec<ActionResult> = Vec::new();

    if plan.dry_run {
        output.log_message("DRY RUN - No changes will be made");
    }

    output.log_current("Preparing submission");

    for batch in &plan.actions {
        output.log_current(&batch.first().unwrap().action.get_group_text());

        let handles = FuturesUnordered::new();

        for action in batch {
            let failed_deps = action
                .dependencies
                .iter()
                .map(|id| {
                    current_results
                        .iter()
                        .find(|result| result.id == *id)
                        .unwrap()
                        .clone()
                })
                .filter(|result| result.data.is_err())
                .collect::<Vec<_>>();

            if !failed_deps.is_empty() {
                debug!(
                    "Skipping action {} because dependencies failed: {}",
                    action.id,
                    failed_deps.iter().map(|result| result.id).join(", ")
                );
                current_results.push(ActionResult {
                    id: action.id,
                    data: Err(Error::new(format!(
                        "Dependencies failed: {}",
                        failed_deps.iter().map(|result| result.id).join(", ")
                    ))),
                });
                continue;
            }

            let execute_action: Box<dyn ExecuteAction> = match &action.action {
                Action::Push { bookmark, remote } => {
                    Box::new(PushAction::new(bookmark.clone(), remote.clone()))
                }
                Action::CreateMR {
                    bookmark,
                    target_branch,
                    title,
                    description,
                } => Box::new(CreateMRAction::new(
                    bookmark.clone(),
                    target_branch.clone(),
                    title.clone(),
                    description.clone(),
                )),
                Action::UpdateMRBase {
                    bookmark,
                    mr_iid,
                    new_target_branch,
                } => Box::new(UpdateMRBaseAction::new(
                    bookmark.clone(),
                    mr_iid.clone(),
                    new_target_branch.clone(),
                )),
                Action::UpdateMRDescription {
                    bookmark,
                    bookmarks_being_submitted: _,
                    bookmark_graph,
                } => Box::new(UpdateMRDescriptionAction::new(
                    bookmark.clone(),
                    bookmark_graph.clone(),
                )),
            };

            let action_id = action.id;
            let ctx = ExecutionActionContext {
                plan,
                jj,
                forge,
                config,
                output,
            };

            handles.push(async move {
                let action_text = action.action.get_substep_text();
                let output = ctx.output;

                let _substep = output.start_substep(action_text);
                let result = execute_action.execute(ctx).await;

                (action_id, result)
            });
        }

        let results = handles.collect::<Vec<_>>().await;

        for (action_id, result) in results {
            current_results.push(ActionResult {
                id: action_id,
                data: result,
            });
        }
    }

    for result in current_results {
        match result.data {
            Ok(ActionResultData::Pushed { bookmark, pushed }) => {
                if pushed {
                    bookmarks_pushed.push(bookmark);
                }
            }
            Ok(ActionResultData::MRUpdated(mr_update)) => {
                merge_requests.push(*mr_update);
            }
            Ok(ActionResultData::DryRun) => {}
            Err(error) => {
                errors.push(error.to_string());
            }
        }
    }

    Ok(SubmissionResult {
        merge_requests,
        errors,
        bookmarks_pushed,
    })
}
