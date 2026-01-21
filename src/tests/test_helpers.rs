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

    pub jj: Jujutsu,
}

fn make_repo() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    let output = Command::new("jj")
        .args(["git", "init"])
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
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
            path,
            forge: Default::default(),
        }
    }

    /// Create a bookmark at current revision, chainable.
    /// If a remote is configured, also tracks the bookmark.
    pub fn create_bookmark(&self, name: &str) -> &Self {
        self.jj.exec(["bookmark", "create", name]).unwrap();
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
        self.jj.exec(["new"]).unwrap();
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
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
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

        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.gitlab.host", &host])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                &project,
            ])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.gitlab.token", &token])
            .unwrap();

        if let Some(ref bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", bundle])
                .unwrap();
        }

        if accept_non_compliant {
            repo.jj
                .exec([
                    "config",
                    "set",
                    "--repo",
                    "jj-vine.tlsAcceptNonCompliantCerts",
                    "true",
                ])
                .unwrap();
        }

        // Add git remote
        let hostname = host
            .trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let remote_url = format!("git@{}:{}.git", hostname, project);

        repo.jj
            .exec(["git", "remote", "add", "origin", &remote_url])
            .unwrap();

        // Fetch and track main so tests don't have to
        repo.jj.exec(["git", "fetch"]).unwrap();
        repo.jj.exec(["bookmark", "track", "main@origin"]).unwrap();

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
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
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

        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forge", "github"])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.github.host", &host])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.github.project",
                &project,
            ])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.github.token", &token])
            .unwrap();

        if let Some(ref bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", bundle])
                .unwrap();
        }

        if accept_non_compliant {
            repo.jj
                .exec([
                    "config",
                    "set",
                    "--repo",
                    "jj-vine.tlsAcceptNonCompliantCerts",
                    "true",
                ])
                .unwrap();
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

        repo.jj
            .exec(["git", "remote", "add", "origin", &remote_url])
            .unwrap();

        // Fetch and track main so tests don't have to
        repo.jj.exec(["git", "fetch"]).unwrap();
        repo.jj.exec(["bookmark", "track", "main@origin"]).unwrap();

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
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
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

        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forge", "forgejo"])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forgejo.host", &host])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.forgejo.project",
                &project,
            ])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forgejo.token", &token])
            .unwrap();

        if let Some(ref bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", bundle])
                .unwrap();
        }

        if accept_non_compliant {
            repo.jj
                .exec([
                    "config",
                    "set",
                    "--repo",
                    "jj-vine.tlsAcceptNonCompliantCerts",
                    "true",
                ])
                .unwrap();
        }

        // Add git remote
        let hostname = host
            .trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        let hostname_no_port = hostname.split(":").next().unwrap_or(hostname);

        let port = if host.contains("localhost") { 222 } else { 22 };

        let remote_url = format!("ssh://git@{}:{}/{}.git", hostname_no_port, port, project);

        repo.jj
            .exec(["git", "remote", "add", "origin", &remote_url])
            .unwrap();

        // Fetch and track main so tests don't have to
        repo.jj.exec(["git", "fetch"]).unwrap();
        repo.jj.exec(["bookmark", "track", "main@origin"]).unwrap();

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
        self.jj.exec(["bookmark", "create", name]).unwrap();
        self.jj
            .exec(["bookmark", "track", &format!("{}@origin", name)])
            .unwrap();
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
        self.jj.exec(["new"]).unwrap();
        self
    }
}

impl<T> TestRepo<T> {
    /// Create a file and describe the current change, chainable
    pub fn create_change(&self, file: &str, content: &str, msg: &str) -> &Self {
        std::fs::write(self.path.join(file), content).unwrap();
        self.jj.exec(["describe", "-m", msg]).unwrap();
        self
    }

    /// Push a bookmark to origin, chainable
    pub fn push_bookmark(&self, name: &str) -> &Self {
        self.jj.exec(["git", "push", "--bookmark", name]).unwrap();
        self
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
