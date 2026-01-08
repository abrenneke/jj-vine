/// Common utilities for e2e integration tests
use jj_mrs::gitlab::GitLabClient;
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

        // Combine stdout and stderr to capture all output
        let mut full_output = String::from_utf8(output.stdout)?;
        if !output.stderr.is_empty() {
            full_output.push_str("\n--- STDERR ---\n");
            full_output.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        Ok(full_output)
    }

    /// Run jj-mrs command and expect it to fail
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn commit(&self, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.jj(&["commit", "-m", message])?;
        Ok(())
    }

    /// Initialize jj-mrs configuration
    #[allow(dead_code)]
    pub fn init_mrs_config(
        &self,
        gitlab_host: &str,
        gitlab_project: &str,
        gitlab_token: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.jj(&["config", "set", "--repo", "jj-mrs.gitlabHost", gitlab_host])?;
        self.jj(&[
            "config",
            "set",
            "--repo",
            "jj-mrs.gitlabProject",
            gitlab_project,
        ])?;
        self.jj(&[
            "config",
            "set",
            "--repo",
            "jj-mrs.gitlabToken",
            gitlab_token,
        ])?;
        Ok(())
    }

    /// Add a git remote to the repository
    #[allow(dead_code)]
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

/// Generate a unique test branch name to avoid conflicts between test runs
///
/// Format: `jjmrs-test-{uuid}-{name}`
pub fn unique_test_branch(name: &str) -> String {
    format!("jjmrs-test-{}-{}", uuid::Uuid::new_v4(), name)
}

/// GitLab test helper for integration tests
///
/// Provides utilities for creating GitLab clients and working with test MRs.
/// Uses unique branch names to avoid conflicts between test runs.
pub struct GitLabTestHelper {
    /// GitLab API client
    pub client: GitLabClient,
    /// GitLab project path
    pub project: String,
}

impl GitLabTestHelper {
    /// Create a GitLabTestHelper from environment variables
    ///
    /// Returns None if environment variables are not set.
    /// Prints a message to stderr explaining why the test is being skipped.
    pub async fn from_env() -> Option<Self> {
        let config = match GitLabConfig::from_env() {
            Some(c) => c,
            None => {
                eprintln!(
                    "Skipping GitLab integration test: GITLAB_HOST, GITLAB_PROJECT, and GITLAB_TOKEN must be set"
                );
                return None;
            }
        };

        // Read optional TLS configuration
        let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
        let accept_non_compliant_certs = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let client = match GitLabClient::new(
            config.host.clone(),
            config.project.clone(),
            config.token.clone(),
            ca_bundle,
            accept_non_compliant_certs,
        ) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "Skipping GitLab integration test: Failed to create GitLab client: {}",
                    e
                );
                return None;
            }
        };

        Some(Self {
            client,
            project: config.project,
        })
    }
}

impl TestRepo {
    /// Create a new test repository with GitLab remote configured
    ///
    /// This sets up:
    /// - A new jj repository with git colocate
    /// - jj-mrs configuration with GitLab credentials
    /// - A git remote pointing to the GitLab project
    pub fn with_gitlab_remote(config: &GitLabConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let repo = Self::new()?;

        // Initialize jj-mrs configuration
        repo.init_mrs_config(&config.host, &config.project, &config.token)?;

        // Construct SSH remote URL from host and project
        // Extract hostname from GITLAB_HOST (e.g., "https://gitlab.internal.valence.nl/" -> "gitlab.internal.valence.nl")
        let hostname = config
            .host
            .trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        let remote_url = format!("git@{}:{}.git", hostname, config.project);

        // Add git remote
        repo.add_git_remote("origin", &remote_url)?;

        Ok(repo)
    }

    /// Get the git remote URL for a given remote name
    pub fn get_remote_url(&self, remote: &str) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .args(["remote", "get-url", remote])
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            return Err(format!(
                "Failed to get remote URL: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }
}

/// All-in-one test wrapper for GitLab integration tests
///
/// Combines TestRepo and GitLabTestHelper for convenient test setup.
/// Creates a repository with GitLab remote and API client ready to use.
pub struct GitLabTest {
    /// Test repository with GitLab remote configured
    pub repo: TestRepo,
    /// GitLab API helper
    pub gitlab: GitLabTestHelper,
}

impl GitLabTest {
    /// Set up a complete GitLab integration test environment
    ///
    /// Returns None if GitLab environment variables are not configured.
    /// This allows tests to be skipped gracefully when not configured.
    pub async fn setup() -> Option<Self> {
        let config = GitLabConfig::from_env()?;
        let gitlab = GitLabTestHelper::from_env().await?;

        let repo = match TestRepo::with_gitlab_remote(&config) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "Skipping GitLab integration test: Failed to create test repo: {}",
                    e
                );
                return None;
            }
        };

        Some(Self { repo, gitlab })
    }
}
