#![cfg(test)]
#![allow(dead_code)]

use std::path::PathBuf;

use clap::Parser;
use snafu::ResultExt;
use tempfile::TempDir;

#[cfg(not(feature = "no-e2e-tests"))]
use crate::forge::{
    azure::AzureDevOpsForge,
    forgejo::ForgejoForge,
    github::GitHubForge,
    gitlab::GitLabForge,
};
use crate::{
    bookmark::JJName,
    cli::Cli,
    error::{ClapSnafu, Result},
    forge::Forge,
    jj::Jujutsu,
};

/// A test repository with jj initialized
pub struct TestRepo<T> {
    /// Temporary directory containing the repository
    pub dir: TempDir,

    /// Path to the repository
    pub path: PathBuf,

    /// Forge API client
    forge: T,

    pub jj: Jujutsu,

    pub id: String,
}

impl TestRepo<()> {
    /// Create a new test repository with jj initialized, but no forge client
    pub fn new() -> Self {
        let RepoInfo { dir, path, .. } = Self::make_repo();
        Self {
            dir,
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
            path,
            forge: Default::default(),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn with_main() -> Self {
        let repo = Self::new();
        repo.jj(["new"])
            .unwrap()
            .create_change("main.txt", "main", "Main commit")
            .create_bookmark("main")
            .jj(["new", "main"])
            .unwrap();
        repo
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

        let RepoInfo { dir, path, .. } = Self::make_repo();

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
                true,
            )
            .expect("Failed to create GitLab client"),
            id: uuid::Uuid::new_v4().to_string(),
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

        if let Some(bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", &bundle])
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
        repo.track_bookmark("main");

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

        let RepoInfo { dir, path, .. } = Self::make_repo();

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
            id: uuid::Uuid::new_v4().to_string(),
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

        if let Some(bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", &bundle])
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
        repo.track_bookmark("main");

        repo
    }
}

#[cfg(not(feature = "no-e2e-tests"))]
impl TestRepo<ForgejoForge> {
    pub fn with_forgejo_remote() -> Self {
        use crate::{
            config::{Config, ForgeType, ForgejoConfig},
            forge::ForgeImpl,
        };

        dotenv::dotenv().ok();

        let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
        let project = std::env::var("FORGEJO_PROJECT").expect("FORGEJO_PROJECT required");
        let token = std::env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN required");
        let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
        let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let RepoInfo { dir, path, .. } = Self::make_repo();

        let forge = match ForgeImpl::new(
            &Config::builder()
                .maybe_ca_bundle(ca_bundle.clone())
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .forge(ForgeType::Forgejo)
                .forgejo(ForgejoConfig {
                    host: host.clone(),
                    target_project: project.clone(),
                    project: project.clone(),
                    token: token.clone(),
                    ..Default::default()
                })
                .build(),
        )
        .unwrap()
        {
            ForgeImpl::Forgejo(forge) => forge,
            _ => unreachable!(),
        };

        let repo = Self {
            dir,
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
            path,
            forge,
            id: uuid::Uuid::new_v4().to_string(),
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

        if let Some(bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", &bundle])
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

#[cfg(not(feature = "no-e2e-tests"))]
impl TestRepo<AzureDevOpsForge> {
    pub fn with_azure_remote() -> Self {
        dotenv::dotenv().ok();

        let host = std::env::var("AZURE_HOST").expect("AZURE_HOST required");
        let vssps_host = std::env::var("AZURE_VSSPS_HOST").expect("AZURE_VSSPS_HOST required");
        let ssh_host = std::env::var("AZURE_SSH_HOST").expect("AZURE_SSH_HOST required");
        let project_id = std::env::var("AZURE_PROJECT").expect("AZURE_PROJECT required");
        let repo_name = std::env::var("AZURE_REPO_NAME").expect("AZURE_REPO_NAME required");
        let token = std::env::var("AZURE_TOKEN").expect("AZURE_TOKEN required");
        let ca_bundle = std::env::var("AZURE_CA_BUNDLE").ok();
        let accept_non_compliant = std::env::var("AZURE_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let RepoInfo { dir, path, .. } = Self::make_repo();

        let (org, project) = project_id.split_once('/').unwrap();

        let forge = AzureDevOpsForge::builder()
            .base_url(host.clone())
            .vssps_base_url(vssps_host.clone())
            .source_project_id(project_id.clone())
            .target_project_id(project_id.clone())
            .source_repository_name(repo_name.clone())
            .target_repository_name(repo_name.clone())
            .token(token.clone())
            .maybe_ca_bundle(ca_bundle.clone())
            .accept_non_compliant_certs(accept_non_compliant)
            .build()
            .expect("Failed to create Azure DevOps client");

        let repo = Self {
            dir,
            jj: Jujutsu::new(&path).expect("Failed to create Jujutsu"),
            path,
            forge,
            id: uuid::Uuid::new_v4().to_string(),
        };

        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.forge", "azure"])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.azure.host", &host])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.azure.vsspsHost",
                &vssps_host,
            ])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.azure.project",
                &project_id,
            ])
            .unwrap();
        repo.jj
            .exec(["config", "set", "--repo", "jj-vine.azure.token", &token])
            .unwrap();
        repo.jj
            .exec([
                "config",
                "set",
                "--repo",
                "jj-vine.azure.sourceRepositoryName",
                &repo_name,
            ])
            .unwrap();

        if let Some(bundle) = ca_bundle {
            repo.jj
                .exec(["config", "set", "--repo", "jj-vine.caBundle", &bundle])
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

        let remote_url = format!("git@{}:v3/{}/{}/{}", ssh_host, org, project, repo_name);

        repo.jj
            .exec(["git", "remote", "add", "origin", &remote_url])
            .unwrap();

        repo.jj.exec(["git", "fetch"]).unwrap();
        repo.track_bookmark("main");

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

    pub async fn get_mr(&self, bookmark: impl JJName) -> T::MergeRequest {
        self.forge()
            .find_merge_request_by_source_branch(&bookmark.raw_name())
            .await
            .unwrap_or_else(|_| panic!("Failed to find merge request for {}", bookmark.raw_name()))
            .unwrap_or_else(|| panic!("Merge request should exist for {}", bookmark.raw_name()))
    }

    pub async fn get_mr_with_base(
        &self,
        bookmark: impl JJName,
        base: impl JJName,
    ) -> T::MergeRequest {
        self.forge()
            .find_merge_request_by_source_branch_base_branch(&bookmark.raw_name(), &base.raw_name())
            .await
            .unwrap_or_else(|_| panic!("Failed to find merge request for {}", bookmark.raw_name()))
            .unwrap_or_else(|| panic!("Merge request should exist for {}", bookmark.raw_name()))
    }
}

impl TestRepo<TestRepo<()>> {
    pub fn upstream(&self) -> &TestRepo<()> {
        &self.forge
    }

    pub fn with_local_remote() -> Self {
        let RepoInfo { dir, path, .. } = Self::make_repo();

        let upstream = TestRepo::new();

        let repo = Self {
            dir,
            jj: Jujutsu::new(&path).unwrap(),
            path,
            forge: upstream,
            id: uuid::Uuid::new_v4().to_string(),
        };

        repo.forge
            .create_change("init.txt", "initial", "Initial commit")
            .create_bookmark("main");

        repo.jj
            .exec([
                "git",
                "remote",
                "add",
                "origin",
                repo.forge.path.to_str().unwrap(),
            ])
            .unwrap();

        repo.jj.exec(["git", "fetch"]).unwrap();

        repo.track_bookmark("main").jj(["new", "main"]).unwrap();

        repo
    }
}

impl<T> TestRepo<T> {
    /// Gets the unique identifier for this TestRepo.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Gets a bookmark name that is unique to this TestRepo.
    pub fn bookmark_name(&self, bookmark: &str) -> String {
        format!("{}-{}", self.id(), bookmark)
    }

    /// Create a file and describe the current change
    pub fn create_change(&self, file: &str, content: &str, msg: &str) -> &Self {
        std::fs::write(self.path.join(file), content).unwrap();
        self.jj.exec(["describe", "-m", msg]).unwrap();
        self
    }

    pub fn create_bookmark(&self, bookmark: impl JJName) -> &Self {
        self.jj
            .exec(["bookmark", "create", &bookmark.name_for_jj()])
            .unwrap();
        self
    }

    /// Create a commit with a bookmark, then start new working copy
    pub fn create_change_and_bookmark(&self, bookmark: impl JJName) -> &Self {
        self.create_change(
            &format!("{}.txt", bookmark.raw_name().replace("/", "--")),
            &format!("content for {}", bookmark.raw_name()),
            &format!("Commit for {} bookmark", bookmark.raw_name()),
        )
        .create_bookmark(bookmark)
        .jj(["new"])
        .unwrap()
    }

    /// Push a bookmark to origin
    pub fn push_bookmark(&self, bookmark: impl JJName) -> &Self {
        self.jj
            .exec(["git", "push", "--bookmark", &bookmark.name_for_jj()])
            .unwrap();
        self
    }

    /// Run a jj command
    pub fn jj<'a>(&self, args: impl AsRef<[&'a str]>) -> Result<&Self> {
        self.jj.exec(args)?;
        Ok(self)
    }

    /// Run the CLI with the given arguments
    pub async fn run<'a>(&self, args: impl AsRef<[&'a str]>) -> String {
        self.try_run(args).await.unwrap()
    }

