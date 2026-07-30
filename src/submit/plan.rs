use std::{collections::HashMap, path::Path};

use futures::{StreamExt as _, stream::FuturesUnordered};
use owo_colors::OwoColorize as _;
use snafu::whatever;

use crate::{
    bookmark::{BookmarkOrPending, BookmarkRef},
    description::{generate_description, remove_jj_vine_stack_from_description},
    error::{Error, Result},
    forge::{AnyForgeMergeRequest, Forge as _},
    output::OutputExt as _,
    submit::{
        PlanContext,
        execute::{
            Action,
            ActionInfo as _,
            BookmarkNameOrPendingChangeId,
            create_mr::CreateMRAction,
            push::PushAction,
            push_create::PushCreateAction,
            sync_dependent_merge_requests::SyncDependentMergeRequestsAction,
            update_mr_base::UpdateMRBaseAction,
            update_mr_title_description::UpdateMRTitleDescriptionAction,
        },
        mr_base_branch,
    },
    title::get_mr_title,
};

/// Plan for submission execution.
#[derive(Debug, Clone)]
#[expect(clippy::module_name_repetitions, reason = "it's fine")]
pub struct SubmissionPlan {
    /// Actions organized into batches. Each batch's actions execute in
    /// parallel, and batches execute sequentially.
    pub actions: Vec<Vec<Action>>,

    /// MRs that already existed on the forge at planning time.
    pub existing_mrs: HashMap<String, AnyForgeMergeRequest>,
}

impl core::fmt::Display for SubmissionPlan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, batch) in self.actions.iter().enumerate() {
            writeln!(f, "Batch {}:", i.strict_add(1))?;
            for action in batch {
                writeln!(f, "  {}", action.plan_text())?;
            }
        }
        Ok(())
    }
}

#[expect(dead_code, reason = "will be used")]
struct PlanMergeRequest {
    bookmark: String,
    target_branch: String,
    title: String,
    description: String,
}

impl PlanMergeRequest {
    #[expect(clippy::single_call_fn, reason = "seems fine")]
    fn from_forge_mr(mr: &AnyForgeMergeRequest) -> Self {
        Self {
            bookmark: mr.source_branch().to_owned(),
            target_branch: mr.target_branch().to_owned(),
            title: mr.title().to_owned(),
            description: mr.description().to_owned(),
        }
    }
}

