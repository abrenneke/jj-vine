use std::collections::HashMap;

use owo_colors::OwoColorize;

use crate::{
    bookmark::BookmarkGraph,
    config::Config,
    error::Result,
    gitlab::GitLabClient,
    jj::Jujutsu,
    output::Output,
    submit::analyze::SubmissionAnalysis,
};

/// Action to perform during execution
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
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
        mr_iid: u64,
        new_target_branch: String,
    },

    /// Update MR description (after all MRs created)
    UpdateMRDescription {
        bookmark: String,
        bookmark_graph: crate::bookmark::BookmarkGraph,
        bookmarks_being_submitted: Vec<String>,
    },
}

impl Action {
    pub fn get_group_text(&self) -> String {
        match self {
            Action::Push { .. } => "Pushing bookmarks".to_string(),
            Action::CreateMR { .. } => "Creating MRs".to_string(),
            Action::UpdateMRBase { .. } => "Updating MR bases".to_string(),
            Action::UpdateMRDescription { .. } => "Updating MR descriptions".to_string(),
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
        }
    }

    pub fn get_substep_text(&self) -> String {
        match self {
            Action::Push { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::CreateMR { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::UpdateMRBase { bookmark, .. } => format!("{}", bookmark.magenta()),
            Action::UpdateMRDescription { bookmark, .. } => format!("{}", bookmark.magenta()),
        }
    }
}

/// A planned action with ID and dependencies
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedAction {
    /// Sequential ID for this action
    pub id: usize,

    /// The actual action to perform
    pub action: Action,

    /// IDs of actions that must complete successfully before this action can
    /// run
    pub dependencies: Vec<usize>,
}

/// Plan for submission execution
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Actions organized into batches. Each batch's actions execute in
    /// parallel, and batches execute sequentially.
    pub actions: Vec<Vec<PlannedAction>>,

    /// Whether this is a dry run (don't actually execute)
    pub dry_run: bool,
}

/// Create a submission plan based on analysis
pub async fn plan(
    analysis: &SubmissionAnalysis,
    jj: &Jujutsu,
    gitlab: &GitLabClient,
    config: &Config,
    bookmark_graph: &BookmarkGraph,
    dry_run: bool,
    output: &dyn Output,
) -> Result<SubmissionPlan> {
    let mut batches = Vec::new();
    let mut current_id = 1;
    let get_id = &mut || {
        let id = current_id;
        current_id += 1;
        id
    };

    let mut existing_mrs = HashMap::new();
    for bookmark in &analysis.bookmarks_to_submit {
        output.set_substep(&format!("MRs for {}", bookmark));
        if let Some(mr) = gitlab.find_mr_by_source_branch(bookmark).await? {
            existing_mrs.insert(bookmark.clone(), mr);
        }
    }

    output.set_substep("");

    let mut push_action_ids: HashMap<String, usize> = HashMap::new();

    let mut push_batch = Vec::new();
    for bookmark in &analysis.bookmarks_to_submit {
        let action_id = get_id();

        push_action_ids.insert(bookmark.clone(), action_id);

        push_batch.push(PlannedAction {
            id: action_id,
            action: Action::Push {
                bookmark: bookmark.clone(),
                remote: config.remote_name.clone(),
            },
            dependencies: vec![],
        });
    }
    if !push_batch.is_empty() {
        batches.push(push_batch);
    }

    let mut mr_action_ids: Vec<usize> = Vec::new();

    for bookmark in &analysis.bookmarks_to_submit {
        let target_branch = bookmark_graph
            .get_parent(bookmark)
            .cloned()
            .unwrap_or_else(|| analysis.base_branch.clone());

        let push_dependency = push_action_ids
            .get(bookmark)
            .copied()
            .map(|id| vec![id])
            .unwrap_or_default();

        match existing_mrs.get(bookmark) {
            Some(existing_mr) => {
                if existing_mr.target_branch != target_branch {
                    let action_id = get_id();
                    mr_action_ids.push(action_id);

                    batches.push(vec![PlannedAction {
                        id: action_id,
                        action: Action::UpdateMRBase {
                            bookmark: bookmark.clone(),
                            mr_iid: existing_mr.iid,
                            new_target_branch: target_branch.clone(),
                        },
                        dependencies: push_dependency,
                    }]);
                }
            }
            None => {
                let title = get_mr_title(jj, bookmark, &target_branch)?;
                let action_id = get_id();
                mr_action_ids.push(action_id);

                batches.push(vec![PlannedAction {
                    id: action_id,
                    action: Action::CreateMR {
                        bookmark: bookmark.clone(),
                        target_branch,
                        title,
                        description: String::new(),
                    },
                    dependencies: push_dependency,
                }]);
            }
        }
    }

    if config.enable_stack_visualization {
        let bookmarks_needing_descriptions: Vec<String> = analysis
            .bookmarks_to_submit
            .iter()
            .filter(|bookmark| {
                if let Some(stack) = bookmark_graph.find_stack_for_bookmark(bookmark) {
                    stack.bookmarks.len() >= 2
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        if !bookmarks_needing_descriptions.is_empty() {
            let mut description_batch = Vec::new();
            for bookmark in &bookmarks_needing_descriptions {
                let action_id = get_id();

                description_batch.push(PlannedAction {
                    id: action_id,
                    action: Action::UpdateMRDescription {
                        bookmark: bookmark.clone(),
                        bookmark_graph: bookmark_graph.clone(),
                        bookmarks_being_submitted: analysis.bookmarks_to_submit.clone(),
                    },
                    dependencies: mr_action_ids.clone(),
                });
            }
            batches.push(description_batch);
        }
    }

    Ok(SubmissionPlan {
        actions: batches,
        dry_run,
    })
}

/// Determine the title for an MR based on the number of commits
///
/// If the bookmark contains exactly one commit, use the commit's first line as
/// the title. Otherwise, use the bookmark name.
fn get_mr_title(jj: &Jujutsu, bookmark: &str, base: &str) -> Result<String> {
    // Build revset to get commits between base and bookmark (excluding base itself)
    let revset = format!("::{}  ~ ::{}", bookmark, base);

    // Get commit descriptions using the same revset
    let output = jj.run_captured(&[
        "log",
        "-r",
        &revset,
        "--no-graph",
        "--template",
        r#"description.first_line() ++ "\n""#,
    ])?;

    let descriptions: Vec<&str> = output
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if descriptions.len() == 1 {
        // Exactly one commit - use its description as title
        let title = descriptions[0].trim();
        if !title.is_empty() {
            return Ok(title.to_string());
        }
    }

    // Fall back to bookmark name for multiple commits or edge cases
    Ok(bookmark.to_string())
}

#[cfg(test)]
mod tests {
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
    fn test_action_create_mr_with_description() {
        let action = Action::CreateMR {
            bookmark: "test".to_string(),
            target_branch: "main".to_string(),
            title: "Test".to_string(),
            description: "Stack info".to_string(),
        };
        assert!(matches!(action, Action::CreateMR { .. }));
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
    fn test_get_mr_title_single_commit() {
        use std::{fs, process::Command as StdCommand};

        use tempfile::TempDir;

        use crate::jj::Jujutsu;

        // Create test repo
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize jj repo
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["git", "init", "--colocate"])
            .output()
            .expect("Failed to init jj repo");

        // Create initial commit on main
        fs::write(repo_path.join("README.md"), "# Test\n").expect("Failed to write README");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Initial commit"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "main"])
            .output()
            .expect("Failed to create main bookmark");

        // Create a new change with specific description
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("feature.txt"), "feature content\n")
            .expect("Failed to write feature file");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Add awesome feature"])
            .output()
            .expect("Failed to describe feature");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "feature"])
            .output()
            .expect("Failed to create feature bookmark");

        let jj = Jujutsu::new(repo_path).unwrap();

        // Test: Single commit should use commit description
        let title = get_mr_title(&jj, "feature", "main").unwrap();
        assert_eq!(title, "Add awesome feature");
    }

    #[test]
    fn test_get_mr_title_multiple_commits() {
        use std::{fs, process::Command as StdCommand};

        use tempfile::TempDir;

        use crate::jj::Jujutsu;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["git", "init", "--colocate"])
            .output()
            .expect("Failed to init jj repo");

        fs::write(repo_path.join("README.md"), "# Test\n").expect("Failed to write README");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Initial commit"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "main"])
            .output()
            .expect("Failed to create main bookmark");

        // Create first commit
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("file1.txt"), "content 1\n").expect("Failed to write file1");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "First commit"])
            .output()
            .expect("Failed to describe");

        // Create second commit
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("file2.txt"), "content 2\n").expect("Failed to write file2");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Second commit"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "multi-commit-feature"])
            .output()
            .expect("Failed to create bookmark");

        let jj = Jujutsu::new(repo_path).unwrap();

        // Test: Multiple commits should use bookmark name
        let title = get_mr_title(&jj, "multi-commit-feature", "main").unwrap();
        assert_eq!(title, "multi-commit-feature");
    }

    #[test]
    fn test_get_mr_title_empty_description() {
        use std::{fs, process::Command as StdCommand};

        use tempfile::TempDir;

        use crate::jj::Jujutsu;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["git", "init", "--colocate"])
            .output()
            .expect("Failed to init jj repo");

        fs::write(repo_path.join("README.md"), "# Test\n").expect("Failed to write README");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Initial commit"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "main"])
            .output()
            .expect("Failed to create main bookmark");

        // Create change with empty description
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("file.txt"), "content\n").expect("Failed to write file");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", ""])
            .output()
            .expect("Failed to describe with empty message");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "empty-desc"])
            .output()
            .expect("Failed to create bookmark");

        let jj = Jujutsu::new(repo_path).unwrap();

        // Test: Empty description should fall back to bookmark name
        let title = get_mr_title(&jj, "empty-desc", "main").unwrap();
        assert_eq!(title, "empty-desc");
    }

    #[test]
    fn test_get_mr_title_stacked_bookmarks() {
        use std::{fs, process::Command as StdCommand};

        use tempfile::TempDir;

        use crate::jj::Jujutsu;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["git", "init", "--colocate"])
            .output()
            .expect("Failed to init jj repo");

        fs::write(repo_path.join("README.md"), "# Test\n").expect("Failed to write README");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Initial commit"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "main"])
            .output()
            .expect("Failed to create main bookmark");

        // Create first bookmark with single commit
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("auth.txt"), "auth code\n").expect("Failed to write file");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Add authentication"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "feature-a"])
            .output()
            .expect("Failed to create bookmark");

        // Create second bookmark with single commit (stacked on first)
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["new"])
            .output()
            .expect("Failed to create new change");
        fs::write(repo_path.join("logging.txt"), "logging code\n").expect("Failed to write file");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["describe", "-m", "Add logging"])
            .output()
            .expect("Failed to describe");
        StdCommand::new("jj")
            .current_dir(&repo_path)
            .args(["bookmark", "create", "feature-b"])
            .output()
            .expect("Failed to create bookmark");

        let jj = Jujutsu::new(repo_path).unwrap();

        // Test: First bookmark relative to main
        let title_a = get_mr_title(&jj, "feature-a", "main").unwrap();
        assert_eq!(title_a, "Add authentication");

        // Test: Second bookmark relative to first bookmark
        let title_b = get_mr_title(&jj, "feature-b", "feature-a").unwrap();
        assert_eq!(title_b, "Add logging");
    }
}
