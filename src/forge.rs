pub mod forgejo;
pub mod github;
pub mod gitlab;

use async_trait::async_trait;
use bon::Builder;
use serde::{Deserialize, Serialize};

use crate::{config::ForgeType, description::FormatMergeRequest, error::Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeUser {
    /// The ID of the user (usually a numeric ID)
    pub id: String,

    /// The username of the user
    pub username: String,
}

#[derive(Debug, Clone)]
pub enum ForgeMergeRequest {
    GitLab(gitlab::MergeRequest),
    GitHub(github::PullRequest),
    Forgejo(forgejo::PullRequest),
}

impl ForgeMergeRequest {
    pub fn iid(&self) -> u64 {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr.iid,
            ForgeMergeRequest::GitHub(pr) => pr.number,
            ForgeMergeRequest::Forgejo(pr) => pr.number,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.title,
            ForgeMergeRequest::GitHub(pr) => &pr.title,
            ForgeMergeRequest::Forgejo(pr) => &pr.title,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr.description.as_deref().unwrap_or(""),
            ForgeMergeRequest::GitHub(pr) => pr.body.as_deref().unwrap_or(""),
            ForgeMergeRequest::Forgejo(pr) => pr.body.as_deref().unwrap_or(""),
        }
    }

    pub fn source_branch(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.source_branch,
            ForgeMergeRequest::GitHub(pr) => &pr.head.ref_name,
            ForgeMergeRequest::Forgejo(pr) => &pr.head.ref_name,
        }
    }

    pub fn target_branch(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.target_branch,
            ForgeMergeRequest::GitHub(pr) => &pr.base.ref_name,
            ForgeMergeRequest::Forgejo(pr) => &pr.base.ref_name,
        }
    }

    pub fn state(&self) -> ForgeMergeRequestState {
        match self {
            ForgeMergeRequest::GitLab(mr) => {
                if mr.state == "opened" {
                    ForgeMergeRequestState::Open
                } else if mr.state == "closed" {
                    ForgeMergeRequestState::Closed
                } else if mr.state == "merged" {
                    ForgeMergeRequestState::Merged
                } else {
                    ForgeMergeRequestState::Open
                }
            }
            ForgeMergeRequest::GitHub(pr) => {
                if pr.merged {
                    ForgeMergeRequestState::Merged
                } else if pr.state == "open" {
                    ForgeMergeRequestState::Open
                } else {
                    ForgeMergeRequestState::Closed
                }
            }
            ForgeMergeRequest::Forgejo(pr) => {
                if pr.merged {
                    ForgeMergeRequestState::Merged
                } else if pr.state == "open" {
                    ForgeMergeRequestState::Open
                } else {
                    ForgeMergeRequestState::Closed
                }
            }
        }
    }

    pub fn url(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.web_url,
            ForgeMergeRequest::GitHub(pr) => &pr.html_url,
            ForgeMergeRequest::Forgejo(pr) => &pr.html_url,
        }
    }

    pub fn author_username(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.author.username,
            ForgeMergeRequest::GitHub(pr) => &pr.user.login,
            ForgeMergeRequest::Forgejo(pr) => &pr.user.login,
        }
    }

    pub fn created_at(&self) -> jiff::Timestamp {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr
                .created_at
                .parse()
                .expect("Failed to parse created at timestamp from GitLab API response"),
            ForgeMergeRequest::GitHub(pr) => pr
                .created_at
                .parse()
                .expect("Failed to parse created at timestamp from GitHub API response"),
            ForgeMergeRequest::Forgejo(pr) => pr
                .created_at
                .parse()
                .expect("Failed to parse created at timestamp from Forgejo API response"),
        }
    }

    pub fn assignees(&self) -> Vec<ForgeUser> {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr
                .assignees
                .clone()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
            ForgeMergeRequest::GitHub(pr) => pr
                .assignees
                .clone()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
            ForgeMergeRequest::Forgejo(pr) => pr
                .assignees
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
        }
    }

    pub fn reviewers(&self) -> Vec<ForgeUser> {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr
                .reviewers
                .clone()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
            ForgeMergeRequest::GitHub(pr) => pr
                .requested_reviewers
                .clone()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
            ForgeMergeRequest::Forgejo(pr) => pr
                .requested_reviewers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(ForgeUser::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ForgeMergeRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Builder)]
pub struct ForgeCreateMergeRequestOptions {
    /// The source branch of the merge request
    source_branch: String,

    /// The target branch of the merge request
    target_branch: String,

    /// The title of the merge request
    title: String,

    /// The description of the merge request
    description: Option<String>,

    /// The IDs of the initial assignees of the merge request
    assignee_ids: Option<Vec<String>>,

    /// The IDs of the initial reviewers of the merge request
    reviewer_ids: Option<Vec<String>>,

    /// Whether to remove the source branch after the merge request is merged
    remove_source_branch: Option<bool>,

    /// Whether to squash the commits into a single commit
    squash: Option<bool>,
}

/// A trait for a code forge (e.g. GitLab, GitHub, Forgejo, etc.)
#[async_trait]
pub trait Forge: Send + Sync + FormatMergeRequest {
    /// The project ID of the project in the forge. E.g. "group/project" or
    /// "12345" for a numeric project ID. Combined with the base URL, this forms
    /// the full URL to the project in the forge.
    fn project_id(&self) -> &str;

    /// The base URL of the forge. E.g. <https://gitlab.example.com>
    fn base_url(&self) -> &str;

    /// The full URL to the project in the forge. E.g. <https://gitlab.example.com/group/project>
    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url(), self.project_id())
    }

    /// Get the current authenticated user in the forge
    async fn current_user(&self) -> Result<ForgeUser>;

    /// Gets a user in the forge by their username
    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>>;

    /// Find merge request by source branch name. Returns the first MR found
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>>;

    /// Create a new merge request in the forge for the project
    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest>;

    /// Update the target branch (base) of an existing merge request
    async fn update_merge_request_base(
        &self,
        merge_request_iid: u64,
        new_base: &str,
    ) -> Result<ForgeMergeRequest>;

    /// Update the description of an existing merge request
    async fn update_merge_request_description(
        &self,
        merge_request_iid: u64,
        new_description: &str,
    ) -> Result<ForgeMergeRequest>;

    /// Get a specific merge request by IID
    async fn get_merge_request(&self, merge_request_iid: u64) -> Result<ForgeMergeRequest>;
}

/// Create a forge instance based on configuration
pub fn create_forge(config: &crate::config::Config) -> Result<Box<dyn Forge>> {
    config.validate()?;

    match config.forge {
        ForgeType::GitLab => {
            let forge = gitlab::GitLabForge::new(
                config.gitlab.host.clone(),
                config.gitlab.project.clone(),
                config.gitlab.token.clone(),
                config.ca_bundle.clone(),
                config.tls_accept_non_compliant_certs,
            )?;
            Ok(Box::new(forge))
        }
        ForgeType::GitHub => {
            let forge = github::GitHubForge::new(
                config.github.host.clone(),
                config.github.project.clone(),
                config.github.token.clone(),
                config.ca_bundle.clone(),
                config.tls_accept_non_compliant_certs,
            )?;
            Ok(Box::new(forge))
        }
        ForgeType::Forgejo => {
            let forge = forgejo::ForgejoForge::new(
                config.forgejo.host.clone(),
                config.forgejo.project.clone(),
                config.forgejo.token.clone(),
                config.ca_bundle.clone(),
                config.tls_accept_non_compliant_certs,
            )?;
            Ok(Box::new(forge))
        }
    }
}
