#![cfg(test)]

use std::{path::PathBuf, process::Command};

use tempfile::TempDir;

use crate::gitlab::GitLabClient;

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

        repo.jj(["config", "set", "--repo", "jj-mrs.gitlabHost", &host]);
        repo.jj(["config", "set", "--repo", "jj-mrs.gitlabProject", &project]);
        repo.jj(["config", "set", "--repo", "jj-mrs.gitlabToken", &token]);

        if let Some(ref bundle) = ca_bundle {
            repo.jj(["config", "set", "--repo", "jj-mrs.caBundle", bundle]);
        }

        if accept_non_compliant {
            repo.jj([
                "config",
                "set",
                "--repo",
                "jj-mrs.tlsAcceptNonCompliantCerts",
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
        repo
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

    /// Create a bookmark at current revision, chainable
    pub fn create_bookmark(&self, name: &str) -> &Self {
        self.jj(&["bookmark", "create", name]);
        self
    }

    /// Submit bookmarks via the library function
    pub async fn submit(&self, bookmarks: impl AsRef<[&str]>) {
        crate::commands::submit::submit(
            self.path.clone(),
            bookmarks.as_ref().iter().map(|s| s.to_string()).collect(),
            "origin".to_string(),
            false,
            false,
        )
        .await
        .unwrap()
    }

    /// Submit bookmarks with options
    pub async fn submit_with_options(
        &self,
        bookmarks: impl Iterator<Item = &str>,
        dry_run: bool,
        verbose: bool,
    ) {
        crate::commands::submit::submit(
            self.path.clone(),
            bookmarks.into_iter().map(|s| s.to_string()).collect(),
            "origin".to_string(),
            dry_run,
            verbose,
        )
        .await
        .unwrap()
    }
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}
