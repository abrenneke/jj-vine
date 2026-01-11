#![cfg(test)]

use std::{path::PathBuf, process::Command};

use tempfile::TempDir;

use crate::{
    cli::CliConfig,
    commands::submit::SubmitCommandConfig,
    error::Result,
    gitlab::GitLabClient,
    jj::Jujutsu,
    output::BufferedOutput,
};

/// Generate a unique test branch name to avoid conflicts between test runs
pub fn unique_branch(name: &str) -> String {
    format!("jjmrs-test-{}-{}", uuid::Uuid::new_v4(), name)
}

/// A test repository with jj initialized
pub struct TestRepo {
    /// Temporary directory containing the repository
    pub dir: TempDir,

    /// Path to the repository
    pub path: PathBuf,

    /// GitLab API client
    client: Option<GitLabClient>,
}

impl TestRepo {
    /// Create a new test repository with jj initialized
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let output = Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Self {
            dir,
            path,
            client: None,
        }
    }

    pub fn with_gitlab_remote() -> Self {
        dotenv::dotenv().ok();

        let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
        let project = std::env::var("GITLAB_PROJECT").expect("GITLAB_PROJECT required");
        let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN required");
        let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
        let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let mut repo = Self::new();

        repo.jj(["config", "set", "--repo", "jj-vine.gitlabHost", &host]);
        repo.jj(["config", "set", "--repo", "jj-vine.gitlabProject", &project]);
        repo.jj(["config", "set", "--repo", "jj-vine.gitlabToken", &token]);

        if let Some(ref bundle) = ca_bundle {
            repo.jj(["config", "set", "--repo", "jj-vine.caBundle", bundle]);
        }

        if accept_non_compliant {
            repo.jj([
                "config",
                "set",
                "--repo",
                "jj-vine.tlsAcceptNonCompliantCerts",
                "true",
            ]);
        }

        // Add git remote
        let hostname = host
            .trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let remote_url = format!("git@{}:{}.git", hostname, project);

        repo.jj(["git", "remote", "add", "origin", &remote_url]);

        let client = GitLabClient::new(host, project, token, ca_bundle, accept_non_compliant)
            .expect("Failed to create GitLab client");

        repo.client = Some(client);

        // Fetch and track main so tests don't have to
        repo.jj(["git", "fetch"]);
        repo.jj(["bookmark", "track", "main@origin"]);

        repo
    }

    pub fn gitlab(&self) -> &GitLabClient {
        self.client
            .as_ref()
            .expect("GitLab client not initialized, use with_gitlab_remote() instead of new()")
    }

    pub fn jj<'a>(&self, args: impl AsRef<[&'a str]>) -> String {
        let output = Command::new("jj")
            .args(args.as_ref())
            .current_dir(&self.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// Create a file and describe the current change, chainable
    pub fn create_change(&self, file: &str, content: &str, msg: &str) -> &Self {
        std::fs::write(self.path.join(file), content).unwrap();
        self.jj(["describe", "-m", msg]);
        self
    }

    /// Create a bookmark at current revision, chainable.
    /// If a remote is configured, also tracks the bookmark.
    pub fn create_bookmark(&self, name: &str) -> &Self {
        self.jj(["bookmark", "create", name]);
        if self.client.is_some() {
            self.jj(["bookmark", "track", &format!("{}@origin", name)]);
        }
        self
    }

    /// Push a bookmark to origin, chainable
    pub fn push_bookmark(&self, name: &str) -> &Self {
        self.jj(["git", "push", "--bookmark", name]);
        self
    }

    /// Create a bookmark and push it to origin, chainable
    pub fn create_and_push_bookmark(&self, name: &str) -> &Self {
        self.create_bookmark(name);
        self.push_bookmark(name)
    }

    /// Create a commit with a bookmark, then start new working copy
    pub fn commit_with_bookmark(
        &self,
        file: &str,
        content: &str,
        msg: &str,
        bookmark: &str,
    ) -> &Self {
        self.create_change(file, content, msg);
        self.create_bookmark(bookmark);
        self.jj(["new"]);
        self
    }

    /// Get a Jujutsu instance for this repo
    pub fn jujutsu(&self) -> Jujutsu {
        Jujutsu::new(self.path.clone()).expect("Failed to create Jujutsu instance")
    }

    /// Submit bookmarks with options
    pub async fn submit(&self, config: SubmitCommandConfig) -> String {
        self.try_submit(config).await.unwrap()
    }

    /// Submit bookmarks with options, returning Result for error testing
    pub async fn try_submit(&self, config: SubmitCommandConfig) -> Result<String> {
        let buffered_output = BufferedOutput::new();
        crate::commands::submit::submit(
            config,
            CliConfig {
                repository: self.path.clone(),
                output: &buffered_output,
            },
        )
        .await?;

        Ok(strip_ansi_escapes::strip_str(buffered_output.get_buffer()))
    }

    /// jj mr submit --bookmark <bookmark>
    pub async fn submit_bookmark(&self, bookmark: String) -> String {
        self.submit(SubmitCommandConfig {
            bookmark: Some(bookmark),
            ..Default::default()
        })
        .await
    }

    /// jj mr submit --tracked
    pub async fn submit_tracked(&self) -> String {
        self.submit(SubmitCommandConfig {
            tracked: true,
            ..Default::default()
        })
        .await
    }
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}
