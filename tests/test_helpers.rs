/// Common utilities for e2e integration tests
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Test repository setup
pub struct TestRepo {
    /// Temporary directory containing the test repository
    #[allow(dead_code)]
    pub dir: TempDir,
    /// Path to the repository
    pub path: PathBuf,
}

impl TestRepo {
    /// Create a new test repository with jj initialized
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let path = dir.path().to_path_buf();

        // Initialize jj repository with git colocate
        let output = Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(&path)
            .output()?;

        if !output.status.success() {
            return Err(format!(
                "Failed to init jj repo: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(Self { dir, path })
    }

    /// Run a jj command in this repository
    pub fn jj(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("jj")
            .args(args)
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            return Err(format!(
                "jj command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Run jj-mrs command in this repository
    #[allow(dead_code)]
    pub fn jj_mrs(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("jj")
            .arg("mr")
            .args(args)
            .current_dir(&self.path)
            .output()?;

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Run jj-mrs command and expect it to fail
    pub fn jj_mrs_expect_error(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("jj")
            .arg("mr")
            .args(args)
            .current_dir(&self.path)
            .output()?;

        if output.status.success() {
            return Err("Expected jj-mrs to fail but it succeeded".into());
        }

        Ok(String::from_utf8(output.stderr)?)
    }

    /// Create a file in the repository
    pub fn create_file(&self, name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = self.path.join(name);
        std::fs::write(file_path, content)?;
        Ok(())
    }

    /// Create a bookmark at current revision
    pub fn create_bookmark(&self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.jj(&["bookmark", "create", name])?;
        Ok(())
    }

    /// Commit current changes
    pub fn commit(&self, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.jj(&["commit", "-m", message])?;
        Ok(())
    }

    /// Initialize jj-mrs configuration
    pub fn init_mrs_config(
        &self,
        gitlab_host: &str,
        gitlab_project: &str,
        gitlab_token: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.jj(&["config", "set", "--repo", "spr.gitlabHost", gitlab_host])?;
        self.jj(&[
            "config",
            "set",
            "--repo",
            "spr.gitlabProject",
            gitlab_project,
        ])?;
        self.jj(&["config", "set", "--repo", "spr.gitlabToken", gitlab_token])?;
        Ok(())
    }

    /// Add a git remote to the repository
    pub fn add_git_remote(&self, name: &str, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .args(["remote", "add", name, url])
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            return Err(format!(
                "Failed to add git remote: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(())
    }
}

/// GitLab configuration for integration tests
pub struct GitLabConfig {
    /// GitLab instance URL
    pub host: String,
    /// GitLab project path (e.g., "username/repo")
    pub project: String,
    /// GitLab personal access token
    pub token: String,
}

impl GitLabConfig {
    /// Load GitLab configuration from environment variables
    ///
    /// First attempts to load from a .env file in the current directory,
    /// then falls back to system environment variables.
    ///
    /// Requires the following environment variables:
    /// - GITLAB_HOST: GitLab instance URL (e.g., "https://gitlab.com")
    /// - GITLAB_PROJECT: Project path (e.g., "username/test-repo")
    /// - GITLAB_TOKEN: Personal access token with API access
    pub fn from_env() -> Option<Self> {
        dotenv::dotenv().ok();

        let host = std::env::var("GITLAB_HOST").ok()?;
        let project = std::env::var("GITLAB_PROJECT").ok()?;
        let token = std::env::var("GITLAB_TOKEN").ok()?;

        Some(Self {
            host,
            project,
            token,
        })
    }
}