/// Create a submission plan.
///
/// # Panics
///
/// Panics if many reasons.
#[expect(clippy::too_many_lines, reason = "important")]
pub async fn plan(ctx: PlanContext<'_>) -> Result<SubmissionPlan> {
    ctx.output.log_current("Planning submission");

    let default_branch = ctx.jj.default_branch()?;

    let mut batches: Vec<Vec<Action>> = Vec::new();

    let existing_mrs: HashMap<_, _> = ctx
        .output
        .run_async("Loading existing merge requests", || async {
            Ok::<_, Error>(
                ctx.bookmark_graph
                    .components()
                    .iter()
                    .flat_map(|component| component.all_bookmarks())
                    .filter(|bookmark| !bookmark.is_pending())
                    .map(async |bookmark| {
                        let _substep = ctx
                            .output
                            .start_substep(&bookmark.name().magenta().to_string());

                        if let Some(mr) = ctx
                            .forge
                            .find_merge_request_by_source_branch_base_branch(
                                bookmark.name(),
                                &mr_base_branch(ctx.forge, bookmark, default_branch),
                            )
                            .await?
                        {
                            Ok(Some((bookmark.name().to_owned(), mr)))
                        } else {
                            Ok(None)
                        }
                    })
                    .collect::<FuturesUnordered<_>>()
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect(),
            )
        })
        .await?;

    let mut push_action_ids = HashMap::new();

    let mut push_batch = Vec::new();

    let (to_push_create, to_push_update): (Vec<_>, Vec<_>) = ctx
        .bookmark_graph
        .bookmarks()
        .partition(BookmarkOrPending::is_pending);

    if !to_push_create.is_empty() {
        let push_action = PushCreateAction::builder()
            .change_ids(
                to_push_create
                    .iter()
                    .map(|b| b.change_id().to_owned())
                    .collect(),
            )
            .remote(ctx.config.remote_name.clone())
            .build();

        push_action_ids.extend(to_push_create.iter().map(|b| (b.name(), push_action.id())));
        push_batch.push(Action::PushCreate(push_action));
    }

    if !to_push_update.is_empty() {
        let push_action = PushAction::builder()
            .bookmarks(to_push_update.iter().map(|b| b.name().to_owned()).collect())
            .remote(ctx.config.remote_name.clone())
            .build();

        push_action_ids.extend(to_push_update.iter().map(|b| (b.name(), push_action.id())));
        push_batch.push(Action::Push(push_action));
    }

    if !push_batch.is_empty() {
        batches.push(push_batch);
    }

    let mut mr_action_ids = Vec::new();
    let mut plan_mrs: HashMap<_, _> = existing_mrs
        .values()
        .map(|mr| {
            (
                mr.source_branch().to_owned(),
                PlanMergeRequest::from_forge_mr(mr),
            )
        })
        .collect();

    for component in ctx.bookmark_graph.components() {
        for bookmark in component.topological_sort()? {
            let bookmark = ctx
                .bookmark_graph
                .find_bookmark_in_components(&bookmark)
                .unwrap();

            let mut dependencies = Vec::new();

            // MR updates depend on the source & target bookmarks being pushed
            dependencies.extend(
                push_action_ids
                    .get(bookmark.name())
                    .map(|id| vec![id.clone()])
                    .unwrap_or_default(),
            );

            // MR updates depend on the parent MRs being created
            dependencies.extend(
                bookmark
                    .parents
                    .iter()
                    .filter_map(|p| match p {
                        BookmarkRef::Bookmark(b) => Some(batches.iter().flat_map(|batch| {
                            batch.iter().filter_map(|action| match action {
                                Action::CreateMR(create_mr_action)
                                    if create_mr_action.bookmark
                                        == BookmarkNameOrPendingChangeId::new_from_pointer(b) =>
                                {
                                    Some(create_mr_action.id())
                                }
                                _ => None,
                            })
                        })),
                        BookmarkRef::Trunk => None,
                    })
                    .flatten(),
            );

            let target_branch = mr_base_branch(ctx.forge, bookmark, default_branch);

            if let Some(existing_mr) = existing_mrs.get(bookmark.name()) {
                if existing_mr.target_branch() != target_branch {
                    let update_mr_base_action = UpdateMRBaseAction::builder()
                        .bookmark(bookmark.name().to_owned())
                        .mr_iid(existing_mr.iid().to_string())
                        .new_target_branch(target_branch.clone())
                        .dependencies(dependencies)
                        .build();

                    mr_action_ids.push(update_mr_base_action.id());

                    batches.push(vec![Action::UpdateMRBase(update_mr_base_action)]);
                }
            } else {
                let revisions = bookmark.revisions(ctx.jj)?;
                let title =
                    get_mr_title(ctx.jj, &ctx.config.title, bookmark, component, revisions)?;

                let description = generate_description(
                    &ctx.config.description,
                    &bookmark.revisions(ctx.jj)?,
                    Path::new(&ctx.jj.exec(["root"])?.stdout),
                );

                let create_mr_action = CreateMRAction::builder()
                    .bookmark(BookmarkNameOrPendingChangeId::new_from_pointer(bookmark))
                    .target_branch(target_branch.clone())
                    .title(title.clone())
                    .description(description.clone())
                    .dependencies(dependencies)
                    .build();

                mr_action_ids.push(create_mr_action.id());

                batches.push(vec![Action::CreateMR(create_mr_action)]);

                plan_mrs.insert(
                    bookmark.name().to_owned(),
                    PlanMergeRequest {
                        bookmark: bookmark.name().to_owned(),
                        target_branch: target_branch.clone(),
                        title,
                        description,
                    },
                );
            }
        }
    }

    let title_description_batch = (ctx.config.description.enabled
        || ctx.config.title.sync_single_revision
        || ctx.config.title.sync_multiple_revisions
        || ctx.config.description.sync)
        .then(|| {
            let actions = ctx
                .bookmark_graph
                .components()
                .iter()
                .flat_map(|component| {
                    component.all_bookmarks().into_iter().map(|bookmark| {
                        let Some((current_title, current_description)) = plan_mrs
                            .get(bookmark.name())
                            .map(|mr| (&mr.title, &mr.description))
                        else {
                            whatever!("No MR found for {}", bookmark.name());
                        };

                        let revisions = bookmark.revisions(ctx.jj)?;

                        let should_sync_title = match revisions.len() {
                            1 => ctx.config.title.sync_single_revision,
                            _ => ctx.config.title.sync_multiple_revisions,
                        };

                        let maybe_title = if should_sync_title {
                            let new_title = get_mr_title(
                                ctx.jj,
                                &ctx.config.title,
                                bookmark,
                                component,
                                revisions,
                            )?;

                            if new_title == *current_title {
                                None
                            } else {
                                Some(new_title)
                            }
                        } else {
                            None
                        };

                        let maybe_description =
                            if ctx.config.description.enabled && ctx.config.description.sync {
                                let new_description = generate_description(
                                    &ctx.config.description,
                                    &bookmark.revisions(ctx.jj)?,
                                    Path::new(&ctx.jj.exec(["root"])?.stdout),
                                );

                                if new_description.trim()
                                    == remove_jj_vine_stack_from_description(current_description)
                                {
                                    None
                                } else {
                                    Some(new_description)
                                }
                            } else {
                                None
                            };

                        if maybe_title.is_some() || ctx.config.description.enabled {
                            Ok(Some(Action::UpdateMRTitleDescription(
                                UpdateMRTitleDescriptionAction::builder()
                                    .bookmark(BookmarkNameOrPendingChangeId::new_from_pointer(
                                        bookmark,
                                    ))
                                    .generate_stack_in_description(ctx.config.description.enabled)
                                    .maybe_title(maybe_title)
                                    .maybe_description(maybe_description)
                                    .dependencies(mr_action_ids.clone())
                                    .build(),
                            )))
                        } else {
                            Ok(None)
                        }
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();

            Ok::<_, Error>(actions)
        })
        .transpose()?
        .take_if(|batch| !batch.is_empty());

    let sync_dependent_merge_requests_enabled = ctx.forge.supports_dependent_merge_requests()
        && ctx.config.gitlab.create_merge_request_dependencies; // Kinda awkward but whatever

    let sync_dependent_merge_requests_batch = sync_dependent_merge_requests_enabled
        .then(|| {
            ctx.bookmark_graph
                .components()
                .iter()
                .flat_map(|component| {
                    component.all_bookmarks().into_iter().filter(|bookmark| {
                        bookmark
                            .parents
                            .iter()
                            .any(|p| matches!(p, BookmarkRef::Bookmark { .. }))
                    })
                })
                .map(|bookmark| {
                    Action::SyncDependentMergeRequests(
                        SyncDependentMergeRequestsAction::builder()
                            .bookmark(BookmarkNameOrPendingChangeId::new_from_pointer(bookmark))
                            .build(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .take_if(|batch| !batch.is_empty());

    if let Some(description_batch) = title_description_batch {
        batches.push(description_batch);
    }

    if let Some(sync_dependent_merge_requests_batch) = sync_dependent_merge_requests_batch {
        batches.push(sync_dependent_merge_requests_batch);
    }

    Ok(SubmissionPlan {
        actions: batches,
        existing_mrs,
    })
}
