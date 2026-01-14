use std::{collections::HashMap, path::Path};

use async_trait::async_trait;
use futures::try_join;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{
        Forge,
        ForgeCreateMergeRequestOptions,
        ForgeMergeRequest,
        ForgeUser,
        MergeRequestStatus,
    },
};

/// Forgejo/Gitea REST API client
pub struct ForgejoForge {
    base_url: String,
    source_project_id: String,
    target_project_id: String,
    source_owner: String,

    #[allow(dead_code)]
    source_repo: String,

    target_owner: String,
    target_repo: String,
    token: String,
    client: reqwest::Client,
}

/// Forgejo/Gitea user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoUser {
    /// User ID
    pub id: u64,

    /// Username (login)
    pub login: String,
}

impl From<ForgejoUser> for ForgeUser {
    fn from(user: ForgejoUser) -> Self {
        ForgeUser {
            id: user.id.to_string(),
            username: user.login,
        }
    }
}

/// Branch reference in a pull request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoBranchRef {
    /// Branch name
    #[serde(rename = "ref")]
    pub ref_name: String,

    /// Repository information (for cross-repo PRs)
    pub repo: Option<ForgejoRepo>,
}

/// Forgejo/Gitea repository info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoRepo {
    /// Repository full name (owner/repo)
    pub full_name: String,
}

/// Forgejo/Gitea Pull Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// PR number (index within repo)
    pub number: u64,

    /// PR ID (unique globally)
    pub id: u64,

    /// PR title
    pub title: String,

    /// PR body/description
    pub body: Option<String>,

    /// Head branch information
    pub head: ForgejoBranchRef,

    /// Base branch information
    pub base: ForgejoBranchRef,

    /// PR state (open, closed)
    pub state: String,

    /// HTML URL to view the PR
    pub html_url: String,

    /// User who created the PR
    pub user: ForgejoUser,

    /// Created at timestamp (ISO 8601)
    pub created_at: String,

    /// Assignees of the PR
    pub assignees: Option<Vec<ForgejoUser>>,

    /// Requested reviewers
    pub requested_reviewers: Option<Vec<ForgejoUser>>,

    /// Whether the PR was merged
    #[serde(default)]
    pub merged: bool,
}

/// Review state type (APPROVED, REQUEST_CHANGES, COMMENT, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ReviewStateType {
    Approved,
    RequestChanges,
    Comment,
    Pending,
    #[serde(other)]
    Unknown,
}

/// Forgejo Pull Request Review
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PullReview {
    /// Review ID
    pub id: u64,

    /// User who submitted the review
    pub user: ForgejoUser,

    /// Review state
    pub state: ReviewStateType,

    /// Whether the review was dismissed
    #[serde(default)]
    pub dismissed: bool,

    /// When the review was submitted
    pub submitted_at: Option<String>,
}

/// Commit status state (pending, success, error, failure)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum CommitStatusState {
    Pending,
    Success,
    Error,
    Failure,
    #[serde(other)]
    Unknown,
}

/// Individual commit status
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommitStatus {
    /// Status state
    pub state: CommitStatusState,
}

/// Combined status for a commit
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CombinedStatus {
    /// Combined state across all statuses
    pub state: CommitStatusState,

    /// Individual statuses
    #[serde(default)]
    pub statuses: Vec<CommitStatus>,

    /// Total count of statuses
    #[serde(default)]
    pub total_count: i64,
}

