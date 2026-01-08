use crate::error::{Error, Result};
use snafu::ResultExt;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, trace};

/// Jujutsu subprocess interface
pub struct Jujutsu {
    repo_path: PathBuf,
}

impl Jujutsu {
    /// Create a new Jujutsu instance for the given repository path
    pub fn new(repo_path: PathBuf) -> Result<Self> {
        which_jj()?; // Verify jj is available
        Ok(Self { repo_path })
    }

    /// Run a jj command and return the output
    pub fn run_captured(&self, args: &[&str]) -> Result<String> {
        trace!("Running jj command: jj {}", args.join(" "));
        run_jj_command(&self.repo_path, args)
    }

    /// Get all bookmarks authored by the current user with their commit info
    pub fn get_bookmarks(&self) -> Result<Vec<Bookmark>> {
        // Use jj log with mine() & bookmarks() revset to get only user's bookmarks
        // Each bookmark will appear on its own line with its commit/change ID
        let output = self.run_captured(&[
            "log",
            "-r",
            "mine() & bookmarks()",
            "--no-graph",
            "--template",
            // For each commit with bookmarks, output each bookmark name on a separate line
            r#"bookmarks.map(|b| b ++ "\t" ++ commit_id ++ "\t" ++ change_id).join("\n") ++ "\n""#,
        ])?;

        let mut bookmarks = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                // Parse bookmark name (might have @remote suffix and/or * suffix for tracking conflicts)
                let full_name = parts[0];

                // Strip trailing * if present (indicates tracking conflict/divergence)
                let full_name = full_name.strip_suffix('*').unwrap_or(full_name);

                let (name, remote) = if let Some(at_pos) = full_name.rfind('@') {
                    let name = full_name[..at_pos].to_string();
                    let remote = full_name[at_pos + 1..].to_string();
                    (name, Some(remote))
                } else {
                    (full_name.to_string(), None)
                };

                let is_local = remote.is_none();
                // For now, assume local bookmarks might have remotes (we'd need git ls-remote to check)
                let has_remote = false;

                bookmarks.push(Bookmark {
                    name,
                    commit_id: parts[1].to_string(),
                    change_id: parts[2].to_string(),
                    remote,
                    is_local,
                    has_remote,
                });
            }
        }

        Ok(bookmarks)
    }

    /// Get bookmarks matching a custom revset
    pub fn get_bookmarks_with_revset(&self, revset: &str) -> Result<Vec<Bookmark>> {
        let output = self.run_captured(&[
            "log",
            "-r",
            revset,
            "--no-graph",
            "--template",
            r#"bookmarks.map(|b| b ++ "\t" ++ commit_id ++ "\t" ++ change_id).join("\n") ++ "\n""#,
        ])?;

        let mut bookmarks = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let full_name = parts[0];
                let full_name = full_name.strip_suffix('*').unwrap_or(full_name);

                let (name, remote) = if let Some(at_pos) = full_name.rfind('@') {
                    let name = full_name[..at_pos].to_string();
                    let remote = full_name[at_pos + 1..].to_string();
                    (name, Some(remote))
                } else {
                    (full_name.to_string(), None)
                };

                let is_local = remote.is_none();
                let has_remote = false;

                bookmarks.push(Bookmark {
                    name,
                    commit_id: parts[1].to_string(),
                    change_id: parts[2].to_string(),
                    remote,
                    is_local,
                    has_remote,
                });
            }
        }

        Ok(bookmarks)
    }

    /// Get changes between two revisions
    pub fn get_changes(&self, from: &str, to: &str) -> Result<Vec<Change>> {
        let revset = format!("{}::{}", from, to);
        let output = self.run_captured(&[
            "log",
            "-r",
            &revset,
            "--no-graph",
            "--template",
            r#"commit_id ++ "\t" ++ change_id ++ "\t" ++ description.first_line() ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\n""#,
        ])?;

        let mut changes = Vec::new();
        for line in output.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                changes.push(Change {
                    commit_id: parts[0].to_string(),
                    change_id: parts[1].to_string(),
                    description_first_line: parts[2].to_string(),
                    parent_ids: parts[3]
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect(),
                });
            }
        }

        Ok(changes)
    }

    /// Resolve a revision to a commit ID
    pub fn resolve_revision(&self, revset: &str) -> Result<String> {
        trace!("Resolving revision: {}", revset);
        let output = self.run_captured(&[
            "log",
            "-r",
            revset,
            "--limit",
            "1",
            "--no-graph",
            "--template",
            "commit_id",
        ])?;

        Ok(output.trim().to_string())
    }

    /// Get the change ID for a commit
    pub fn get_change_id(&self, commit_id: &str) -> Result<String> {
        let output = self.run_captured(&[
            "log",
            "-r",
            commit_id,
            "--limit",
            "1",
            "--no-graph",
            "--template",
            "change_id",
        ])?;

        Ok(output.trim().to_string())
    }

    /// Get the default branch name (trunk)
    pub fn get_default_branch(&self) -> Result<String> {
        // Try common default branch names
        for branch in &["main", "master", "trunk"] {
            let revset = format!("{}@origin", branch);
            if self.resolve_revision(&revset).is_ok() {
                return Ok(branch.to_string());
            }
        }

        Err(Error::Config {
            message: "Could not find default branch (tried main, master, trunk)".to_string(),
        })
    }

    /// Track a bookmark on a remote
    pub fn track_bookmark(&self, bookmark: &str, remote: &str) -> Result<()> {
        let remote_bookmark = format!("{}@{}", bookmark, remote);
        self.run_captured(&["bookmark", "track", &remote_bookmark])?;
        Ok(())
    }

    /// Push a bookmark to a remote using jj git push
    ///
    /// This will automatically track the bookmark on the remote if it's not already tracked
    pub fn push_bookmark(&self, bookmark: &str, remote: &str) -> Result<()> {
        // Try to track the bookmark first (ignore errors if already tracked)
        let _ = self.track_bookmark(bookmark, remote);

        self.run_captured(&["git", "push", "--remote", remote, "--bookmark", bookmark])?;
        Ok(())
    }

    /// List git remotes using jj git remote list
    pub fn list_remotes(&self) -> Result<Vec<String>> {
        let output = self.run_captured(&["git", "remote", "list"])?;
        Ok(output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect())
    }

    /// Check if a bookmark exists on a remote
    /// This checks if `bookmark@remote` resolves to a commit
    pub fn remote_bookmark_exists(&self, bookmark: &str, remote: &str) -> Result<bool> {
        trace!(
            "Checking if bookmark '{}' exists on remote '{}'",
            bookmark, remote
        );
        let revset = format!("{}@{}", bookmark, remote);
        match self.resolve_revision(&revset) {
            Ok(_) => {
                trace!("Bookmark '{}@{}' exists", bookmark, remote);
                Ok(true)
            }
            Err(Error::JjCommand { .. }) => {
                trace!("Bookmark '{}@{}' does not exist", bookmark, remote);
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Get all tracked bookmarks for the current user
    ///
    /// A bookmark is "tracked" if:
    /// 1. It was authored by the current user (mine() revset)
    /// 2. It is a local bookmark (not a remote-tracking bookmark)
    /// 3. It has been pushed to the remote
    pub fn get_tracked_bookmarks(&self, remote: &str) -> Result<Vec<String>> {
        debug!("Getting tracked bookmarks for remote: {}", remote);

        // Use mine() & bookmarks() to get user's local bookmarks
        debug!("Running jj log to get mine() & bookmarks()");
        let output = self.run_captured(&[
            "log",
            "-r",
            "mine() & bookmarks()",
            "--no-graph",
            "--template",
            r#"bookmarks.map(|b| b ++ "\n").join("")"#,
        ])?;
        debug!("Got bookmarks output, processing lines");

        let mut tracked = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Strip trailing * if present (indicates tracking conflict/divergence)
            let bookmark_name = line.strip_suffix('*').unwrap_or(line);

            // Skip remote-tracking bookmarks (those with @remote suffix)
            if bookmark_name.contains('@') {
                continue;
            }

            // Check if the bookmark exists on the remote
            debug!("Checking if bookmark '{}' exists on remote", bookmark_name);
            if self.remote_bookmark_exists(bookmark_name, remote)? {
                debug!(
                    "Bookmark '{}' exists on remote, adding to tracked list",
                    bookmark_name
                );
                tracked.push(bookmark_name.to_string());
            } else {
                debug!(
                    "Bookmark '{}' does not exist on remote, skipping",
                    bookmark_name
                );
            }
        }

        debug!("Found {} tracked bookmarks", tracked.len());
        Ok(tracked)
    }
}

/// Find the jj binary in PATH or JJ environment variable
pub fn which_jj() -> Result<PathBuf> {
    if let Ok(jj_path) = std::env::var("JJ") {
        return Ok(PathBuf::from(jj_path));
    }

    which::which("jj").map_err(|e| Error::Config {
        message: format!("jj binary not found in PATH: {}", e),
    })
}

/// Run a jj command and return the output
pub fn run_jj_command(repo_path: &PathBuf, args: &[&str]) -> Result<String> {
    let jj_bin = which_jj()?;
    let output = Command::new(&jj_bin)
        .current_dir(repo_path)
        .args(args)
        .output()
        .context(crate::error::IoSnafu)?;

    if !output.status.success() {
        return Err(Error::JjCommand {
            message: format!(
                "jj {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ),
            output: Some(output),
        });
    }

    let stdout = String::from_utf8(output.stdout).context(crate::error::Utf8Snafu)?;
    Ok(stdout)
}

#[derive(Debug, Clone)]
pub struct Bookmark {
    pub name: String,

    /// Git commit ID (40 hex characters)
    pub commit_id: String,

    /// Jujutsu change ID (32 lowercase letters, custom encoding)
    pub change_id: String,

    /// Some(remote_name) if this is a remote-tracking bookmark
    pub remote: Option<String>,

    /// true if this is a local bookmark (not remote@name)
    pub is_local: bool,

    /// true if local bookmark has a remote counterpart
    pub has_remote: bool,
}

#[derive(Debug, Clone)]
pub struct Change {
    /// Git commit ID (40 hex characters)
    pub commit_id: String,
    /// Jujutsu change ID (32 lowercase letters, custom encoding)
    pub change_id: String,
    pub description_first_line: String,
    pub parent_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    /// Create a temporary jj repository for testing
    fn create_test_repo() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize jj repo
        let output = StdCommand::new(which_jj().expect("jj not found"))
            .current_dir(&repo_path)
            .args(["git", "init", "--colocate"])
            .output()
            .expect("Failed to init jj repo");

        assert!(
            output.status.success(),
            "Failed to initialize jj repo: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Create an initial commit
        std::fs::write(repo_path.join("README.md"), "# Test repo\n")
            .expect("Failed to write README");

        let output = StdCommand::new(which_jj().expect("jj not found"))
            .current_dir(&repo_path)
            .args(["describe", "-m", "Initial commit"])
            .output()
            .expect("Failed to create initial commit");

        assert!(
            output.status.success(),
            "Failed to create initial commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        (temp_dir, repo_path)
    }

    #[test]
    fn test_which_jj() {
        let jj_path = which_jj().expect("jj binary must be available in PATH");
        assert!(jj_path.exists());
    }

    #[test]
    fn test_jujutsu_new() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");
        assert!(jj.repo_path.exists());
    }

    #[test]
    fn test_resolve_revision() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");

        // @ should always exist in a jj repo
        let commit_id = jj
            .resolve_revision("@")
            .expect("Failed to resolve @ revision");
        assert!(!commit_id.is_empty());
        // Commit IDs are hex strings
        assert!(commit_id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_get_change_id() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");

        let commit_id = jj.resolve_revision("@").expect("Failed to resolve @");
        let change_id = jj
            .get_change_id(&commit_id)
            .expect("Failed to get change ID");

        assert!(!change_id.is_empty());
        // Change IDs use jj's custom encoding (32 lowercase letters)
        assert_eq!(change_id.len(), 32, "Change ID should be 32 characters");
        assert!(
            change_id.chars().all(|c| c.is_ascii_lowercase()),
            "Change ID should be lowercase letters"
        );
    }

    #[test]
    fn test_get_bookmarks() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path.clone()).expect("Failed to create Jujutsu instance");

        // Create a bookmark
        let output = StdCommand::new(which_jj().expect("jj not found"))
            .current_dir(&repo_path)
            .args(["bookmark", "create", "test-feature"])
            .output()
            .expect("Failed to create bookmark");

        assert!(
            output.status.success(),
            "Failed to create bookmark: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Get bookmarks
        let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");

        // Should have at least our test bookmark
        assert!(bookmarks.iter().any(|b| b.name == "test-feature"));
    }

    #[test]
    fn test_get_default_branch_no_remote() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");

        // No remote configured, should return error
        let result = jj.get_default_branch();
        assert!(result.is_err());

        assert!(
            matches!(result, Err(Error::Config { .. })),
            "Expected Config error, got {:?}",
            result
        );

        if let Err(Error::Config { message }) = result {
            assert!(message.contains("Could not find default branch"));
        }
    }

    #[test]
    fn test_bookmark_parsing() {
        // Test parsing bookmark output format
        let bookmark = Bookmark {
            name: "feature".to_string(),
            commit_id: "abc123".to_string(),
            change_id: "xyz789".to_string(),
            remote: None,
            is_local: true,
            has_remote: false,
        };

        assert_eq!(bookmark.name, "feature");
        assert!(bookmark.is_local);
        assert!(!bookmark.has_remote);
    }

    #[test]
    fn test_change_structure() {
        let change = Change {
            commit_id: "abc123".to_string(),
            change_id: "xyz789".to_string(),
            description_first_line: "Add feature".to_string(),
            parent_ids: vec!["parent1".to_string()],
        };

        assert_eq!(change.description_first_line, "Add feature");
        assert_eq!(change.parent_ids.len(), 1);
    }

    #[test]
    fn test_get_changes() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path.clone()).expect("Failed to create Jujutsu instance");

        // Get the initial commit ID (verify it works)
        let _initial_commit = jj.resolve_revision("@").expect("Failed to resolve @");

        // Create a second commit
        std::fs::write(repo_path.join("test.txt"), "test content\n")
            .expect("Failed to write test file");

        run_jj_command(&repo_path, &["describe", "-m", "Second commit"])
            .expect("Failed to describe commit");

        // Get changes between root and current
        let changes = jj
            .get_changes("root()", "@")
            .expect("Failed to get changes");

        // Should have at least 2 changes (initial + second)
        assert!(
            changes.len() >= 2,
            "Expected at least 2 changes, got {}",
            changes.len()
        );

        // Check that commit IDs are 40 hex characters
        for change in &changes {
            assert_eq!(
                change.commit_id.len(),
                40,
                "Commit ID should be 40 characters"
            );
            assert!(
                change.commit_id.chars().all(|c| c.is_ascii_hexdigit()),
                "Commit ID should be hex"
            );
        }

        // Check that change IDs are 32 lowercase characters
        for change in &changes {
            assert_eq!(
                change.change_id.len(),
                32,
                "Change ID should be 32 characters"
            );
            assert!(
                change.change_id.chars().all(|c| c.is_ascii_lowercase()),
                "Change ID should be lowercase"
            );
        }

        // Find the change with "Second commit" description
        let second_commit = changes
            .iter()
            .find(|c| c.description_first_line == "Second commit");
        assert!(
            second_commit.is_some(),
            "Should have a commit with 'Second commit' description. Found commits: {:?}",
            changes
                .iter()
                .map(|c| &c.description_first_line)
                .collect::<Vec<_>>()
        );
    }

    /// Test: Bookmark parsing should strip asterisk suffix from diverged bookmarks
    ///
    /// Problem: When a bookmark has diverged (local and remote point to different commits),
    /// jj displays it with a trailing asterisk (e.g., "bookmark-a*"). The bookmark parser
    /// was including this asterisk in the bookmark name, causing BookmarkNotFound errors
    /// when trying to submit the bookmark by its actual name.
    ///
    /// This test directly tests the parsing logic with sample input that includes asterisks.
    #[test]
    fn test_bookmark_parsing_strips_asterisk() {
        let (_temp_dir, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");

        // Create a bookmark
        jj.run_captured(&["bookmark", "create", "test-bookmark"])
            .expect("Failed to create bookmark");

        // Manually create output that mimics what jj log would return with an asterisk
        // This simulates a diverged bookmark scenario
        let sample_output_with_asterisk = "test-bookmark*\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n";

        // Parse the output using the same logic as get_bookmarks()
        let mut bookmarks = Vec::new();
        for line in sample_output_with_asterisk.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                // This is the parsing logic from get_bookmarks() that should strip the asterisk
                let full_name = parts[0];

                // Strip trailing * if present (indicates tracking conflict/divergence)
                let full_name = full_name.strip_suffix('*').unwrap_or(full_name);

                let (name, remote) = if let Some(at_pos) = full_name.rfind('@') {
                    let name = full_name[..at_pos].to_string();
                    let remote = full_name[at_pos + 1..].to_string();
                    (name, Some(remote))
                } else {
                    (full_name.to_string(), None)
                };

                let is_local = remote.is_none();
                bookmarks.push(Bookmark {
                    name,
                    commit_id: parts[1].to_string(),
                    change_id: parts[2].to_string(),
                    remote,
                    is_local,
                    has_remote: false,
                });
            }
        }

        // The bug: bookmark name includes the asterisk
        assert_eq!(bookmarks.len(), 1, "Should parse one bookmark");
        assert_eq!(
            bookmarks[0].name, "test-bookmark",
            "Bookmark name should NOT include asterisk, found: '{}'",
            bookmarks[0].name
        );
    }

    #[test]
    fn test_get_tracked_bookmarks_empty() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path).expect("Failed to create Jujutsu instance");

        // No bookmarks created yet, should return empty list
        let tracked = jj
            .get_tracked_bookmarks("origin")
            .expect("Failed to get tracked bookmarks");

        assert_eq!(tracked.len(), 0, "Should have no tracked bookmarks");
    }

    #[test]
    fn test_get_tracked_bookmarks_filters_unpushed() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path.clone()).expect("Failed to create Jujutsu instance");

        // Create a local bookmark
        run_jj_command(&repo_path, &["bookmark", "create", "local-only"])
            .expect("Failed to create bookmark");

        // The bookmark exists locally but hasn't been pushed to any remote
        // so it should not appear in tracked bookmarks
        let tracked = jj
            .get_tracked_bookmarks("origin")
            .expect("Failed to get tracked bookmarks");

        assert_eq!(
            tracked.len(),
            0,
            "Should have no tracked bookmarks (local bookmark not pushed)"
        );
    }

    #[test]
    fn test_get_tracked_bookmarks_returns_pushed() {
        let (_temp, repo_path) = create_test_repo();
        let jj = Jujutsu::new(repo_path.clone()).expect("Failed to create Jujutsu instance");

        // Create a bookmark
        run_jj_command(&repo_path, &["bookmark", "create", "feature-a"])
            .expect("Failed to create bookmark");

        // Set up a bare git repo to act as a remote
        let remote_dir = _temp.path().join("remote.git");
        std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");

        StdCommand::new("git")
            .current_dir(&remote_dir)
            .args(["init", "--bare"])
            .output()
            .expect("Failed to init bare git repo");

        // Add the remote
        run_jj_command(
            &repo_path,
            &[
                "git",
                "remote",
                "add",
                "origin",
                remote_dir.to_str().unwrap(),
            ],
        )
        .expect("Failed to add remote");

        // Push the bookmark
        jj.push_bookmark("feature-a", "origin")
            .expect("Failed to push bookmark");

        // Now the bookmark should appear in tracked bookmarks
        let tracked = jj
            .get_tracked_bookmarks("origin")
            .expect("Failed to get tracked bookmarks");

        assert_eq!(tracked.len(), 1, "Should have 1 tracked bookmark");
        assert_eq!(tracked[0], "feature-a", "Should track feature-a");
    }
}
