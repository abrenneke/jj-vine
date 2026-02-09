use std::{collections::HashMap, path::Path};

use futures::{StreamExt, stream::FuturesUnordered};
use owo_colors::OwoColorize;
use snafu::whatever;

use crate::{
    bookmark::BookmarkRef,
    description::{generate_description, remove_jj_vine_stack_from_description},
    error::{Error, Result},
    forge::{AnyForgeMergeRequest, Forge},
    submit::{
        SubmitContext,
        execute::{
            Action,
            ActionInfo,
            create_mr::CreateMRAction,
            load_all_mrs::LoadAllMRsAction,
            push::PushAction,
            sync_dependent_merge_requests::SyncDependentMergeRequestsAction,
            update_mr_base::UpdateMRBaseAction,
            update_mr_title_description::UpdateMRTitleDescriptionAction,
        },
    },
    title::get_mr_title,
};

/// Plan for submission execution
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Actions organized into batches. Each batch's actions execute in
    /// parallel, and batches execute sequentially.
    pub actions: Vec<Vec<Action>>,
}

impl std::fmt::Display for SubmissionPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, batch) in self.actions.iter().enumerate() {
            writeln!(f, "Batch {}:", i + 1)?;
            for action in batch {
                writeln!(f, "  {}", action.plan_text())?;
            }
        }
        Ok(())
    }
}

#[allow(dead_code)]
struct PlanMergeRequest {
    bookmark: String,
    target_branch: String,
    title: String,
    description: String,
}

impl PlanMergeRequest {
    fn from_forge_mr(mr: &AnyForgeMergeRequest) -> Self {
        Self {
            bookmark: mr.source_branch().to_string(),
            target_branch: mr.target_branch().to_string(),
            title: mr.title().to_string(),
            description: mr.description().to_string(),
        }
    }
}

/// Create a submission plan
pub async fn plan(ctx: SubmitContext<'_>) -> Result<SubmissionPlan> {
    ctx.output.log_current("Planning submission");

    let default_branch = ctx.jj.default_branch()?;

    let mut batches: Vec<Vec<Action>> = Vec::new();

    let existing_mrs: HashMap<_, _> = ctx
        .bookmark_graph
        .components()
        .iter()
        .flat_map(|component| component.all_bookmarks())
        .map(async |bookmark| {
            let _substep = ctx
                .output
                .start_substep(&bookmark.name().magenta().to_string());

            if let Some(mr) = ctx
                .forge
                .find_merge_request_by_source_branch_base_branch(
                    bookmark.name(),
                    &bookmark.parent_name(default_branch),
                )
                .await?
            {
                Ok(Some((bookmark.name().to_string(), mr)))
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
        .collect();

    let mut push_action_ids = HashMap::new();

    let mut push_batch = Vec::new();
    for bookmark in ctx.bookmark_graph.bookmarks() {
        let push_action = PushAction::builder()
            .bookmark(bookmark.name().to_string())
            .remote(ctx.config.remote_name.clone())
            .build();

        push_action_ids.insert(bookmark.name().to_string(), push_action.id());
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
                mr.source_branch().to_string(),
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

            let target_branch = bookmark.parent_name(default_branch);

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
                                    if create_mr_action.bookmark == b.name() =>
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

            match existing_mrs.get(bookmark.name()) {
                Some(existing_mr) => {
                    if existing_mr.target_branch() != target_branch {
                        let update_mr_base_action = UpdateMRBaseAction::builder()
                            .bookmark(bookmark.name().to_string())
                            .mr_iid(existing_mr.iid().to_string())
                            .new_target_branch(target_branch.to_string())
                            .dependencies(dependencies)
                            .build();

                        mr_action_ids.push(update_mr_base_action.id());

                        batches.push(vec![Action::UpdateMRBase(update_mr_base_action)]);
                    }
                }
                None => {
                    let title =
                        get_mr_title(ctx.jj, &ctx.config.title, bookmark.clone(), component)?;

                    let description = generate_description(
                        &ctx.config.description,
                        &bookmark.revisions(ctx.jj)?,
                        Path::new(&ctx.jj.exec(["root"])?.stdout),
                    );

                    let create_mr_action = CreateMRAction::builder()
                        .bookmark(bookmark.name().to_string())
                        .target_branch(target_branch.to_string())
                        .title(title.clone())
                        .description(description.clone())
                        .dependencies(dependencies)
                        .build();

                    mr_action_ids.push(create_mr_action.id());

                    batches.push(vec![Action::CreateMR(create_mr_action)]);

                    plan_mrs.insert(
                        bookmark.name().to_string(),
                        PlanMergeRequest {
                            bookmark: bookmark.name().to_string(),
                            target_branch: target_branch.to_string(),
                            title,
                            description,
                        },
                    );
                }
            }
        }
    }

    let title_description_batch = (ctx.config.description.enabled
        || ctx.config.title.sync
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
                            whatever!("No MR found for {}", bookmark.name())
                        };

                        let maybe_title = if ctx.config.title.sync {
                            let new_title = get_mr_title(
                                ctx.jj,
                                &ctx.config.title,
                                bookmark.clone(),
                                component,
                            )?;

                            if new_title != *current_title {
                                Some(new_title)
                            } else {
                                None
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
                                    != remove_jj_vine_stack_from_description(current_description)
                                {
                                    Some(new_description)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                        if maybe_title.is_some() || ctx.config.description.enabled {
                            Ok(Some(Action::UpdateMRTitleDescription(
                                UpdateMRTitleDescriptionAction::builder()
                                    .bookmark(bookmark.name().to_string())
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
                            .bookmark(bookmark.name().to_string())
                            .build(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .take_if(|batch| !batch.is_empty());

    // TODO better dependency handling instead of just sticking it here manually
    if title_description_batch.is_some() || sync_dependent_merge_requests_batch.is_some() {
        batches.push(vec![Action::LoadAllMRs(LoadAllMRsAction)]);
    }

    if let Some(description_batch) = title_description_batch {
        batches.push(description_batch);
    }

    if let Some(sync_dependent_merge_requests_batch) = sync_dependent_merge_requests_batch {
        batches.push(sync_dependent_merge_requests_batch);
    }

    Ok(SubmissionPlan { actions: batches })
}