impl ForgejoForge {
    /// Create a new Forgejo/Gitea client
    ///
    /// # Arguments
    /// * `base_url` - Forgejo/Gitea instance URL (e.g., <https://codeberg.org>)
    /// * `project_id` - Repository in "owner/repo" format
    /// * `token` - Personal Access Token
    /// * `ca_bundle` - Optional path to CA bundle for TLS verification
    /// * `accept_non_compliant_certs` - Accept non-compliant TLS certificates
    pub fn new(
        base_url: impl Into<String>,
        source_project_id: impl Into<String>,
        target_project_id: impl Into<String>,
        token: impl Into<String>,
        ca_bundle: Option<impl AsRef<Path>>,
        accept_non_compliant_certs: bool,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder();

        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        if let Some(ca_path) = ca_bundle {
            let ca_cert = std::fs::read(ca_path.as_ref()).map_err(|e| Error::Config {
                message: format!(
                    "Failed to read CA bundle at {}: {}",
                    ca_path.as_ref().to_string_lossy(),
                    e
                ),
            })?;

            let certs =
                reqwest::Certificate::from_pem_bundle(&ca_cert).map_err(|e| Error::Config {
                    message: format!("Failed to parse CA bundle: {}", e),
                })?;

            for cert in certs {
                client_builder = client_builder.add_root_certificate(cert);
            }
        }

        let client = client_builder.build().map_err(|e| Error::Config {
            message: format!("Failed to build HTTP client: {}", e),
        })?;

        // Strip trailing slashes from base_url to avoid double slashes in constructed
        // URLs
        let base_url = base_url.into().trim_end_matches('/').to_string();

        let source_project_id = source_project_id.into();
        let target_project_id = target_project_id.into();

        let source_project_id_clone = source_project_id.clone();
        let (source_owner, source_repo) =
            source_project_id_clone
                .split_once('/')
                .ok_or(Error::Config {
                    message: format!("Invalid source project ID: {}", source_project_id),
                })?;

        let target_project_id_clone = target_project_id.clone();
        let (target_owner, target_repo) =
            target_project_id_clone
                .split_once('/')
                .ok_or(Error::Config {
                    message: format!("Invalid target project ID: {}", target_project_id),
                })?;

        Ok(Self {
            base_url,
            source_project_id,
            target_project_id,
            source_owner: source_owner.into(),
            source_repo: source_repo.into(),
            target_owner: target_owner.into(),
            target_repo: target_repo.into(),
            token: token.into(),
            client,
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: impl AsRef<str>,
        payload: Option<impl Serialize>,
    ) -> Result<T> {
        let url = format!("{}/api/v1{}", self.base_url, path.as_ref());
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
            return Err(Error::ForgejoApi {
                message: format!("Failed request: {} - {}", status, text),
            });
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| Error::ForgejoApi {
            message: format!(
                "Failed to parse response to {}: {}, response: {}",
                path.as_ref(),
                e,
                body
            ),
        })?;
        Ok(data)
    }

    async fn add_assignees(&self, pr_number: u64, assignee_usernames: Vec<String>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/{}/issues/{}/assignees",
                self.target_owner, self.target_repo, pr_number
            ),
            Some(serde_json::json!({
                "assignees": assignee_usernames,
            })),
        )
        .await?;
        Ok(())
    }

    async fn request_reviewers(
        &self,
        pr_number: u64,
        reviewer_usernames: Vec<String>,
    ) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/{}/pulls/{}/requested_reviewers",
                self.target_owner, self.target_repo, pr_number
            ),
            Some(serde_json::json!({
                "reviewers": reviewer_usernames,
            })),
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Forge for ForgejoForge {
    fn project_id(&self) -> &str {
        &self.target_project_id
    }

    fn source_project_id(&self) -> &str {
        &self.source_project_id
    }

    fn target_project_id(&self) -> &str {
        &self.target_project_id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url, self.target_project_id)
    }

    async fn current_user(&self) -> Result<ForgeUser> {
        let user: ForgejoUser = self.request(Method::GET, "/user", None::<()>).await?;
        Ok(user.into())
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>> {
        match self
            .request::<ForgejoUser>(
                Method::GET,
                format!("/users/{}", urlencoding::encode(username)),
                None::<()>,
            )
            .await
        {
            Ok(user) => Ok(Some(user.into())),
            Err(Error::ForgejoApi { message }) if message.contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>> {
        let user = self.current_user().await?.username;

        // Forgejo doesn't support filtering by head branch in the API yet,
        // so we fetch all open PRs and filter client-side
        // See https://codeberg.org/api/swagger#/repository/repoListPullRequests

        let mut page = 1;
        let limit = 25;

        loop {
            let prs: Vec<PullRequest> = self
                .request(
                    Method::GET,
                    format!(
                        "/repos/{}/{}/pulls?state=open&poster={}&page={}&limit={}",
                        self.target_owner,
                        self.target_repo,
                        urlencoding::encode(&user),
                        page,
                        limit
                    ),
                    None::<()>,
                )
                .await?;

            let has_more = prs.len() == limit;

            if let Some(pr) = prs.into_iter().find(|pr| pr.head.ref_name == branch) {
                return Ok(Some(ForgeMergeRequest::Forgejo(pr)));
            }

            if !has_more {
                break;
            }

            page += 1;
        }

        Ok(None)
    }

    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest> {
        let url = format!("/repos/{}/{}/pulls", self.target_owner, self.target_repo);

        // For fork workflows, head needs to be "owner:branch"
        let head = if self.source_project_id != self.target_project_id {
            format!("{}:{}", self.source_owner, options.source_branch)
        } else {
            options.source_branch.clone()
        };

        let mut payload = serde_json::json!({
            "title": options.title,
            "head": head,
            "base": options.target_branch,
        });

        if let Some(desc) = options.description {
            payload["body"] = serde_json::json!(desc);
        }

        let pr: PullRequest = self.request(Method::POST, url, Some(payload)).await?;

        if let Some(assignees) = options.assignee_ids
            && !assignees.is_empty()
        {
            self.add_assignees(pr.number, assignees).await?;
        }

        if let Some(reviewers) = options.reviewer_ids
            && !reviewers.is_empty()
        {
            self.request_reviewers(pr.number, reviewers).await?;
        }

        Ok(ForgeMergeRequest::Forgejo(pr))
    }

    async fn update_merge_request_base(
        &self,
        pr_number: &str,
        new_base: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, pr_number
                ),
                Some(serde_json::json!({
                    "base": new_base,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::Forgejo(pr))
    }

    async fn update_merge_request_description(
        &self,
        pr_number: &str,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, pr_number
                ),
                Some(serde_json::json!({
                    "body": new_description,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::Forgejo(pr))
    }

    async fn get_merge_request(&self, pr_number: &str) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, pr_number
                ),
                None::<()>,
            )
            .await?;

        Ok(ForgeMergeRequest::Forgejo(pr))
    }

    async fn get_approval_status(&self, pr_number: &str) -> Result<crate::forge::ApprovalStatus> {
        let reviews: Result<Vec<PullReview>, _> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/{}/pulls/{}/reviews",
                    self.target_owner, self.target_repo, pr_number
                ),
                None::<()>,
            )
            .await;

        let reviews = match reviews {
            Ok(reviews) => reviews,
            Err(_) => {
                // If we can't fetch reviews (e.g., feature not available), return Unknown
                return Ok(crate::forge::ApprovalStatus {
                    approved_count: 0,
                    required_count: 0,
                    blocking_count: 0,
                    satisfaction: crate::forge::ApprovalSatisfaction::Unknown,
                });
            }
        };

        // Group reviews by user, keeping only the most recent review from each user
        let mut user_reviews: HashMap<u64, &PullReview> = HashMap::new();

        for review in &reviews {
            if review.dismissed {
                continue;
            }

            // Keep the most recent review by comparing submitted_at
            if let Some(existing) = user_reviews.get(&review.user.id) {
                if let (Some(new_time), Some(existing_time)) =
                    (&review.submitted_at, &existing.submitted_at)
                    && new_time > existing_time
                {
                    user_reviews.insert(review.user.id, review);
                }
            } else {
                user_reviews.insert(review.user.id, review);
            }
        }

        let approved_count = user_reviews
            .values()
            .filter(|review| review.state == ReviewStateType::Approved)
            .count() as u32;

        let blocking_count = user_reviews
            .values()
            .filter(|review| review.state == ReviewStateType::RequestChanges)
            .count() as u32;

        // Forgejo doesn't expose required approval count via API
        Ok(crate::forge::ApprovalStatus {
            approved_count,
            required_count: 0,
            blocking_count,
            satisfaction: crate::forge::ApprovalSatisfaction::Unknown,
        })
    }

    async fn get_check_status(&self, pr_number: &str) -> Result<crate::forge::CheckStatus> {
        let pr = self.get_merge_request(pr_number).await?;
        let head_branch = pr.source_branch();

        // Get combined status for the head branch
        let response = self
            .client
            .request(
                Method::GET,
                format!(
                    "{}/api/v1/repos/{}/{}/commits/{}/status",
                    self.base_url,
                    self.target_owner,
                    self.target_repo,
                    urlencoding::encode(head_branch)
                ),
            )
            .header("Authorization", format!("Bearer {}", &self.token))
            .header("Accept", "application/json")
            .header("User-Agent", "jj-vine")
            .send()
            .await?;

        // Handle different status codes
        match response.status() {
            reqwest::StatusCode::OK => {
                let status: CombinedStatus = response.json().await?;

                if status.total_count == 0 {
                    return Ok(crate::forge::CheckStatus::None);
                }

                match status.state {
                    CommitStatusState::Success => Ok(crate::forge::CheckStatus::Success),
                    CommitStatusState::Pending => Ok(crate::forge::CheckStatus::Pending),
                    CommitStatusState::Error | CommitStatusState::Failure => {
                        Ok(crate::forge::CheckStatus::Failed)
                    }
                    CommitStatusState::Unknown => Ok(crate::forge::CheckStatus::None),
                }
            }
            reqwest::StatusCode::NOT_FOUND => Ok(crate::forge::CheckStatus::None),
            status => Err(Error::ForgejoApi {
                message: format!("Failed to get commit status: {}", status),
            }),
        }
    }

    async fn get_merge_request_status(&self, pr_number: &str) -> Result<MergeRequestStatus> {
        let (approval_status, check_status) = try_join!(
            self.get_approval_status(pr_number),
            self.get_check_status(pr_number),
        )?;

        Ok(MergeRequestStatus {
            iid: pr_number.to_string(),
            approval_status,
            check_status,
        })
    }
}

