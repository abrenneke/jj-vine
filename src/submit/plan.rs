use std::collections::HashMap;

use owo_colors::OwoColorize;

use crate::{
    bookmark::{BookmarkGraph, BookmarkRef},
    config::Config,
    error::Result,
    forge::{Forge, ForgeImpl},
    jj::Jujutsu,
    output::Output,
};

/// Action to perform during execution
#[derive(Debug, Clone, PartialEq)]
pub enum Action<'a> {
    /// Push a bookmark to remote
    Push { bookmark: String, remote: String },

    /// Create a new merge request
    CreateMR {
        bookmark: String,
        target_branch: String,
        title: String,
        description: String,
    },

    /// Update the target branch (base) of an existing MR
    UpdateMRBase {
        bookmark: String,
        mr_iid: String,
        new_target_branch: String,
    },

    /// Update MR description (after all MRs created)
    UpdateMRDescription {
        bookmark: String,
        bookmark_graph: BookmarkGraph<'a>,
    },

    /// Sync dependent merge requests (after all MRs created)
    SyncDependentMergeRequests {
        bookmark: String,
        bookmark_graph: BookmarkGraph<'a>,
    },
}

impl<'a> Action<'a> {
    pub fn get_group_text(&self) -> String {
        match self {
            Action::Push { .. } => "Pushing bookmarks".to_string(),
            Action::CreateMR { .. } => "Creating MRs".to_string(),
            Action::UpdateMRBase { .. } => "Updating MR bases".to_string(),
            Action::UpdateMRDescription { .. } => "Updating MR descriptions".to_string(),
            Action::SyncDependentMergeRequests { .. } => {
                "Syncing dependent merge requests".to_string()
            }
        }
    }

    pub fn get_text(&self) -> String {
        match self {
            Action::Push { bookmark, .. } => format!("Pushing {}", bookmark.magenta()),
            Action::CreateMR { bookmark, .. } => format!("Creating MR for {}", bookmark.magenta()),
            Action::UpdateMRBase {
                bookmark,
                mr_iid,
                new_target_branch,
            } => format!(
                "Updating MR {} base for {} to {}",
                mr_iid.cyan(),
                bookmark.magenta(),
                new_target_branch.magenta()
            ),
            Action::UpdateMRDescription { bookmark, .. } => {
                format!("Updating MR description for {}", bookmark.magenta())
            }
            Action::SyncDependentMergeRequests { bookmark, .. } => format!(
                "Syncing dependent merge requests for {}",
                bookmark.magenta()
            ),
        }
    }

    pub fn get_substep_text(&self) -> String {
        match self {
            Action::Push { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::CreateMR { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::UpdateMRBase { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::UpdateMRDescription { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::SyncDependentMergeRequests { bookmark, .. } => {
                format!("{}", bookmark.magenta())
            }
        }
    }
}

/// A planned action with ID and dependencies
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedAction<'a> {
    /// Sequential ID for this action
    pub id: usize,

    /// The actual action to perform
    pub action: Action<'a>,

    /// IDs of actions that must complete successfully before this action can
    /// run
    pub dependencies: Vec<usize>,
}

/// Plan for submission execution
#[derive(Debug, Clone)]
pub struct SubmissionPlan<'a> {
    /// Actions organized into batches. Each batch's actions execute in
    /// parallel, and batches execute sequentially.
    pub actions: Vec<Vec<PlannedAction<'a>>>,

    /// Whether this is a dry run (don't actually execute)
    pub dry_run: bool,
}

/// Create a submission plan based on analysis
pub async fn plan<'a>(
    jj: &Jujutsu,
    forge: &ForgeImpl,
    config: &Config,
    graph: &BookmarkGraph<'a>,
    dry_run: bool,
    output: &dyn Output,
) -> Result<SubmissionPlan<'a>> {
    output.log_current("Planning submission");

    let mut batches = Vec::new();
    let mut current_id = 1;
    let mut get_id = || {
        let id = current_id;
        current_id += 1;
        id
    };

    let mut existing_mrs = HashMap::new();
    for bookmark in graph.bookmarks() {
        output.set_substep(&format!("MRs for {}", bookmark.name().magenta()));
        if let Some(mr) = forge
            .find_merge_request_by_source_branch(bookmark.name())
            .await?
        {
            existing_mrs.insert(bookmark.name().to_string(), mr);
        }
    }

    output.set_substep("");

    let mut push_action_ids = HashMap::new();

