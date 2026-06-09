#![expect(clippy::module_name_repetitions, reason = "seems fine")]

use core::hash::BuildHasher;
use std::collections::HashSet;

use itertools::Itertools as _;

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
pub fn find_changes_to_submit(
    jj: &Jujutsu,
    targets: impl IntoIterator<Item = impl JJName>,
    change_ids_pending_bookmarks: &HashSet<String, impl BuildHasher>,
) -> Result<Vec<Change>> {
    jj.log_with_pending_bookmarks(
        format!(
            "((({}) & mine() & bookmarks()) | ({})) ~ (::trunk())",
            targets
                .into_iter()
                .map(|t| format!("::{}", t.name_for_jj()))
                .join(" | "),
            if change_ids_pending_bookmarks.is_empty() {
                "none()".to_owned()
            } else {
                change_ids_pending_bookmarks.iter().join(" | ")
            }
        ),
        change_ids_pending_bookmarks,
    )
}

#[derive(Clone)]
pub struct PlanContext<'a> {
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

    pub plan: &'a SubmissionPlan,
}

impl<'a> ExecuteContext<'a> {
    #[must_use]
    pub fn new(ctx: &'a RootExecuteContext<'a>, bookmark_graph: &'a BookmarkGraph<'a>) -> Self {
        Self {
            jj: ctx.jj,
            forge: ctx.forge,
            config: ctx.config,
            output: ctx.output,
            bookmark_graph,
            dry_run: ctx.dry_run,
            plan: &ctx.plan,
        }
    }
}

pub struct RootExecuteContext<'a> {
    pub jj: &'a Jujutsu,
    pub forge: &'a ForgeImpl,
    pub config: &'a Config,
    pub output: &'a dyn Output,
    pub dry_run: bool,

    pub plan: SubmissionPlan,
    pub changes: Vec<Change>,
    pub skip_untracked_local_bookmarks: bool,
}

impl<'a> RootExecuteContext<'a> {
    #[expect(clippy::too_many_arguments, reason = "really need them all")]
    pub fn new(
        jj: &'a Jujutsu,
        forge: &'a ForgeImpl,
        config: &'a Config,
        output: &'a dyn Output,
        dry_run: bool,
        plan: SubmissionPlan,
        changes: Vec<Change>,
        skip_untracked_local_bookmarks: bool,
    ) -> Self {
        Self {
            jj,
            forge,
            config,
            output,
            dry_run,
            plan,
            changes,
            skip_untracked_local_bookmarks,
        }
    }
}
