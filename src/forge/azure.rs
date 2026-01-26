use std::{collections::HashMap, path::Path};

use bon::bon;
use futures::try_join;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::OnceCell;

use crate::{
    description::FormatMergeRequest,
    error::{AzureDevOpsApiSnafu, ConfigSnafu, Result},
    forge::{
        ApprovalSatisfaction,
        ApprovalStatus,
        CheckStatus,
        DiscussionCount,
        Forge,
        ForgeCreateMergeRequestOptions,
        ForgeMergeRequest,
        ForgeUser,
        MergeRequestStatus,
    },
};

#[allow(dead_code)]
pub struct AzureDevOpsForge {
    /// The base URL of the Azure DevOps instance, e.g. <https://dev.azure.com>.
    base_url: String,

    /// The base URL of the Azure DevOps Security (VSSP) instance, e.g. <https://vssps.dev.azure.com>.
    vssps_base_url: Option<String>,

    /// The organization and project name in the format "organization/project".
    /// This is the project where branches are pushed.
    source_project_id: String,

    /// The organization and project name in the format "organization/project".
    /// This is the project where MRs/PRs are created.
    target_project_id: String,

    /// The personal access token for the Azure DevOps instance.
    token: String,

    /// The name of the organization in the source project.
    source_org: String,

    /// The name of the project in the source organization.
    source_project: String,

    /// The name of the organization in the target project.
    target_org: String,

    /// The name of the project in the target organization.
    target_project: String,

    /// The name of the repository where branches are pushed (source/fork
    /// project). The repository ID will be fetched from the API.
    source_repository_name: Option<String>,

    /// The name of the repository where MRs/PRs are created (target/upstream
    /// project). The repository ID will be fetched from the API.
    target_repository_name: Option<String>,

    /// The repository ID where branches are pushed (source/fork project).
    source_repository_id: OnceCell<String>,

    /// The repository ID where MRs/PRs are created (target/upstream project).
    target_repository_id: OnceCell<String>,

    client: reqwest::Client,
}