    let mut push_batch = Vec::new();
    for bookmark in graph.bookmarks() {
        let action_id = get_id();

        push_action_ids.insert(bookmark.name().to_string(), action_id);

        push_batch.push(PlannedAction {
            id: action_id,
            action: Action::Push {
                bookmark: bookmark.name().to_string(),
                remote: config.remote_name.clone(),
            },
            dependencies: vec![],
        });
    }
    if !push_batch.is_empty() {
        batches.push(push_batch);
    }

    let mut mr_action_ids: Vec<usize> = Vec::new();

    let default_branch = jj.default_branch()?;

    for component in graph.components() {
        for bookmark in component.topological_sort()? {
            let bookmark = graph.find_bookmark_in_components(&bookmark).unwrap();

            // TODO let user pick target branch
            let target_branch = match bookmark.parents.first() {
                Some(BookmarkRef::Bookmark(b)) => b.name(),
                Some(BookmarkRef::Trunk) | None => default_branch,
            };

            let mut dependencies = Vec::new();

            // MR updates depend on the source & target bookmarks being pushed
            dependencies.extend(
                push_action_ids
                    .get(bookmark.name())
                    .copied()
                    .map(|id| vec![id])
                    .unwrap_or_default(),
            );

            // MR updates depend on the parent MRs being created
            dependencies.extend(
                bookmark
                    .parents
                    .iter()
                    .filter_map(|p| match p {
                        BookmarkRef::Bookmark(b) => Some(batches.iter().flat_map(|batch| {
                            batch.iter().filter_map(|action| match &action.action {
                                Action::CreateMR { bookmark, .. } if bookmark == b.name() => {
                                    Some(action.id)
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
                        let action_id = get_id();
                        mr_action_ids.push(action_id);

                        batches.push(vec![PlannedAction {
                            id: action_id,
                            action: Action::UpdateMRBase {
                                bookmark: bookmark.name().to_string(),
                                mr_iid: existing_mr.iid().to_string(),
                                new_target_branch: target_branch.to_string(),
                            },
                            dependencies,
                        }]);
                    }
                }
                None => {
                    let title = get_mr_title(jj, bookmark.name(), target_branch)?;
                    let action_id = get_id();
                    mr_action_ids.push(action_id);

                    batches.push(vec![PlannedAction {
                        id: action_id,
                        action: Action::CreateMR {
                            bookmark: bookmark.name().to_string(),
                            target_branch: target_branch.to_string(),
                            title,
                            description: String::new(),
                        },
                        dependencies,
                    }]);
                }
            }
        }
    }

    if config.description.enabled {
        let bookmarks_needing_descriptions: Vec<_> = graph
            .bookmarks()
            .filter(|bookmark| {
                if let Some(stack) = graph.component_containing(bookmark.name())
                    && stack.len() >= 2
                {
                    true
                } else {
                    false
                }
            })
            .collect();

        let mut description_batch = Vec::new();
        for bookmark in &bookmarks_needing_descriptions {
            let action_id = get_id();

            description_batch.push(PlannedAction {
                id: action_id,
                action: Action::UpdateMRDescription {
                    bookmark: bookmark.name().to_string(),
                    bookmark_graph: graph.clone(),
                },
                dependencies: mr_action_ids.clone(),
            });
        }

        if !description_batch.is_empty() {
            batches.push(description_batch);
        }
    }

    let sync_dependent_merge_requests_batch = graph
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
        .map(|bookmark| PlannedAction {
            id: get_id(),
            action: Action::SyncDependentMergeRequests {
                bookmark: bookmark.name().to_string(),
                bookmark_graph: graph.clone(),
            },
            dependencies: vec![],
        })
        .collect::<Vec<_>>();

    if !sync_dependent_merge_requests_batch.is_empty() {
        batches.push(sync_dependent_merge_requests_batch);
    }

    Ok(SubmissionPlan {
        actions: batches,
        dry_run,
    })
}

fn get_mr_title(jj: &Jujutsu, bookmark: &str, base: &str) -> Result<String> {
    let changes = jj.log(format!("::{}  ~ ::{}", bookmark, base))?;
    match &changes[..] {
        [change] if !change.description_first_line.is_empty() => {
            Ok(change.description_first_line.to_string())
        }
        _ => Ok(bookmark.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_action_push() {
        let action = Action::Push {
            bookmark: "feature".to_string(),
            remote: "origin".to_string(),
        };

        assert!(
            matches!(&action, Action::Push { .. }),
            "Expected Push action, got {:?}",
            action
        );

        if let Action::Push { bookmark, remote } = action {
            assert_eq!(bookmark, "feature");
            assert_eq!(remote, "origin");
        }
    }

    #[test]
    fn test_action_create_mr() {
        let action = Action::CreateMR {
            bookmark: "feature".to_string(),
            target_branch: "main".to_string(),
            title: "[jj-vine] feature".to_string(),
            description: "Stack visualization here".to_string(),
        };

        assert!(
            matches!(&action, Action::CreateMR { .. }),
            "Expected CreateMR action, got {:?}",
            action
        );

        if let Action::CreateMR {
            bookmark,
            target_branch,
            title,
            description,
        } = action
        {
            assert_eq!(bookmark, "feature");
            assert_eq!(target_branch, "main");
            assert_eq!(title, "[jj-vine] feature");
            assert_eq!(description, "Stack visualization here");
        }
    }

    #[test]
    fn test_submission_plan_struct() {
        let plan = SubmissionPlan {
            actions: vec![
                vec![PlannedAction {
                    id: 1,
                    action: Action::Push {
                        bookmark: "feature".to_string(),
                        remote: "origin".to_string(),
                    },
                    dependencies: vec![],
                }],
                vec![PlannedAction {
                    id: 2,
                    action: Action::CreateMR {
                        bookmark: "feature".to_string(),
                        target_branch: "main".to_string(),
                        title: "[jj-vine] feature".to_string(),
                        description: "".to_string(),
                    },
                    dependencies: vec![1],
                }],
            ],
            dry_run: false,
        };

        assert_eq!(plan.actions.len(), 2);
        assert!(!plan.dry_run);
        assert_eq!(plan.actions[0][0].id, 1);
        assert_eq!(plan.actions[1][0].id, 2);
        assert_eq!(plan.actions[1][0].dependencies, vec![1]);
    }

    #[test]
    fn test_get_mr_title_single_commit() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        let jj = Jujutsu::new(&repo_path)?;

        jj.exec(["git", "init"])?;

        std::fs::write(repo_path.join("README.md"), "# Test\n")?;

        jj.exec(["describe", "-m", "Initial commit"])?;
        jj.exec(["bookmark", "create", "main"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("feature.txt"), "feature content\n")?;
        jj.exec(["describe", "-m", "Add awesome feature"])?;
        jj.exec(["bookmark", "create", "feature"])?;

        assert_eq!(
            get_mr_title(&jj, "feature", "main")?,
            "Add awesome feature".to_string()
        );

        Ok(())
    }

    #[test]
    fn test_get_mr_title_multiple_commits() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        let jj = Jujutsu::new(&repo_path)?;

        jj.exec(["git", "init"])?;

        std::fs::write(repo_path.join("README.md"), "# Test\n")?;

        jj.exec(["describe", "-m", "Initial commit"])?;
        jj.exec(["bookmark", "create", "main"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("file1.txt"), "content 1\n")?;
        jj.exec(["describe", "-m", "First commit"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("file2.txt"), "content 2\n")?;
        jj.exec(["describe", "-m", "Second commit"])?;
        jj.exec(["bookmark", "create", "multi-commit-feature"])?;

        assert_eq!(
            get_mr_title(&jj, "multi-commit-feature", "main")?,
            "multi-commit-feature"
        );

        Ok(())
    }

    #[test]
    fn test_get_mr_title_empty_description() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        let jj = Jujutsu::new(&repo_path)?;

        jj.exec(["git", "init"])?;

        std::fs::write(repo_path.join("README.md"), "# Test\n")?;

        jj.exec(["describe", "-m", "Initial commit"])?;
        jj.exec(["bookmark", "create", "main"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("file.txt"), "content\n")?;
        jj.exec(["describe", "-m", ""])?;
        jj.exec(["bookmark", "create", "empty-desc"])?;

        assert_eq!(get_mr_title(&jj, "empty-desc", "main")?, "empty-desc");

        Ok(())
    }

    #[test]
    fn test_get_mr_title_stacked_bookmarks() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        let jj = Jujutsu::new(&repo_path)?;

        jj.exec(["git", "init", "--colocate"])?;

        std::fs::write(repo_path.join("README.md"), "# Test\n")?;
        jj.exec(["describe", "-m", "Initial commit"])?;
        jj.exec(["bookmark", "create", "main"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("auth.txt"), "auth code\n")?;
        jj.exec(["describe", "-m", "Add authentication"])?;
        jj.exec(["bookmark", "create", "feature-a"])?;

        jj.exec(["new"])?;
        std::fs::write(repo_path.join("logging.txt"), "logging code\n")?;
        jj.exec(["describe", "-m", "Add logging"])?;
        jj.exec(["bookmark", "create", "feature-b"])?;

        assert_eq!(
            get_mr_title(&jj, "feature-a", "main")?,
            "Add authentication"
        );

        assert_eq!(get_mr_title(&jj, "feature-b", "feature-a")?, "Add logging");

        Ok(())
    }
}
