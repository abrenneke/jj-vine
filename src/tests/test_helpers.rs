#![cfg(test)]
#![allow(dead_code)]

use std::{path::PathBuf, process::Command};

use tempfile::TempDir;

#[cfg(not(feature = "no-e2e-tests"))]
use crate::forge::{forgejo::ForgejoForge, github::GitHubForge, gitlab::GitLabForge};
use crate::{
    cli::CliConfig,
    commands::submit::SubmitCommandConfig,
    error::Result,
    forge::Forge,
    jj::Jujutsu,
    output::BufferedOutput,
};

/// Generate a unique test branch name to avoid conflicts between test runs
pub fn unique_branch(name: &str) -> String {
    format!("jjmrs-test-{}-{}", uuid::Uuid::new_v4(), name)
}

/// A test repository with jj initialized
pub struct TestRepo<T> {
    /// Temporary directory containing the repository
    pub dir: TempDir,

    /// Path to the repository
    pub path: PathBuf,

    /// Forge API client
    forge: T,
}

fn make_repo() -> (TempDir, PathBuf) {
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

    (dir, path)
}

impl TestRepo<()> {
    /// Create a new test repository with jj initialized, but no forge client
    pub fn new() -> Self {
        let (dir, path) = make_repo();
        Self {
            dir,
            path,
            forge: Default::default(),
        }
    }

    /// Create a bookmark at current revision, chainable.
    /// If a remote is configured, also tracks the bookmark.
    pub fn create_bookmark(&self, name: &str) -> &Self {
        self.jj(["bookmark", "create", name]);
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
}

#[cfg(not(feature = "no-e2e-tests"))]
impl TestRepo<GitLabForge> {
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

        let (dir, path) = make_repo();

        let repo = Self {
            dir,
            path,
            forge: GitLabForge::new(
                &host,
                &project,
                &project, // For tests, source and target are the same (direct mode)
                &token,
                ca_bundle.as_ref(),
                accept_non_compliant,
            )
            .expect("Failed to create GitLab client"),
        };

        repo.jj(["config", "set", "--repo", "jj-vine.forge", "gitlab"]);
        repo.jj(["config", "set", "--repo", "jj-vine.gitlab.host", &host]);
        repo.jj([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            &project,
        ]);
        repo.jj(["config", "set", "--repo", "jj-vine.gitlab.token", &token]);

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

        // Fetch and track main so tests don't have to
        repo.jj(["git", "fetch"]);
        repo.jj(["bookmark", "track", "main@origin"]);

        repo
    }
}

#[cfg(not(feature = "no-e2e-tests"))]
impl TestRepo<GitHubForge> {
    pub fn with_github_remote() -> Self {
        dotenv::dotenv().ok();

        let host =
            std::env::var("GITHUB_HOST").unwrap_or_else(|_| "https://api.github.com".to_string());
        let project = std::env::var("GITHUB_PROJECT").expect("GITHUB_PROJECT required");
        let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN required");
        let ca_bundle = std::env::var("GITHUB_CA_BUNDLE").ok();
        let accept_non_compliant = std::env::var("GITHUB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let (dir, path) = make_repo();

        let repo = Self {
            dir,
            path,
            forge: GitHubForge::new(
                &host,
                &project,
                &project, // For tests, source and target are the same (direct mode)
                &token,
                ca_bundle.as_deref(),
                accept_non_compliant,
            )
            .expect("Failed to create GitHub client"),
        };

        repo.jj(["config", "set", "--repo", "jj-vine.forge", "github"]);
        repo.jj(["config", "set", "--repo", "jj-vine.github.host", &host]);
        repo.jj([
            "config",
            "set",
            "--repo",
            "jj-vine.github.project",
            &project,
        ]);
        repo.jj(["config", "set", "--repo", "jj-vine.github.token", &token]);

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
        // Convert API URL to git remote host: api.github.com -> github.com
        let hostname = host
            .trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .replace("api.github.com", "github.com")
            .trim_end_matches("/api/v3")
            .to_string();
        let remote_url = format!("git@{}:{}.git", hostname, project);

        repo.jj(["git", "remote", "add", "origin", &remote_url]);

        // Fetch and track main so tests don't have to
        repo.jj(["git", "fetch"]);
        repo.jj(["bookmark", "track", "main@origin"]);

        repo
    }
}

#[cfg(not(feature = "no-e2e-tests"))]
impl TestRepo<ForgejoForge> {
    pub fn with_forgejo_remote() -> Self {
        dotenv::dotenv().ok();

        let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
        let project = std::env::var("FORGEJO_PROJECT").expect("FORGEJO_PROJECT required");
        let token = std::env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN required");
        let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
        let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let (dir, path) = make_repo();

        let repo = Self {
            dir,
            path,
            forge: ForgejoForge::new(
                &host,
                &project,
                &project, // For tests, source and target are the same (direct mode)
                &token,
                ca_bundle.as_deref(),
                accept_non_compliant,
            )
            .expect("Failed to create Forgejo client"),
        };

        repo.jj(["config", "set", "--repo", "jj-vine.forge", "forgejo"]);
        repo.jj(["config", "set", "--repo", "jj-vine.forgejo.host", &host]);
        repo.jj([
            "config",
            "set",
            "--repo",
            "jj-vine.forgejo.project",
            &project,
        ]);
        repo.jj(["config", "set", "--repo", "jj-vine.forgejo.token", &token]);

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

        let hostname_no_port = hostname.split(":").next().unwrap_or(hostname);

        let port = if host.contains("localhost") { 222 } else { 22 };

        let remote_url = format!("ssh://git@{}:{}/{}.git", hostname_no_port, port, project);

        repo.jj(["git", "remote", "add", "origin", &remote_url]);

        // Fetch and track main so tests don't have to
        repo.jj(["git", "fetch"]);
        repo.jj(["bookmark", "track", "main@origin"]);

        repo
    }
}

impl<T> TestRepo<T>
where
    T: Forge,
{
    pub fn forge(&self) -> &T {
        &self.forge
    }

    /// Create a bookmark at current revision, chainable.
    /// If a remote is configured, also tracks the bookmark.
    pub fn create_bookmark(&self, name: &str) -> &Self {
        self.jj(["bookmark", "create", name]);
        self.jj(["bookmark", "track", &format!("{}@origin", name)]);
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
}

impl<T> TestRepo<T> {
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

    /// Push a bookmark to origin, chainable
    pub fn push_bookmark(&self, name: &str) -> &Self {
        self.jj(["git", "push", "--bookmark", name]);
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
}

impl Default for TestRepo<()> {
    fn default() -> Self {
        Self::new()
    }
}
