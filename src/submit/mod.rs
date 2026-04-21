use itertools::Itertools;

use crate::{
    bookmark::{BookmarkGraph, JJName},
    config::Config,
    error::Result,
    forge::ForgeImpl,
    jj::{Change, Jujutsu},
    output::Output,
    submit::plan::SubmissionPlan,
};

pub mod execute;
pub mod plan;

/// Find the changes that matter for a submission starting from `targets`:
/// bookmarked changes authored by the current user that are reachable from
/// the targets and are not already in the trunk ancestry.
pub fn find_changes_to_submit<N>(
    jj: &Jujutsu,
    targets: impl IntoIterator<Item = N>,
) -> Result<Vec<Change>>
where
    N: JJName,
{
    jj.log(format!(
        "(({}) & mine() & bookmarks()) ~ (::trunk())",
        targets
            .into_iter()
            .map(|t| format!("::{}", t.name_for_jj()))
            .join(" | ")
    ))
}

#[derive(Clone)]
pub struct SubmitContext<'a> {
    pub jj: &'a Jujutsu,
    pub forge: &'a ForgeImpl,
    pub config: &'a Config,
    pub output: &'a dyn Output,
    pub bookmark_graph: &'a BookmarkGraph<'a>,
    pub dry_run: bool,
}

#[derive(Clone)]
pub struct ExecuteContext<'a> {
    pub jj: &'a Jujutsu,
    pub forge: &'a ForgeImpl,
    pub config: &'a Config,
    pub output: &'a dyn Output,
    pub bookmark_graph: &'a BookmarkGraph<'a>,
    pub dry_run: bool,

    pub plan: SubmissionPlan,
}

impl<'a> SubmitContext<'a> {
    pub fn into_execute_context(self, plan: SubmissionPlan) -> ExecuteContext<'a> {
        ExecuteContext {
            jj: self.jj,
            forge: self.forge,
            config: self.config,
            output: self.output,
            bookmark_graph: self.bookmark_graph,
            dry_run: self.dry_run,
            plan,
        }
    }
}