impl FormatMergeRequest for ForgejoForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("#{}", mr_iid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forgejo_client_new() {
        let forge = ForgejoForge::new(
            "https://codeberg.org".to_string(),
            "owner/repo".to_string(),
            "owner/repo".to_string(),
            "test-token".to_string(),
            None::<&str>,
            false,
        );

        assert!(forge.is_ok());
        let forge = forge.unwrap();
        assert_eq!(forge.base_url, "https://codeberg.org");
        assert_eq!(forge.source_project_id, "owner/repo");
        assert_eq!(forge.target_project_id, "owner/repo");
        assert_eq!(forge.token, "test-token");
        assert_eq!(forge.source_owner, "owner");
        assert_eq!(forge.source_repo, "repo");
        assert_eq!(forge.target_owner, "owner");
        assert_eq!(forge.target_repo, "repo");
    }

    #[test]
    fn test_forgejo_client_new_with_trailing_slash() {
        let forge = ForgejoForge::new(
            "https://codeberg.org/".to_string(),
            "owner/repo".to_string(),
            "owner/repo".to_string(),
            "test-token".to_string(),
            None::<&str>,
            false,
        );

        assert!(forge.is_ok());
        let forge = forge.unwrap();
        // Trailing slash should be removed
        assert_eq!(forge.base_url, "https://codeberg.org");
    }
}
