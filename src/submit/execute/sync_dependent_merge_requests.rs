use bon::Builder;
use futures::{StreamExt, stream::FuturesUnordered};
use owo_colors::OwoColorize;

use crate::{
    bookmark::BookmarkRef,
    error::{Error, Result, make_whatever},
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

/// Sync dependent merge requests for a bookmark (after all MRs created)
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct SyncDependentMergeRequestsAction {
    pub bookmark: String,
    pub dependencies: Option<Vec<String>>,
}

impl ActionInfo for SyncDependentMergeRequestsAction {
    fn id(&self) -> String {
        format!("sync_dependent_merge_requests:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Syncing dependent merge requests".to_string()
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
        self.dependencies.as_ref().cloned().unwrap_or_default()
    }
}

impl ExecuteAction for SyncDependentMergeRequestsAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        if ctx.execute.dry_run {
            return Ok(ActionResultData::DryRun);
        }

        let bookmark = ctx
            .execute
            .bookmark_graph
            .find_bookmark_in_components(&self.bookmark)
            .ok_or_else::<Error, _>(|| make_whatever!("Bookmark not found: {}", self.bookmark))?;

        let default_branch = ctx.execute.jj.default_branch()?;

        let mr = ctx
            .execute
            .forge
            .find_merge_request_by_source_branch_base_branch(
                &self.bookmark,
                &bookmark.parent_name(default_branch),
            )
            .await?
            .ok_or_else::<Error, _>(|| {
                make_whatever!("No merge request found for {}", self.bookmark)
            })?;

        let dependent_merge_request_iids: Vec<_> = bookmark
            .parents
            .iter()
            .filter_map(|p| match p {
                BookmarkRef::Bookmark(b) => Some(b),
                BookmarkRef::Immutable => None,
            })
            .map(|parent_bookmark| async move {
                let mr = ctx
                    .execute
                    .forge
                    .find_merge_request_by_source_branch_base_branch(
                        parent_bookmark.name(),
                        &parent_bookmark.parent_name(default_branch),
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

        let changed = ctx
            .execute
            .forge
            .sync_dependent_merge_requests(
                mr.iid(),
                dependent_merge_request_iids
                    .iter()
                    .map(|s| s.into())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await?;

        Ok(ActionResultData::MRUpdated(MRUpdate {
            mr,
            bookmark: self.bookmark.clone(),
            update_type: if changed {
                MRUpdateType::new_updated()
                    .synced_dependent_merge_requests(true)
                    .call()
            } else {
                MRUpdateType::Unchanged
            },
        }))
    }
}
