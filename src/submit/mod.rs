use crate::{
    bookmark::BookmarkGraph,
    config::Config,
    forge::ForgeImpl,
    jj::Jujutsu,
    output::Output,
    submit::plan::SubmissionPlan,
};

pub mod execute;
pub mod plan;

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
