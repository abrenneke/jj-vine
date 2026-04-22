pub mod create_mr;
pub mod push;
pub mod push_create;
pub mod sync_dependent_merge_requests;
pub mod update_mr_base;
pub mod update_mr_title_description;

use std::collections::HashMap;

use bon::bon;
use enum_dispatch::enum_dispatch;
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::{Either, Itertools};
use snafu::whatever;
use tracing::debug;

use crate::{
    bookmark::{
        BookmarkGraph,
        BookmarkOrPending,
        BookmarkWithPointers,
        change_id_to_temp_bookmark_name,
    },
    error::{ClonableError, Error, Result},
    forge::AnyForgeMergeRequest,
    submit::{
        ExecuteContext,
        RootExecuteContext,
        execute::{
            create_mr::CreateMRAction,
            push::PushAction,
            push_create::PushCreateAction,
            sync_dependent_merge_requests::SyncDependentMergeRequestsAction,
            update_mr_base::UpdateMRBaseAction,
            update_mr_title_description::UpdateMRTitleDescriptionAction,
        },
    },
};

/// Action to perform during execution
#[derive(Debug, Clone, PartialEq)]
#[enum_dispatch(ExecuteAction, ActionInfo)]
pub enum Action {
    Push(PushAction),
    PushCreate(PushCreateAction),
    CreateMR(CreateMRAction),
    UpdateMRBase(UpdateMRBaseAction),
    UpdateMRTitleDescription(UpdateMRTitleDescriptionAction),
    SyncDependentMergeRequests(SyncDependentMergeRequestsAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkNameOrPendingChangeId {
    Bookmark(String),
    PendingChangeId(String),
}

impl BookmarkNameOrPendingChangeId {
    pub fn new_from_bookmark(bookmark: &BookmarkOrPending<'_>) -> Self {
        match bookmark {
            BookmarkOrPending::Bookmark(b) => Self::Bookmark(b.name().to_string()),
            BookmarkOrPending::Pending { change, .. } => {
                Self::PendingChangeId(change.change_id.clone())
            }
        }
    }

    pub fn new_from_pointer(pointer: &BookmarkWithPointers<'_>) -> Self {
        Self::new_from_bookmark(&pointer.bookmark)
    }
}

impl std::fmt::Display for BookmarkNameOrPendingChangeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bookmark(name) => write!(f, "{}", name),
            Self::PendingChangeId(change_id) => {
                write!(f, "{}", change_id_to_temp_bookmark_name(change_id))
            }
        }
    }
}

/// Result of executing a submission plan
#[derive(Debug)]
pub struct SubmissionResult {
    /// All MRs (created, updated, and unchanged)
    pub merge_requests: Vec<MRUpdate>,

    /// Any errors that occurred (non-fatal)
    pub errors: Vec<ClonableError>,

    /// Bookmarks that were successfully pushed
    pub bookmarks_pushed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MRUpdate {
    pub mr: AnyForgeMergeRequest,
    pub bookmark: String,
    pub update_type: MRUpdateType,
}

#[derive(Debug, Clone)]
pub enum ActionResultData {
    Pushed {
        bookmarks: Vec<String>,
        created_bookmarks: HashMap<String, String>,
        pushed: bool,
    },
    MRCreated(MRUpdate),
    MRUpdated(MRUpdate),
    DryRun,
}

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub id: String,
    pub data: Result<ActionResultData, ClonableError>,
}

pub struct ExecuteActionContext<'a> {
    pub execute: ExecuteContext<'a>,
    pub current_results: Vec<ActionResult>,
}

impl<'a> ExecuteActionContext<'a> {
    /// Gets all MRs at the current state of execution by overlaying creations
    /// and updates on top of MRs that existed at planning time.
    pub fn all_mrs(&self) -> HashMap<String, AnyForgeMergeRequest> {
        let mut all_mrs = self.execute.plan.existing_mrs.clone();

        for result in &self.current_results {
            match &result.data {
                Ok(ActionResultData::MRCreated(update))
                | Ok(ActionResultData::MRUpdated(update)) => {
                    all_mrs.insert(update.bookmark.clone(), update.mr.clone());
                }
                _ => {}
            }
        }

        all_mrs
    }
    pub fn find_bookmark_name(&self, bookmark: &BookmarkNameOrPendingChangeId) -> Option<String> {
        match bookmark {
            BookmarkNameOrPendingChangeId::Bookmark(name) => Some(name.clone()),
            BookmarkNameOrPendingChangeId::PendingChangeId(change_id) => {
                for result in &self.current_results {
                    if let Ok(ActionResultData::Pushed {
                        created_bookmarks, ..
                    }) = &result.data
                        && let Some(name) = created_bookmarks.get(change_id)
                    {
                        return Some(name.clone());
                    }
                }
                None
            }
        }
    }

    pub fn find_bookmark_name_required(
        &self,
        bookmark: &BookmarkNameOrPendingChangeId,
    ) -> Result<String> {
        match self.find_bookmark_name(bookmark) {
            Some(name) => Ok(name),
            None => whatever!("Could not find a created bookmark for change {}", bookmark),
        }
    }
}