    /// Run the CLI with the given arguments, returning Result for error testing
    pub async fn try_run<'a>(&self, args: impl AsRef<[&'a str]>) -> Result<String> {
        let args = args.as_ref();
        let mut cli = Cli::try_parse_from(["jj-vine"].iter().chain(args)).context(ClapSnafu {
            arguments: args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        })?;
        cli.repository = cli.repository.or(Some(self.path.clone()));
        cli.run_captured().await
    }

    pub fn new_on(&self, rev: &'_ str) -> &Self {
        self.exec(["new", rev])
    }

    pub fn exec<'a>(&self, args: impl AsRef<[&'a str]>) -> &Self {
        self.jj.exec(args).unwrap();
        self
    }

    /// Creates a bookmark and tracks it
    pub fn create_tracked_bookmark(&self, bookmark: impl JJName) -> &Self {
        self.jj
            .exec(["bookmark", "create", &bookmark.name_for_jj()])
            .unwrap();
        self.track_bookmark(bookmark);
        self
    }

    pub fn track_bookmark(&self, bookmark: impl JJName) -> &Self {
        let output = self
            .jj
            .exec([
                "bookmark",
                "track",
                &bookmark.name_for_jj(),
                "--remote",
                "origin",
            ])
            .unwrap();

        if output.stdout.contains("Nothing changed.") {
            panic!("Failed to track {}", bookmark.raw_name());
        }

        self
    }

    /// Creates a bookmark, tracks it, and pushes it to origin
    pub fn create_and_push_bookmark(&self, bookmark: impl JJName + Copy) -> &Self {
        self.create_tracked_bookmark(bookmark);
        self.push_bookmark(bookmark)
    }

    /// Creates a change, creates a bookmark, and tracks it
    pub fn create_change_and_tracked_bookmark(&self, bookmark: impl JJName) -> &Self {
        self.create_change(
            &format!("{}.txt", bookmark.raw_name().replace("/", "--")),
            &format!("content for {}", bookmark.raw_name()),
            &format!(
                "Commit for {} bookmark\nDescription for {} bookmark",
                bookmark.raw_name(),
                bookmark.raw_name(),
            ),
        )
        .create_tracked_bookmark(bookmark)
    }

    /// Creates a stack of changes, each with a tracked bookmark, and submits
    /// the stack
    pub async fn submit_stack<I>(&self, bookmarks: I) -> &Self
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: JJName,
    {
        let bookmarks = bookmarks.into_iter().collect::<Vec<_>>();

        let last_name = bookmarks.last().unwrap().name_for_jj();

        for bookmark in bookmarks {
            self.create_change_and_tracked_bookmark(bookmark);
            self.jj(["new"]).unwrap();
        }

        self.run(["submit", &last_name]).await;

        self
    }

    /// Creates a change, creates a bookmark, tracks it, and starts a new
    /// working copy
    pub fn commit_with_bookmark(
        &self,
        file: &str,
        content: &str,
        msg: &str,
        bookmark: impl JJName,
    ) -> &Self {
        self.create_change(file, content, msg);
        self.create_tracked_bookmark(bookmark);
        self.jj.exec(["new"]).unwrap();
        self
    }

    fn make_repo() -> RepoInfo {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let jj = Jujutsu::new(&path).unwrap();

        jj.exec(["git", "init"]).unwrap();

        let user_name = format!("Test User {}", uuid::Uuid::new_v4());
        let user_email = format!("test+{}@example.com", uuid::Uuid::new_v4());

        jj.exec(["config", "set", "--repo", "user.name", &user_name])
            .unwrap();
        jj.exec(["config", "set", "--repo", "user.email", &user_email])
            .unwrap();
        jj.exec(["metaedit", "--update-author"]).unwrap();

        RepoInfo {
            dir,
            path,
            user_name,
            user_email,
        }
    }

    /// Sets a jj configuration value for the repo
    pub fn set_config(&self, key: &str, value: &str) -> &Self {
        self.jj
            .exec(["config", "set", "--repo", key, value])
            .unwrap();
        self
    }
}

struct RepoInfo {
    pub dir: TempDir,
    pub path: PathBuf,
    pub user_name: String,
    pub user_email: String,
}

impl Default for TestRepo<()> {
    fn default() -> Self {
        Self::new()
    }
}
