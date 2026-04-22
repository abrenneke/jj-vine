use std::collections::HashMap;

use futures::{StreamExt, stream::FuturesUnordered};

use crate::{
    description::FormatMergeRequest,
    error::Result,
    forge::Forge,
    submit::execute::{ActionInfo, ActionResultData, ExecuteAction, ExecuteActionContext},
};

/// (re)load all MR data for all bookmarks in the graph
#[derive(Debug, Clone, PartialEq)]
pub struct LoadAllMRsAction;

impl ActionInfo for LoadAllMRsAction {
    fn id(&self) -> String {
        "load_all_mrs".to_string()
    }

    fn group_text(&self) -> String {
        "Loading MRs".to_string()
    }

    fn text(&self) -> String {
        "Loading MRs".to_string()
    }

    fn plan_text(&self) -> String {
        "Fetch all MRs for all bookmarks in revset".to_string()
    }

    fn substep_text(&self) -> String {
        String::new()
    }
}

impl ExecuteAction for LoadAllMRsAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        if ctx.execute.dry_run {
            ctx.execute.output.log_message(&format!(
                "Would reload all {}s",
                ctx.execute.forge.mr_name()
            ));
            return Ok(ActionResultData::MRsLoaded(HashMap::new()));
        }
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

        // let all_mrs = ctx.

        let default_branch = ctx.execute.jj.default_branch()?;

        let all_mrs: HashMap<_, _> = ctx
            .execute
            .bookmark_graph
            .components()
            .iter()
            .flat_map(|component| component.all_bookmarks())
            .map(async |bookmark| {
                Ok((
                    bookmark.name().to_string(),
                    ctx.execute
                        .forge
                        .find_merge_request_by_source_branch_base_branch(
                            bookmark.name(),
                            &bookmark.parent_name(default_branch),
                        )
                        .await?,
                ))
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(bookmark, mr)| mr.map(|mr| (bookmark, mr)))
            .collect();

        Ok(ActionResultData::MRsLoaded(all_mrs))
    }
}