#[enum_dispatch]
pub trait ExecuteAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData>;
}

#[enum_dispatch]
pub trait ActionInfo {
    /// Gets a unique ID for this action that other actions can potentially
    /// refer to.
    fn id(&self) -> String;

    /// The text to display for an entire group of this action type.
    fn group_text(&self) -> String;

    /// The text to display for this action.
    fn text(&self) -> String;

    /// The text to display for a substep that is this action.
    fn substep_text(&self) -> String;

    /// The text to display for this action when showing the plan.
    fn plan_text(&self) -> String;

    /// Gets any dependencies that this action has on other actions.
    fn dependencies(&self) -> Vec<String> {
        vec![]
    }
}

/// Type of MR update
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MRUpdateType {
    /// MR was unchanged
    Unchanged,

    /// MR was created
    Created,

    /// Merge requested was updated in some way
    Updated {
        old_target: Option<String>,
        new_target: Option<String>,

        old_title: Option<String>,
        new_title: Option<String>,

        old_description: Option<String>,
        new_description: Option<String>,

        synced_dependent_merge_requests: bool,
    },
}

#[bon]
impl MRUpdateType {
    #[builder]
    pub fn new_updated(
        old_target: Option<String>,
        new_target: Option<String>,
        old_title: Option<String>,
        new_title: Option<String>,
        old_description: Option<String>,
        new_description: Option<String>,
        synced_dependent_merge_requests: Option<bool>,
    ) -> Self {
        Self::Updated {
            old_target,
            new_target,
            old_title,
            new_title,
            old_description,
            new_description,
            synced_dependent_merge_requests: synced_dependent_merge_requests.unwrap_or(false),
        }
    }
}

/// Execute a submission plan
pub async fn execute(mut ctx: RootExecuteContext<'_>) -> Result<SubmissionResult> {
    let mut merge_requests = Vec::new();
    let mut errors = Vec::new();
    let mut bookmarks_pushed = Vec::new();
    let mut current_results: Vec<ActionResult> = Vec::new();

    let mut bookmark_graph = BookmarkGraph::from_bookmarks(
        ctx.jj,
        ctx.changes.iter().map(BookmarkOrPending::new_pending),
        ctx.skip_untracked_local_bookmarks,
    )?;

    ctx.output.log_current("Preparing submission");

    for batch in &ctx.plan.actions {
        ctx.output.log_current(&batch.first().unwrap().group_text());

        let handles = FuturesUnordered::new();

        for action in batch {
            let (_ok_deps, err_deps): (HashMap<_, _>, Vec<_>) = action
                .dependencies()
                .iter()
                .map(|id| {
                    current_results
                        .iter()
                        .find(|result| result.id == *id)
                        .unwrap_or_else(|| panic!("Dependency {} not found", id))
                })
                .partition_map(|dep| match &dep.data {
                    Ok(data) => Either::Left((&dep.id, data)),
                    Err(error) => Either::Right((&dep.id, error)),
                });

            if !err_deps.is_empty() {
                debug!(
                    "Skipping action {} because dependencies failed: {}",
                    action.id(),
                    err_deps.iter().map(|(id, _)| id).join(", ")
                );
                current_results.push(ActionResult {
                    id: action.id(),
                    data: Err(Error::new(format!(
                        "Dependencies failed: {}",
                        err_deps.iter().map(|(id, _)| id).join(", ")
                    ))
                    .to_clonable_error()),
                });
                continue;
            }

            let action_id = action.id();

            let action_ctx = ExecuteActionContext {
                execute: ExecuteContext::new(&ctx, &bookmark_graph),
                current_results: current_results.clone(),
            };

            handles.push(async move {
                let output = action_ctx.execute.output;

                let _substep = output.start_substep(&action.substep_text());
                let result = action.execute(action_ctx).await;

                (action_id, result)
            });
        }

        let results = handles.collect::<Vec<_>>().await;

        for (action_id, result) in results {
            current_results.push(ActionResult {
                id: action_id,
                data: result.as_ref().map_err(|e| e.to_clonable_error()).cloned(),
            });

            if let Ok(ActionResultData::Pushed {
                created_bookmarks, ..
            }) = &result
            {
                for (change_id, name) in created_bookmarks {
                    let change = ctx
                        .changes
                        .iter_mut()
                        .find(|c| c.change_id == *change_id)
                        .unwrap_or_else(|| {
                            panic!("Could not find change {} in changes", change_id)
                        });

                    change.solidify_bookmark(name);
                }

                // Rebuild the graph using the new changes
                bookmark_graph = BookmarkGraph::from_changes(
                    ctx.jj,
                    &ctx.changes,
                    ctx.skip_untracked_local_bookmarks,
                )?;
            }
        }
    }

    for result in current_results {
        match result.data {
            Ok(ActionResultData::Pushed {
                bookmarks, pushed, ..
            }) => {
                if pushed {
                    bookmarks_pushed.extend(bookmarks);
                }
            }
            Ok(ActionResultData::MRCreated(mr_update))
            | Ok(ActionResultData::MRUpdated(mr_update)) => {
                merge_requests.push(mr_update);
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
