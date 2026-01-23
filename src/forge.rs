pub mod forgejo;
pub mod github;
pub mod gitlab;
pub mod test;

use std::borrow::Cow;

use bon::Builder;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use crate::{
    config::{Config, ForgeType},
    description::FormatMergeRequest,
    error::Result,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeUser {
    /// The ID of the user (usually a numeric ID)
    pub id: Option<String>,

    /// The username of the user
    pub username: String,
}

#[derive(Debug, Clone)]
pub enum ForgeMergeRequest {
    GitLab(gitlab::MergeRequest),
    GitHub(github::PullRequest),
    Forgejo(forgejo::PullRequest),
    Test(test::MergeRequest),
}

impl ForgeMergeRequest {
    pub fn iid(&self) -> Cow<'_, str> {
        match self {
            ForgeMergeRequest::GitLab(mr) => Cow::Owned(mr.iid.to_string()),
            ForgeMergeRequest::GitHub(pr) => Cow::Owned(pr.number.to_string()),
            ForgeMergeRequest::Forgejo(pr) => Cow::Owned(pr.number.to_string()),
            ForgeMergeRequest::Test(mr) => Cow::Owned(mr.id.clone()),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.title,
            ForgeMergeRequest::GitHub(pr) => &pr.title,
            ForgeMergeRequest::Forgejo(pr) => &pr.title,
            ForgeMergeRequest::Test(mr) => &mr.title,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => mr.description.as_deref().unwrap_or(""),
            ForgeMergeRequest::GitHub(pr) => pr.body.as_deref().unwrap_or(""),
            ForgeMergeRequest::Forgejo(pr) => pr.body.as_deref().unwrap_or(""),
            ForgeMergeRequest::Test(mr) => mr.description.as_deref().unwrap_or(""),
        }
    }

    pub fn source_branch(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.source_branch,
            ForgeMergeRequest::GitHub(pr) => &pr.head.ref_name,
            ForgeMergeRequest::Forgejo(pr) => &pr.head.ref_name,
            ForgeMergeRequest::Test(mr) => &mr.source_branch,
        }
    }

    pub fn target_branch(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.target_branch,
            ForgeMergeRequest::GitHub(pr) => &pr.base.ref_name,
            ForgeMergeRequest::Forgejo(pr) => &pr.base.ref_name,
            ForgeMergeRequest::Test(mr) => &mr.target_branch,
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
            ForgeMergeRequest::Test(mr) => mr.state,
        }
    }

    pub fn url(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.web_url,
            ForgeMergeRequest::GitHub(pr) => &pr.html_url,
            ForgeMergeRequest::Forgejo(pr) => &pr.html_url,
            ForgeMergeRequest::Test(mr) => &mr.url,
        }
    }

    pub fn edit_url(&self) -> Cow<'_, str> {
        match self {
            ForgeMergeRequest::GitLab(mr) => Cow::Owned(format!("{}/edit", mr.web_url)),
            ForgeMergeRequest::GitHub(pr) => Cow::Borrowed(&pr.html_url),
            ForgeMergeRequest::Forgejo(pr) => Cow::Borrowed(&pr.html_url),
            ForgeMergeRequest::Test(mr) => Cow::Borrowed(&mr.url),
        }
    }

    pub fn author_username(&self) -> &str {
        match self {
            ForgeMergeRequest::GitLab(mr) => &mr.author.username,
            ForgeMergeRequest::GitHub(pr) => &pr.user.login,
            ForgeMergeRequest::Forgejo(pr) => &pr.user.login,
            ForgeMergeRequest::Test(mr) => &mr.author_username,
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
            ForgeMergeRequest::Test(mr) => mr.created_at,
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
            ForgeMergeRequest::Test(mr) => mr.assignees.clone(),
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
            ForgeMergeRequest::Test(mr) => mr.reviewers.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ForgeMergeRequestState {
    #[default]
    Open,
    Closed,
    Merged,
}

/// Status of CI/Pipeline checks
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CheckStatus {
    /// All checks passed
    Success,
    /// Checks are still running
    Pending,
    /// Some checks failed
    Failed,
    /// No checks configured or required
    #[default]
    None,
}

/// Satisfaction of approval requirements
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalSatisfaction {
    /// All approval requirements are satisfied
    Satisfied,

    /// Some approval requirements are not satisfied
    Unsatisfied,

    /// Approval requirements are unknown
    Unknown,
}

/// Approval status of a merge request
#[derive(Debug, Clone)]
pub struct ApprovalStatus {
    /// Number of approvals received
    pub approved_count: u32,

    /// Number of approvals required
    pub required_count: u32,

    /// Number of approvals that are blocking the merge request
    pub blocking_count: u32,

    /// Whether approval requirements are satisfied
    pub satisfaction: ApprovalSatisfaction,
}

impl Default for ApprovalStatus {
    fn default() -> Self {
        Self {
            approved_count: 0,
            required_count: 0,
            blocking_count: 0,
            satisfaction: ApprovalSatisfaction::Unknown,
        }
    }
}

/// Complete status information for a merge request
#[derive(Debug, Clone)]
pub struct MergeRequestStatus {
    /// The internal ID of the merge request
    pub iid: String,

    /// CI/Pipeline check status
    pub check_status: CheckStatus,

    /// Approval status
    pub approval_status: ApprovalStatus,
}

impl MergeRequestStatus {
    pub fn ready_to_merge(&self) -> bool {
        self.approval_status.satisfaction == ApprovalSatisfaction::Satisfied
            && (self.check_status == CheckStatus::Success || self.check_status == CheckStatus::None)
    }
}

#[derive(Builder, Default)]
pub struct ForgeCreateMergeRequestOptions {
    /// The source branch of the merge request
    pub source_branch: String,

    /// The target branch of the merge request
    pub target_branch: String,

    /// The title of the merge request
    pub title: String,

    /// The description of the merge request
    #[builder(required)]
    pub description: Option<String>,

    /// The usernames of the initial assignees of the merge request
    pub assignee_usernames: Vec<String>,

    /// The usernames of the initial assignees of the merge request
    pub reviewer_usernames: Vec<String>,

    /// Whether to remove the source branch after the merge request is merged
    pub remove_source_branch: bool,

    /// Whether to squash the commits into a single commit
    pub squash: bool,

    /// Whether to open the merge request as a draft
    pub open_as_draft: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DiscussionCount {
    /// The total number of discussions.
    pub all: u32,

    /// The number of unresolved (resolvable) discussions.
    pub unresolved: u32,

    /// The number of resolved discussions.
    pub resolved: u32,
}

/// A trait for a code forge (e.g. GitLab, GitHub, Forgejo, etc.)
#[enum_dispatch]
pub trait Forge: Send + Sync + FormatMergeRequest {
    /// The project ID of the project in the forge. E.g. "group/project" or
    /// "12345" for a numeric project ID. Combined with the base URL, this forms
    /// the full URL to the project in the forge.
    fn project_id(&self) -> &str;

    /// The project ID where branches are pushed (source/fork project).
    fn source_project_id(&self) -> &str;

    /// The project ID where MRs/PRs are created (target/upstream project).
    fn target_project_id(&self) -> &str;

    /// The base URL of the forge. E.g. <https://gitlab.example.com>.
    fn base_url(&self) -> &str;

    /// The full URL to the project in the forge. E.g. <https://gitlab.example.com/group/project>.
    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url(), self.project_id())
    }

    /// Get the current authenticated user in the forge.
    async fn current_user(&self) -> Result<ForgeUser>;

    /// Gets a user in the forge by their username.
    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>>;

    /// Find merge request by source branch name. Returns the first MR found.
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>>;

    /// Create a new merge request in the forge for the project.
    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest>;

    /// Update the target branch (base) of an existing merge request.
    async fn update_merge_request_base(
        &self,
        merge_request_iid: &str,
        new_base: &str,
    ) -> Result<ForgeMergeRequest>;

    /// Update the description of an existing merge request.
    async fn update_merge_request_description(
        &self,
        merge_request_iid: &str,
        new_description: &str,
    ) -> Result<ForgeMergeRequest>;

    /// Get a specific merge request by IID.
    async fn get_merge_request(&self, merge_request_iid: &str) -> Result<ForgeMergeRequest>;

    /// Get approval status for a merge request.
    async fn get_approval_status(&self, merge_request_iid: &str) -> Result<ApprovalStatus>;

    /// Get CI/pipeline check status for a merge request.
    async fn get_check_status(&self, merge_request_iid: &str) -> Result<CheckStatus>;

    /// Get complete status information for a merge request.
    async fn get_merge_request_status(&self, merge_request_iid: &str)
    -> Result<MergeRequestStatus>;

    /// Get the number of open discussions for a merge request.
    async fn num_open_discussions(&self, merge_request_iid: &str) -> Result<DiscussionCount>;
}

#[enum_dispatch(Forge, FormatMergeRequest)]
pub enum ForgeImpl {
    GitLab(gitlab::GitLabForge),
    GitHub(github::GitHubForge),
    Forgejo(forgejo::ForgejoForge),
    Test(test::TestForge),
}

impl ForgeImpl {
    /// Create a new forge. Looks for a jj-vine config in the current directory.
    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let config = Config::load(&cwd)?;
        Self::new(&config)
    }

    pub fn new(config: &Config) -> Result<Self> {
        config.validate()?;

        match config.forge {
            ForgeType::GitLab => {
                let source = config.gitlab.source_project();
                let target = config.gitlab.target_project();
                gitlab::GitLabForge::new(
                    config.gitlab.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.gitlab.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                )
                .map(|forge| forge.into())
            }
            ForgeType::GitHub => {
                let source = config.github.source_project();
                let target = config.github.target_project();
                github::GitHubForge::new(
                    config.github.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.github.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                )
                .map(|forge| forge.into())
            }
            ForgeType::Forgejo => {
                let source = config.forgejo.source_project();
                let target = config.forgejo.target_project();
                forgejo::ForgejoForge::new(
                    config.forgejo.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.forgejo.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                    config.forgejo.wip_prefix.clone(),
                )
                .map(|forge| forge.into())
            }
        }
    }
}