#[bon]
impl AzureDevOpsForge {
    #[builder]
    pub fn new(
        base_url: impl Into<String>,
        vssps_base_url: Option<impl Into<String>>,
        source_project_id: impl Into<String>,
        target_project_id: impl Into<String>,
        token: impl Into<String>,
        source_repository_name: Option<String>,
        target_repository_name: Option<String>,
        source_repository_id: Option<String>,
        target_repository_id: Option<String>,
        ca_bundle: Option<impl AsRef<Path>>,
        accept_non_compliant_certs: bool,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder();

        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        if let Some(ca_path) = ca_bundle {
            let ca_cert = std::fs::read(ca_path.as_ref()).map_err(|e| {
                ConfigSnafu {
                    message: format!(
                        "Failed to read CA bundle at {}: {}",
                        ca_path.as_ref().to_string_lossy(),
                        e
                    ),
                }
                .build()
            })?;

            let certs = reqwest::Certificate::from_pem_bundle(&ca_cert).map_err(|e| {
                ConfigSnafu {
                    message: format!("Failed to parse CA bundle: {}", e),
                }
                .build()
            })?;

            for cert in certs {
                client_builder = client_builder.add_root_certificate(cert);
            }
        }

        let client = client_builder.build().map_err(|e| {
            ConfigSnafu {
                message: format!("Failed to build HTTP client: {}", e),
            }
            .build()
        })?;

        let base_url = base_url.into().trim_end_matches('/').to_string();
        let vssps_base_url = vssps_base_url.map(|url| url.into().trim_end_matches('/').to_string());

        let source_project_id = source_project_id.into();
        let target_project_id = target_project_id.into();

        let source_project_id_clone = source_project_id.clone();
        let (source_org, source_repo) = source_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid source project ID: {}", source_project_id),
            }
            .build(),
        )?;

        let target_project_id_clone = target_project_id.clone();
        let (target_org, target_repo) = target_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid target project ID: {}", target_project_id),
            }
            .build(),
        )?;

        Ok(Self {
            base_url,
            vssps_base_url,
            source_project_id,
            target_project_id,
            token: token.into(),
            source_org: source_org.into(),
            source_project: source_repo.into(),
            target_org: target_org.into(),
            target_project: target_repo.into(),
            source_repository_name,
            target_repository_name,
            source_repository_id: OnceCell::new_with(source_repository_id),
            target_repository_id: OnceCell::new_with(target_repository_id),
            client,
        })
    }

    async fn request_git<T: DeserializeOwned>(
        &self,
        method: Method,
        project_id: impl AsRef<str>,
        path: impl AsRef<str>,
        payload: Option<impl Serialize>,
    ) -> Result<T> {
        self.request(
            method,
            &self.base_url,
            format!("/{}/_apis/git{}", project_id.as_ref(), path.as_ref()),
            payload,
        )
        .await
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        base_url: impl AsRef<str>,
        path: impl AsRef<str>,
        payload: Option<impl Serialize>,
    ) -> Result<T> {
        let url = format!("{}{}", base_url.as_ref(), path.as_ref());

        let mut req = self
            .client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", &self.token))
            .header("Accept", "application/json")
            .header("User-Agent", "jj-vine");

        if let Some(payload) = payload.as_ref() {
            req = req.json(payload);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(AzureDevOpsApiSnafu {
                message: format!("Failed request: {} - {}", status, text),
            }
            .build());
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| {
            AzureDevOpsApiSnafu {
                message: format!(
                    "Failed to parse response to {}: {}, response: {}",
                    path.as_ref(),
                    e,
                    body
                ),
            }
            .build()
        })?;
        Ok(data)
    }

    async fn source_repository_id(&self) -> Result<&String> {
        match self.source_repository_id.get() {
            Some(id) => return Ok(id),
            None => {
                if self.source_repository_name.is_none() {
                    return ConfigSnafu {
                        message:
                            "Must provide either source repository name or source repository ID"
                                .to_string(),
                    }
                    .fail();
                }
            }
        };

        self.source_repository_id
            .get_or_try_init(async || {
                let repositories: ListResponse<GitRepository> = self
                    .request_git(
                        Method::GET,
                        &self.source_project_id,
                        "/repositories?api-version=7.1",
                        None::<()>,
                    )
                    .await?;

                repositories
                    .value
                    .into_iter()
                    .find(|repository| {
                        repository.name == *self.source_repository_name.as_ref().unwrap()
                    })
                    .map(|repository| repository.id)
                    .ok_or(
                        ConfigSnafu {
                            message: format!(
                                "Source repository not found: {}",
                                self.source_repository_name.as_ref().unwrap()
                            ),
                        }
                        .build(),
                    )
            })
            .await
    }

    async fn target_repository_id(&self) -> Result<&String> {
        match self.target_repository_id.get() {
            Some(id) => return Ok(id),
            None => {
                if self.target_repository_name.is_none() {
                    return ConfigSnafu {
                        message:
                            "Must provide either target repository name or target repository ID"
                                .to_string(),
                    }
                    .fail();
                }
            }
        };

        self.target_repository_id
            .get_or_try_init(async || {
                let repositories: ListResponse<GitRepository> = self
                    .request_git(
                        Method::GET,
                        &self.target_project_id,
                        "/repositories?api-version=7.1",
                        None::<()>,
                    )
                    .await?;

                repositories
                    .value
                    .into_iter()
                    .find(|repository| {
                        repository.name == *self.target_repository_name.as_ref().unwrap()
                    })
                    .map(|repository| repository.id)
                    .ok_or(
                        ConfigSnafu {
                            message: format!(
                                "Source repository not found: {}",
                                self.target_repository_name.as_ref().unwrap()
                            ),
                        }
                        .build(),
                    )
            })
            .await
    }
}

impl Forge for AzureDevOpsForge {
    fn source_project_id(&self) -> &str {
        &self.source_project_id
    }

