use bon::Builder;
use futures::{StreamExt as _, stream::FuturesUnordered};
use owo_colors::OwoColorize as _;

use crate::{
    bookmark::BookmarkRef,
    error::{Error, Result, make_whatever},
    forge::Forge as _,
    submit::{
        execute::{
            ActionInfo,
            ActionResultData,
            BookmarkNameOrPendingChangeId,
            ExecuteAction,
            ExecuteActionContext,
            MRUpdate,
            MRUpdateType,
        },
        mr_base_branch,
    },
};

/// Sync dependent merge requests for a bookmark (after all MRs created).
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct SyncDependentMergeRequestsAction {
    pub bookmark: BookmarkNameOrPendingChangeId,
    pub dependencies: Option<Vec<String>>,
}

impl ActionInfo for SyncDependentMergeRequestsAction {
    fn id(&self) -> String {
        format!("sync_dependent_merge_requests:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Syncing dependent merge requests".to_owned()
    }
    fn text(&self) -> String {
        format!(
            "Syncing dependent merge requests for {}",
            self.bookmark.magenta()
        )
    }
    fn substep_text(&self) -> String {
        self.bookmark.magenta().to_string()
    }

    fn plan_text(&self) -> String {
        format!(
            "Sync dependent merge requests for {}",
            self.bookmark.magenta()
        )
    }

    fn dependencies(&self) -> Vec<String> {
        self.dependencies.clone().unwrap_or_default()
    }
}

impl ExecuteAction for SyncDependentMergeRequestsAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        let bookmark_name = ctx.find_bookmark_name_required(&self.bookmark)?;

        if ctx.execute.dry_run {
            return Ok(ActionResultData::DryRun);
        }

        let bookmark = ctx
            .execute
            .bookmark_graph
            .find_bookmark_in_components(&bookmark_name)
            .ok_or_else::<Error, _>(|| make_whatever!("Bookmark not found: {}", self.bookmark))?;

        let default_branch = ctx.execute.jj.default_branch()?;

        let mr = ctx
            .execute
            .forge
            .find_merge_request_by_source_branch_base_branch(
                &bookmark_name,
                &mr_base_branch(ctx.execute.forge, bookmark, default_branch),
            )
            .await?
            .ok_or_else::<Error, _>(|| {
                make_whatever!("No merge request found for {}", bookmark_name)
            })?;

        let dependent_merge_request_iids: Vec<_> = bookmark
            .parents
            .iter()
            .filter_map(|p| match p {
                BookmarkRef::Bookmark(b) => Some(b),
                BookmarkRef::Trunk => None,
            })
            .map(|parent_bookmark| async move {
                let mr = ctx
                    .execute
                    .forge
                    .find_merge_request_by_source_branch_base_branch(
                        parent_bookmark.name(),
                        &mr_base_branch(ctx.execute.forge, parent_bookmark, default_branch),
                    )
                    .await?
                    .ok_or_else::<Error, _>(|| {
                        make_whatever!("No merge request found for {}", parent_bookmark.name())
                    })?;
                Ok(mr.iid().to_string())
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<Result<String>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        let (changed, warnings) = ctx
            .execute
            .forge
            .sync_dependent_merge_requests(
                mr.iid(),
                dependent_merge_request_iids
                    .iter()
                    .map(Into::into)
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await
            .into_result()?;

        Ok(ActionResultData::MRUpdated(MRUpdate {
            mr,
            bookmark: bookmark_name.clone(),
            update_type: if changed {
                MRUpdateType::new_updated()
                    .synced_dependent_merge_requests(true)
                    .call()
            } else {
                MRUpdateType::Unchanged
            },
            warnings,
        }))
    }
}
