use crate::config::{Config, StackFormat};
use crate::description::{
    DescriptionFormatter, LinearListFormatter, StackBookmarkInfo, StackContext,
};
use crate::error::Result;
use crate::gitlab::{GitLabClient, MergeRequest};
use crate::jj::Jujutsu;
use crate::submit::analyze::SubmissionAnalysis;
use std::collections::HashMap;

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

/// Plan for submission execution
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Actions to perform, in order
    pub actions: Vec<Action>,

    /// Whether this is a dry run (don't actually execute)
    pub dry_run: bool,
}

/// Create a submission plan based on analysis
///
/// Phase 2 of the three-phase submission process:
/// - Query GitLab for existing MRs
/// - Check remote bookmark status
/// - Determine what actions are needed
pub async fn plan(
    analysis: &SubmissionAnalysis,
    jj: &Jujutsu,
    gitlab: &GitLabClient,
    config: &Config,
    bookmark_graph: &crate::bookmark::BookmarkGraph,
    dry_run: bool,
) -> Result<SubmissionPlan> {
    let mut actions = Vec::new();

    // Query all existing MRs upfront (needed for description generation)
    let mut existing_mrs = HashMap::new();
    for bookmark in &analysis.bookmarks_to_submit {
        if let Some(mr) = gitlab.find_mr_by_source_branch(bookmark).await? {
            existing_mrs.insert(bookmark.clone(), mr);
        }
    }

    for (idx, bookmark) in analysis.bookmarks_to_submit.iter().enumerate() {
        // Always push the bookmark
        actions.push(Action::Push {
            bookmark: bookmark.clone(),
            remote: config.remote_name.clone(),
        });

        // Determine the target branch for this bookmark's MR
        let target_branch = if idx == 0 {
            // First bookmark in stack -> target the base branch
            analysis.base_branch.clone()
        } else {
            // Other bookmarks -> target the previous bookmark
            analysis.bookmarks_to_submit[idx - 1].clone()
        };

        // Check if an MR already exists
        match existing_mrs.get(bookmark) {
            Some(existing_mr) => {
                // MR exists - check if we need to update the target branch
                if existing_mr.target_branch != target_branch {
                    actions.push(Action::UpdateMRBase {
                        bookmark: bookmark.clone(),
                        mr_iid: existing_mr.iid,
                        new_target_branch: target_branch.clone(),
                    });
                }
                // Description will be updated via deferred action
            }
            None => {
                // No MR exists - create one with empty description
                // Description will be set via deferred action
                let title = get_mr_title(jj, bookmark, &target_branch)?;

                actions.push(Action::CreateMR {
                    bookmark: bookmark.clone(),
                    target_branch,
                    title,
                    description: String::new(),
                });
            }
        }
    }

    // Add description updates for ALL bookmarks in a stack
    // These will execute AFTER all MRs are created, ensuring all descriptions have MR IIDs
    if config.enable_stack_visualization && analysis.bookmarks_to_submit.len() > 1 {
        for bookmark in &analysis.bookmarks_to_submit {
            actions.push(Action::UpdateMRDescription {
                bookmark: bookmark.clone(),
                bookmark_graph: bookmark_graph.clone(),
                bookmarks_being_submitted: analysis.bookmarks_to_submit.clone(),
            });
        }
    }

    Ok(SubmissionPlan { actions, dry_run })
}

/// Generate description for a bookmark that may be in multiple stacks
pub fn generate_multi_stack_description(
    bookmark: &str,
    stacks: &[&crate::bookmark::BranchStack],
    existing_mrs: &HashMap<String, MergeRequest>,
    format: &StackFormat,
    base_branch: &str,
) -> Result<String> {
    if stacks.is_empty() {
        return Ok(String::new());
    }

    // Create formatter based on config
    let formatter: Box<dyn DescriptionFormatter> = match format {
        StackFormat::Linear => Box::new(LinearListFormatter),
    };

    if stacks.len() == 1 {
        // Single stack - use existing format
        let stack = stacks[0];
        let stack_info: Vec<StackBookmarkInfo> = stack
            .bookmarks
            .iter()
            .map(|bm| StackBookmarkInfo {
                name: bm.clone(),
                mr_iid: existing_mrs.get(bm).map(|mr| mr.iid),
                mr_url: existing_mrs.get(bm).map(|mr| mr.web_url.clone()),
            })
            .collect();

        let context = StackContext {
            bookmarks: stack_info,
            base_branch: base_branch.to_string(),
        };

        return Ok(formatter.format_stack(&context, bookmark));
    }

    // Multiple stacks - format each separately
    let mut lines = Vec::new();
    lines.push(format!("This MR is part of {} stacks:", stacks.len()));
    lines.push("".to_string());

    for (idx, stack) in stacks.iter().enumerate() {
        lines.push(format!(
            "Stack {} ({} MRs):",
            idx + 1,
            stack.bookmarks.len()
        ));

        // Build StackContext for this stack
        let stack_info: Vec<StackBookmarkInfo> = stack
            .bookmarks
            .iter()
            .map(|bm| StackBookmarkInfo {
                name: bm.clone(),
                mr_iid: existing_mrs.get(bm).map(|mr| mr.iid),
                mr_url: existing_mrs.get(bm).map(|mr| mr.web_url.clone()),
            })
            .collect();

        let context = StackContext {
            bookmarks: stack_info,
            base_branch: base_branch.to_string(),
        };

        // Format this stack
        let stack_desc = formatter.format_stack(&context, bookmark);

        // Add indented stack lines (skip the header line "This MR is part of...")
        for line in stack_desc.lines().skip(2) {
            lines.push(line.to_string());
        }

        if idx < stacks.len() - 1 {
            lines.push("".to_string()); // Blank line between stacks
        }
    }

    Ok(lines.join("\n"))
}

/// Determine the title for an MR based on the number of commits
///
/// If the bookmark contains exactly one commit, use the commit's first line as the title.
/// Otherwise, use the bookmark name.
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

    let descriptions: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();

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
            title: "[jj-mrs] feature".to_string(),
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
            assert_eq!(title, "[jj-mrs] feature");
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
                Action::Push {
                    bookmark: "feature".to_string(),
                    remote: "origin".to_string(),
                },
                Action::CreateMR {
                    bookmark: "feature".to_string(),
                    target_branch: "main".to_string(),
                    title: "[jj-mrs] feature".to_string(),
                    description: "".to_string(),
                },
            ],
            dry_run: false,
        };

        assert_eq!(plan.actions.len(), 2);
        assert!(!plan.dry_run);
    }

    #[test]
    fn test_get_mr_title_single_commit() {
        use crate::jj::Jujutsu;
        use std::fs;
        use std::process::Command as StdCommand;
        use tempfile::TempDir;

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
        use crate::jj::Jujutsu;
        use std::fs;
        use std::process::Command as StdCommand;
        use tempfile::TempDir;

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
        use crate::jj::Jujutsu;
        use std::fs;
        use std::process::Command as StdCommand;
        use tempfile::TempDir;

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
        use crate::jj::Jujutsu;
        use std::fs;
        use std::process::Command as StdCommand;
        use tempfile::TempDir;

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
