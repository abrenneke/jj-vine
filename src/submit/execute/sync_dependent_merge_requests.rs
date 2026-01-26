use futures::{StreamExt, stream::FuturesUnordered};

use crate::{
    bookmark::{BookmarkGraph, BookmarkRef},
    error::{Error, Result, make_whatever},
    forge::Forge,
    submit::execute::{
        ActionResultData,
        ExecuteAction,
        ExecutionActionContext,
        MRUpdate,
        MRUpdateType,
    },
};

pub struct SyncDependentMergeRequestsAction<'a> {
    pub bookmark: String,
    pub bookmark_graph: BookmarkGraph<'a>,
}

impl<'a> SyncDependentMergeRequestsAction<'a> {
    pub fn new(bookmark: String, bookmark_graph: BookmarkGraph<'a>) -> Self {
        Self {
            bookmark,
            bookmark_graph,
        }
    }
}

impl<'a> ExecuteAction for SyncDependentMergeRequestsAction<'a> {
    async fn execute(&self, ctx: ExecutionActionContext<'_, '_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            return Ok(ActionResultData::DryRun);
        }

        let mr = ctx
            .forge
            .find_merge_request_by_source_branch(&self.bookmark)
            .await?
            .ok_or_else::<Error, _>(|| {
                make_whatever!("No merge request found for {}", self.bookmark)
            })?;

        let dependent_merge_request_iids: Vec<_> = self
            .bookmark_graph
            .find_bookmark_in_components(&self.bookmark)
            .expect("Bookmark not found in graph")
            .parents
            .iter()
            .filter_map(|p| match p {
                BookmarkRef::Bookmark(b) => Some(b.name()),
                BookmarkRef::Trunk => None,
            })
            .map(|parent_bookmark| async move {
                let mr = ctx
                    .forge
                    .find_merge_request_by_source_branch(parent_bookmark)
                    .await?
                    .ok_or_else::<Error, _>(|| {
                        make_whatever!("No merge request found for {}", parent_bookmark)
                    })?;
                Ok(mr.iid().to_string())
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<Result<String>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        let changed = ctx
            .forge
            .sync_dependent_merge_requests(
                &mr.iid(),
                dependent_merge_request_iids
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await?;

        Ok(ActionResultData::MRUpdated(Box::new(MRUpdate {
            mr,
            bookmark: self.bookmark.clone(),
            update_type: if changed {
                MRUpdateType::SyncedDependentMergeRequests
            } else {
                MRUpdateType::Unchanged
            },
        })))
    }
}
