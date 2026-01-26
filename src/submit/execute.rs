mod create_mr;
mod push;
mod sync_dependent_merge_requests;
mod update_mr_base;
mod update_mr_description;

use std::collections::HashMap;

use enum_dispatch::enum_dispatch;
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::{Either, Itertools};
use tracing::debug;

use crate::{
    config::Config,
    error::{Error, Result},
    forge::{ForgeImpl, ForgeMergeRequest},
    jj::Jujutsu,
    output::Output,
    submit::{
        execute::{
            create_mr::CreateMRAction,
            push::PushAction,
            sync_dependent_merge_requests::SyncDependentMergeRequestsAction,
            update_mr_base::UpdateMRBaseAction,
            update_mr_description::UpdateMRDescriptionAction,
        },
        plan::{Action, SubmissionPlan},
    },
};

/// Result of executing a submission plan
#[derive(Debug)]
pub struct SubmissionResult {
    /// All MRs (created, updated, and unchanged)
    pub merge_requests: Vec<MRUpdate>,

    /// Any errors that occurred (non-fatal)
    pub errors: Vec<Error>,

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
    MRCreated(Box<MRUpdate>),
    MRUpdated(Box<MRUpdate>),
    DryRun,
}

#[derive(Debug)]
pub struct ActionResult {
    pub id: usize,
    pub data: Result<ActionResultData>,
}

#[enum_dispatch]
pub trait ExecuteAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData>;
}

#[enum_dispatch(ExecuteAction)]
pub enum ExecuteActionImpl<'a> {
    Push(PushAction),
    CreateMR(CreateMRAction),
    UpdateMRBase(UpdateMRBaseAction),
    UpdateMRDescription(UpdateMRDescriptionAction<'a>),
    SyncDependentMergeRequests(SyncDependentMergeRequestsAction<'a>),
}

pub struct ExecutionActionContext<'a, 'b> {
    pub plan: &'a SubmissionPlan<'b>,
    pub jj: &'a Jujutsu,
    pub forge: &'a ForgeImpl,
    pub config: &'a Config,
    pub output: &'a dyn Output,
}

/// Type of MR update
#[derive(Debug, Clone, PartialEq, Eq)]
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
    SyncedDependentMergeRequests,
}

/// Execute a submission plan
pub async fn execute(
    plan: &SubmissionPlan<'_>,
    jj: &Jujutsu,
    forge: &ForgeImpl,
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
            let (_ok_deps, err_deps): (HashMap<_, _>, Vec<_>) = action
                .dependencies
                .iter()
                .map(|id| {
                    current_results
                        .iter()
                        .find(|result| result.id == *id)
                        .unwrap_or_else(|| panic!("Dependency {} not found", id))
                })
                .partition_map(|dep| match &dep.data {
                    Ok(data) => Either::Left((dep.id, data)),
                    Err(error) => Either::Right((dep.id, error)),
                });

            if !err_deps.is_empty() {
                debug!(
                    "Skipping action {} because dependencies failed: {}",
                    action.id,
                    err_deps.iter().map(|(id, _)| id).join(", ")
                );
                current_results.push(ActionResult {
                    id: action.id,
                    data: Err(Error::new(format!(
                        "Dependencies failed: {}",
                        err_deps.iter().map(|(id, _)| id).join(", ")
                    ))),
                });
                continue;
            }

            let execute_action: ExecuteActionImpl<'_> = match &action.action {
                Action::Push { bookmark, remote } => {
                    PushAction::new(bookmark.clone(), remote.clone()).into()
                }
                Action::CreateMR {
                    bookmark,
                    target_branch,
                    title,
                    description,
                } => CreateMRAction::new(
                    bookmark.clone(),
                    target_branch.clone(),
                    title.clone(),
                    description.clone(),
                )
                .into(),
                Action::UpdateMRBase {
                    bookmark,
                    mr_iid,
                    new_target_branch,
                } => UpdateMRBaseAction::new(
                    bookmark.clone(),
                    mr_iid.clone(),
                    new_target_branch.clone(),
                )
                .into(),
                Action::UpdateMRDescription {
                    bookmark,
                    bookmark_graph,
                } => {
                    UpdateMRDescriptionAction::new(bookmark.clone(), bookmark_graph.clone()).into()
                }
                Action::SyncDependentMergeRequests {
                    bookmark,
                    bookmark_graph,
                } => {
                    SyncDependentMergeRequestsAction::new(bookmark.clone(), bookmark_graph.clone())
                        .into()
                }
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
            Ok(ActionResultData::MRCreated(mr_update))
            | Ok(ActionResultData::MRUpdated(mr_update)) => {
                merge_requests.push(*mr_update);
            }
            Ok(ActionResultData::DryRun) => {}
            Err(error) => {
                errors.push(error);
            }
        }
    }

    Ok(SubmissionResult {
        merge_requests,
        errors,
        bookmarks_pushed,
    })
}