    fn target_project_id(&self) -> &str {
        &self.target_project_id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn current_user(&self) -> Result<ForgeUser> {
        let user: IdentityRef = self
            .request(
                Method::GET,
                &self.base_url,
                format!("/{}/_apis/ConnectionData?api-version=7.1", self.source_org),
                None::<()>,
            )
            .await?;
        Ok(user.into())
    }

    async fn user_by_username(&self, user_descriptor: &str) -> Result<Option<ForgeUser>> {
        if let Some(vssps_base_url) = &self.vssps_base_url {
            let user: IdentityRef = self
                .request(
                    Method::GET,
                    vssps_base_url,
                    format!(
                        "/{}/_apis/graph/users/{}?api-version=7.1-preview.1",
                        self.source_org, user_descriptor
                    ),
                    None::<()>,
                )
                .await?;
            Ok(Some(user.into()))
        } else {
            Ok(None)
        }
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>> {
        let mrs: ListResponse<GitPullRequest> = self
            .request_git(
                Method::GET,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullRequests?api-version=7.1&searchCriteria.sourceRefName=refs/heads/{}",
                    self.target_repository_id().await?,
                    branch
                ),
                None::<()>,
            )
            .await?;

        Ok(mrs
            .value
            .into_iter()
            .next()
            .map(|pr| ForgeMergeRequest::AzureDevOps(Box::new(pr))))
    }

    async fn create_merge_request(
        &self,
        ForgeCreateMergeRequestOptions {
            description,
            open_as_draft,
            remove_source_branch,
            reviewer_usernames,
            source_branch,
            squash,
            target_branch,
            title,
            // Azure DevOps has no concept of "assignees", only "reviewers".
            assignee_usernames: _assignee_usernames,
        }: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest> {
        let body = CreatePullRequestBody {
            completion_options: RequestGitPullRequestCompletionOptions {
                delete_source_branch: if remove_source_branch {
                    Some(true)
                } else {
                    None
                },
                merge_strategy: if squash {
                    Some(GitPullRequestMergeStrategy::Squash)
                } else {
                    None
                },
            },
            description: description.unwrap_or_default(),
            fork_source: if self.source_project_id != self.target_project_id {
                Some(RequestGitForkRef {
                    repository: RequestGitRepository {
                        id: self.source_repository_id().await?.clone(),
                    },
                })
            } else {
                None
            },
            is_draft: open_as_draft,
            labels: Vec::new(),
            reviewers: reviewer_usernames
                .into_iter()
                .map(|username| RequestIdentityRefWithVote::Descriptor {
                    descriptor: username,
                })
                .collect(),
            source_ref_name: format!("refs/heads/{}", source_branch),
            target_ref_name: format!("refs/heads/{}", target_branch),
            title,
        };

        let pr: GitPullRequest = self
            .request_git(
                Method::POST,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests?api-version=7.1",
                    self.target_repository_id().await?
                ),
                Some(body),
            )
            .await?;

        Ok(ForgeMergeRequest::AzureDevOps(Box::new(pr)))
    }

    async fn update_merge_request_base(
        &self,
        merge_request_iid: &str,
        new_base: &str,
    ) -> Result<ForgeMergeRequest> {
        let body = UpdatePullRequestBody {
            description: None,
            target_ref_name: Some(format!("refs/heads/{}", new_base)),
        };

        let pr: GitPullRequest = self
            .request_git(
                Method::PATCH,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                Some(body),
            )
            .await?;

        Ok(ForgeMergeRequest::AzureDevOps(Box::new(pr)))
    }

    async fn update_merge_request_description(
        &self,
        merge_request_iid: &str,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let body = UpdatePullRequestBody {
            description: Some(new_description.to_string()),
            target_ref_name: None,
        };

        let pr: GitPullRequest = self
            .request_git(
                Method::PATCH,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                Some(body),
            )
            .await?;

        Ok(ForgeMergeRequest::AzureDevOps(Box::new(pr)))
    }

    async fn get_merge_request(&self, merge_request_iid: &str) -> Result<ForgeMergeRequest> {
        let pr: GitPullRequest = self
            .request_git(
                Method::GET,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(ForgeMergeRequest::AzureDevOps(Box::new(pr)))
    }

    async fn get_approval_status(&self, merge_request_iid: &str) -> Result<ApprovalStatus> {
        let pr: GitPullRequest = self
            .request_git(
                Method::GET,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(ApprovalStatus {
            approved_count: pr
                .reviewers
                .iter()
                .filter(|reviewer| reviewer.vote == Vote::Approved)
                .count() as u32,
            required_count: pr
                .reviewers
                .iter()
                .filter(|reviewer| reviewer.is_required)
                .count() as u32,
            blocking_count: pr
                .reviewers
                .iter()
                .filter(|reviewer| {
                    reviewer.vote == Vote::Rejected || reviewer.vote == Vote::WaitingForAuthor
                })
                .count() as u32,
            satisfaction: if pr.merge_status == PullRequestAsyncStatus::RejectedByPolicy {
                ApprovalSatisfaction::Unsatisfied
            } else {
                ApprovalSatisfaction::Satisfied
            },
        })
    }

    async fn get_check_status(&self, merge_request_iid: &str) -> Result<CheckStatus> {
        let pr: GitPullRequest = self
            .request_git(
                Method::GET,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        match pr.merge_status {
            PullRequestAsyncStatus::Succeeded => Ok(CheckStatus::Success),
            PullRequestAsyncStatus::Queued => Ok(CheckStatus::Pending),
            PullRequestAsyncStatus::Conflicts => Ok(CheckStatus::None),
            PullRequestAsyncStatus::RejectedByPolicy => Ok(CheckStatus::None),
            PullRequestAsyncStatus::Failure => Ok(CheckStatus::Failed),
            PullRequestAsyncStatus::NotSet => Ok(CheckStatus::None),
        }
    }

    async fn get_merge_request_status(
        &self,
        merge_request_iid: &str,
    ) -> Result<MergeRequestStatus> {
        let (approval_status, check_status) = try_join!(
            self.get_approval_status(merge_request_iid),
            self.get_check_status(merge_request_iid),
        )?;

        Ok(MergeRequestStatus {
            iid: merge_request_iid.to_string(),
            approval_status,
            check_status,
        })
    }

    async fn num_open_discussions(&self, merge_request_iid: &str) -> Result<DiscussionCount> {
        let threads: ListResponse<GitPullRequestCommentThread> = self
            .request_git(
                Method::GET,
                &self.target_project_id,
                format!(
                    "/repositories/{}/pullrequests/{}/threads?api-version=7.1",
                    self.target_repository_id().await?,
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        let mut unresolved = 0;
        let mut resolved = 0;
        for thread in threads.value {
            match thread.status {
                CommentThreadStatus::Active => unresolved += 1,
                CommentThreadStatus::Pending => unresolved += 1,
                CommentThreadStatus::Unknown => unresolved += 1,

                CommentThreadStatus::Fixed => resolved += 1,
                CommentThreadStatus::Closed => resolved += 1,
                CommentThreadStatus::ByDesign => resolved += 1,
                CommentThreadStatus::WontFix => resolved += 1,
            }
        }

        Ok(DiscussionCount {
            all: unresolved + resolved,
            unresolved,
            resolved,
        })
    }

    fn project_id(&self) -> &str {
        &self.target_project_id
    }

    async fn sync_dependent_merge_requests(
        &self,
        _merge_request_iid: &str,
        _dependent_merge_request_iids: &[&str],
    ) -> Result<bool> {
        // Only supported for GitLab
        Ok(false)
    }
}

impl FormatMergeRequest for AzureDevOpsForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("!{}", mr_iid)
    }

    fn mr_name(&self) -> &'static str {
        "PR"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepository {
    pub id: String,

    pub name: String,

    pub url: String,

    pub project: TeamProjectReference,

    pub remote_url: Option<String>,

    #[serde(default)]
    pub is_fork: bool,

    pub web_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamProjectReference {
    pub id: String,

    pub name: String,

    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPullRequest {
    pub artifact_id: Option<String>,

    pub completion_options: Option<GitPullRequestCompletionOptions>,

    pub created_by: IdentityRef,

    pub creation_date: String,

    #[serde(default)]
    pub description: String,

    pub fork_source: Option<GitForkRef>,

    #[serde(default)]
    pub has_multiple_merge_bases: bool,

    #[serde(default)]
    pub is_draft: bool,

    pub last_merge_commit: Option<GitCommitRef>,

    pub last_merge_source_commit: Option<GitCommitRef>,

    pub last_merge_target_commit: Option<GitCommitRef>,

    pub merge_options: Option<GitPullRequestMergeOptions>,

    pub merge_status: PullRequestAsyncStatus,

    pub pull_request_id: i32,

    pub remote_url: Option<String>,

    pub repository: GitRepository,

    #[serde(default)]
    pub reviewers: Vec<IdentityRefWithVote>,

    pub source_ref_name: String,

    pub status: PullRequestStatus,

    pub target_ref_name: String,

    pub title: String,

    pub url: String,
}

impl GitPullRequest {
    pub fn web_url(&self, forge: &AzureDevOpsForge) -> String {
        format!(
            "{}/{}/_git/{}/pullrequest/{}",
            forge.base_url,
            forge.target_project_id(),
            self.repository.name,
            self.pull_request_id,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPullRequestCompletionOptions {
    pub delete_source_branch: bool,

    pub merge_strategy: GitPullRequestMergeStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitPullRequestMergeStrategy {
    NoFastForward,
    Squash,
    Rebase,
    RebaseMerge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRef {
    pub descriptor: String,

    pub display_name: String,

    pub id: String,

    pub url: String,
}

impl From<IdentityRef> for ForgeUser {
    fn from(user: IdentityRef) -> Self {
        ForgeUser {
            id: Some(user.descriptor),
            username: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitForkRef {
    pub creator: IdentityRef,

    pub is_locked: bool,

    pub is_locked_by: Option<IdentityRef>,

    pub name: String,

    pub object_id: String,

    pub repository: GitRepository,

    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestGitForkRef {
    pub repository: RequestGitRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestGitRepository {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestAsyncStatus {
    NotSet,
    Queued,
    Conflicts,
    Succeeded,
    RejectedByPolicy,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPullRequestMergeOptions {
    pub conflict_authorship_commits: bool,

    pub detect_rename_false_positives: bool,

    pub disable_renames: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitRef {
    pub author: Option<GitUserDate>,

    pub comment: Option<String>,

    pub commit_id: String,

    pub committer: Option<GitUserDate>,

    #[serde(default)]
    pub parents: Vec<String>,

    pub remote_url: Option<String>,

    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitUserDate {
    pub date: String,

    pub email: String,

    pub image_url: Option<String>,

    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRefWithVote {
    pub descriptor: String,

    pub display_name: String,

    pub has_declined: bool,

    pub id: String,

    pub is_flagged: bool,

    pub is_reapprove: bool,

    pub is_required: bool,

    pub reviewer_url: String,

    pub url: String,

    pub vote: Vote,

    pub voted_for: Vec<IdentityRefWithVote>,
}

impl From<IdentityRefWithVote> for ForgeUser {
    fn from(user: IdentityRefWithVote) -> Self {
        ForgeUser {
            id: Some(user.descriptor),
            username: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[repr(i16)]
pub enum Vote {
    Approved = 10,

    ApprovedWithSuggestions = 5,

    NoVote = 0,

    WaitingForAuthor = -5,

    Rejected = -10,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestStatus {
    NotSet,
    Active,
    Abandoned,
    Completed,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse<T> {
    pub value: Vec<T>,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePullRequestBody {
    pub completion_options: RequestGitPullRequestCompletionOptions,

    pub description: String,

    pub fork_source: Option<RequestGitForkRef>,

    pub is_draft: bool,

    pub labels: Vec<WebApiTagDefinition>,

    pub reviewers: Vec<RequestIdentityRefWithVote>,

    pub source_ref_name: String,

    pub target_ref_name: String,

    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum RequestIdentityRefWithVote {
    Descriptor { descriptor: String },
    Id { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebApiTagDefinition {
    pub active: bool,

    pub id: String,

    pub name: String,

    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestGitPullRequestCompletionOptions {
    pub delete_source_branch: Option<bool>,

    pub merge_strategy: Option<GitPullRequestMergeStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePullRequestBody {
    pub target_ref_name: Option<String>,

    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPullRequestCommentThread {
    pub comments: Vec<Comment>,
    pub id: i32,
    pub identifies: HashMap<String, IdentityRef>,
    pub is_deleted: bool,
    pub last_updated_date: String,
    pub published_date: String,
    pub status: CommentThreadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CommentThreadStatus {
    Unknown,
    Active,
    Fixed,
    WontFix,
    Closed,
    ByDesign,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: IdentityRef,

    pub comment_type: CommentType,

    pub content: String,

    pub id: i16,

    pub is_deleted: bool,

    pub last_content_updated_date: String,

    pub last_updated_date: String,

    pub parent_comment_id: Option<i16>,

    pub published_date: String,

    pub users_liked: Vec<IdentityRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommentType {
    Unknown,
    Text,
    CodeChange,
    System,
}
