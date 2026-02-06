use std::{borrow::Cow, collections::HashMap, path::Path};

use futures::try_join;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{ConfigSnafu, Error, ForgejoApiSnafu, Result},
    forge::{
        ApprovalSatisfaction,
        ApprovalStatus,
        CheckStatus,
        DiscussionCount,
        Forge,
        ForgeCreateMergeRequestOptions,
        ForgeMergeRequest,
        ForgeMergeRequestState,
        ForgeUser,
        MergeRequestStatus,
        UserId,
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

    /// Prefix to add for draft merge requests. Configurable per-repository in
    /// Forgejo.
    wip_prefix: String,
}

/// Forgejo/Gitea user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoUser {
    /// User ID
    pub id: u64,

    /// Username (login)
    pub login: String,
}

impl ForgeUser for ForgejoUser {
    fn id(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(self.id.to_string()))
    }

    fn username(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.login))
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

#[derive(Debug, Clone)]
pub struct ForgejoMergeRequest {
    pub pull_request: PullRequest,

    wip_prefix: String,
}

impl ForgeMergeRequest for ForgejoMergeRequest {
    type User = ForgejoUser;

    type Id = u64;

    fn iid(&self) -> Self::Id {
        self.pull_request.number
    }

    fn title(&self) -> &str {
        &self.pull_request.title
    }

    fn description(&self) -> &str {
        self.pull_request.body.as_deref().unwrap_or_default()
    }

    fn source_branch(&self) -> &str {
        &self.pull_request.head.ref_name
    }

    fn target_branch(&self) -> &str {
        &self.pull_request.base.ref_name
    }

    fn state(&self) -> ForgeMergeRequestState {
        if self.pull_request.merged {
            ForgeMergeRequestState::Merged
        } else if self.pull_request.state == "open" {
            ForgeMergeRequestState::Open
        } else {
            ForgeMergeRequestState::Closed
        }
    }

    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.pull_request.html_url)
    }

    fn edit_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.pull_request.html_url)
    }

    fn author_username(&self) -> &str {
        &self.pull_request.user.login
    }

    fn created_at(&self) -> jiff::Timestamp {
        self.pull_request
            .created_at
            .parse()
            .expect("Failed to parse creation date as ISO 8601")
    }

    fn assignees(&self) -> Vec<Self::User> {
        self.pull_request.assignees.clone().unwrap_or_default()
    }

    fn reviewers(&self) -> Vec<Self::User> {
        self.pull_request
            .requested_reviewers
            .clone()
            .unwrap_or_default()
    }

    fn is_draft(&self) -> bool {
        self.pull_request.title.starts_with(&self.wip_prefix)
    }

    fn clone_boxed(
        &self,
    ) -> Box<dyn ForgeMergeRequest<User = Self::User, Id = Self::Id> + Send + Sync>
    where
        Self: Sync + Send,
    {
        Box::new(self.clone())
    }
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
        wip_prefix: String,
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

        // Strip trailing slashes from base_url to avoid double slashes in constructed
        // URLs
        let base_url = base_url.into().trim_end_matches('/').to_string();

        let source_project_id = source_project_id.into();
        let target_project_id = target_project_id.into();

        let source_project_id_clone = source_project_id.clone();
        let (source_owner, source_repo) = source_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid source project ID: {}", source_project_id),
            }
            .build(),
        )?;

        let target_project_id_clone = target_project_id.clone();
        let (target_owner, target_repo) = target_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid target project ID: {}", target_project_id),
            }
            .build(),
        )?;

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
            wip_prefix,
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
            return Err(ForgejoApiSnafu {
                message: format!("Failed request: {} - {}", status, text),
                status,
            }
            .build());
        }

        let status = response.status();
        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| {
            ForgejoApiSnafu {
                message: format!(
                    "Failed to parse response to {}: {}, response: {}",
                    path.as_ref(),
                    e,
                    body
                ),
                status,
            }
            .build()
        })?;
        Ok(data)
    }
}

impl Forge for ForgejoForge {
    type User = ForgejoUser;

    type MergeRequest = ForgejoMergeRequest;

    type UserId = UserId<u64>;

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

    async fn current_user(&self) -> Result<Self::User> {
        let user: ForgejoUser = self.request(Method::GET, "/user", None::<()>).await?;
        Ok(user)
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>> {
        match self
            .request::<ForgejoUser>(
                Method::GET,
                format!("/users/{}", urlencoding::encode(username)),
                None::<()>,
            )
            .await
        {
            Ok(user) => Ok(Some(user)),
            Err(Error::ForgejoApi { message, .. }) if message.contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        let user = &self.current_user().await?.login;

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
                        urlencoding::encode(user),
                        page,
                        limit
                    ),
                    None::<()>,
                )
                .await?;

            let has_more = prs.len() == limit;

            if let Some(pr) = prs.into_iter().find(|pr| pr.head.ref_name == branch) {
                return Ok(Some(ForgejoMergeRequest {
                    pull_request: pr,
                    wip_prefix: self.wip_prefix.clone(),
                }));
            }

            if !has_more {
                break;
            }

            page += 1;
        }

        Ok(None)
    }

    async fn find_merge_request_by_source_branch_base_branch(
        &self,
        source_branch: &str,
        base_branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        // Forgejo has an efficient implementation if you know both source and base
        // branch names.
        let pr: Result<PullRequest> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/{}/pulls/{}/{}",
                    self.target_owner,
                    self.target_repo,
                    urlencoding::encode(base_branch),
                    urlencoding::encode(source_branch),
                ),
                None::<()>,
            )
            .await;

        match pr {
            Ok(pr) => Ok(Some(ForgejoMergeRequest {
                pull_request: pr,
                wip_prefix: self.wip_prefix.clone(),
            })),
            Err(Error::ForgejoApi {
                status: StatusCode::NOT_FOUND,
                ..
            }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn create_merge_request(
        &self,
        ForgeCreateMergeRequestOptions {
            assignees,
            description,
            reviewers,
            source_branch,
            target_branch,
            title,
            open_as_draft,
            // Forge supports this as a default repository setting, but not a per-pull-request
            // setting.
            remove_source_branch: _remove_source_branch,
            // Forge supports this as a default repository setting, but not a per-pull-request
            // setting.
            squash: _squash,
        }: ForgeCreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        let head = if self.source_project_id != self.target_project_id {
            format!("{}:{}", self.source_owner, source_branch)
        } else {
            source_branch.clone()
        };

        let mut payload = serde_json::json!({
            // Not exactly documented, but Forgejo detects this based on a repository-configurable prefix.
            "title": if open_as_draft { format!("{}{}", self.wip_prefix, title) } else { title },
            "head": head,
            "base": target_branch,
        });

        if let Some(desc) = description {
            payload["body"] = serde_json::json!(desc);
        }

        let pr: PullRequest = self
            .request(
                Method::POST,
                format!("/repos/{}/{}/pulls", self.target_owner, self.target_repo),
                Some(payload),
            )
            .await?;

        if !assignees.is_empty() {
            self.add_assignees(
                pr.number,
                assignees.into_iter().map(|user| user.0).collect(),
            )
            .await?;
        }

        if !reviewers.is_empty() {
            self.request_reviewers(
                pr.number,
                reviewers.into_iter().map(|user| user.0).collect(),
            )
            .await?;
        }

        Ok(ForgejoMergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn update_merge_request_base(
        &self,
        pr_number: u64,
        new_base: &str,
    ) -> Result<Self::MergeRequest> {
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

        Ok(ForgejoMergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn update_merge_request_info(
        &self,
        pr_number: u64,
        new_description: &str,
        new_title: &str,
    ) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, pr_number
                ),
                Some(serde_json::json!({
                    "title": new_title,
                    "body": new_description,
                })),
            )
            .await?;

        Ok(ForgejoMergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn get_merge_request(&self, pr_number: u64) -> Result<Self::MergeRequest> {
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

        Ok(ForgejoMergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn get_approval_status(&self, pr_number: u64) -> Result<ApprovalStatus> {
        let reviews = self.pull_request_reviews(pr_number).await;

        let reviews = match reviews {
            Ok(reviews) => reviews,
            Err(_) => {
                // Can't access reviews, fall back to unknown
                return Ok(Default::default());
            }
        };

        // Group reviews by user, keeping only the most recent review from each user
        let mut user_reviews: HashMap<u64, &PullRequestReview> = HashMap::new();

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
        Ok(ApprovalStatus {
            approved_count,
            required_count: 0,
            blocking_count,
            satisfaction: ApprovalSatisfaction::Unknown,
        })
    }

    async fn get_check_status(&self, pr_number: u64) -> Result<CheckStatus> {
        let pr = self.get_merge_request(pr_number).await?;
        let head_branch = pr.pull_request.head.ref_name;

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
                    urlencoding::encode(&head_branch)
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
                    return Ok(CheckStatus::None);
                }

                match status.state {
                    CommitStatusState::Success => Ok(CheckStatus::Success),
                    CommitStatusState::Pending => Ok(CheckStatus::Pending),
                    CommitStatusState::Error | CommitStatusState::Failure => {
                        Ok(CheckStatus::Failed)
                    }
                    CommitStatusState::Unknown => Ok(CheckStatus::None),
                }
            }
            reqwest::StatusCode::NOT_FOUND => Ok(CheckStatus::None),
            status => Err(ForgejoApiSnafu {
                message: format!("Failed to get commit status: {}", status),
                status,
            }
            .build()),
        }
    }

    async fn get_merge_request_status(&self, pr_number: u64) -> Result<MergeRequestStatus> {
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

    async fn num_open_discussions(&self, pr_number: u64) -> Result<DiscussionCount> {
        let reviews = self.pull_request_reviews(pr_number).await?;

        // Let's not overload the API by default
        let mut comments = Vec::new();
        for review in reviews {
            let review_comments = self.pull_request_comments(pr_number, review.id).await?;
            comments.push((review, review_comments));
        }

        Ok(comments.iter().fold(
            DiscussionCount {
                all: 0,
                unresolved: 0,
                resolved: 0,
            },
            |mut acc, (_review, comments)| {
                acc.all += comments.len() as u32;

                for comment in comments {
                    if comment.resolver.is_some() {
                        acc.unresolved += 1;
                    } else {
                        acc.resolved += 1;
                    }
                }

                acc
            },
        ))
    }

    async fn sync_dependent_merge_requests(
        &self,
        _merge_request_iid: u64,
        _dependent_merge_request_iids: &[u64],
    ) -> Result<bool> {
        // Only supported for GitLab
        Ok(false)
    }
}

impl ForgejoForge {
    async fn add_assignees(&self, pr_number: u64, assignees: Vec<u64>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/{}/issues/{}/assignees",
                self.target_owner, self.target_repo, pr_number
            ),
            Some(serde_json::json!({
                "assignees": assignees,
            })),
        )
        .await?;
        Ok(())
    }

    async fn request_reviewers(&self, pr_number: u64, reviewers: Vec<u64>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/{}/pulls/{}/requested_reviewers",
                self.target_owner, self.target_repo, pr_number
            ),
            Some(serde_json::json!({
                "reviewers": reviewers,
            })),
        )
        .await?;
        Ok(())
    }

    async fn pull_request_reviews(&self, pr_number: u64) -> Result<Vec<PullRequestReview>> {
        // TODO pagination, max 100 reviews right now
        self.request(
            Method::GET,
            format!(
                "/repos/{}/{}/pulls/{}/reviews?page=1&limit=100",
                self.target_owner, self.target_repo, pr_number
            ),
            None::<()>,
        )
        .await
    }

    async fn pull_request_comments(&self, pr_number: u64, review_id: u64) -> Result<Vec<Comment>> {
        self.request(
            Method::GET,
            format!(
                "/repos/{}/{}/pulls/{}/reviews/{}/comments",
                self.target_owner, self.target_repo, pr_number, review_id
            ),
            None::<()>,
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PullRequestReview {
    body: String,
    comments_count: u64,
    commit_id: String,
    dismissed: bool,
    html_url: String,
    id: u64,
    official: bool,
    pull_request_url: String,
    stale: bool,
    state: ReviewStateType,
    submitted_at: Option<String>,
    user: ForgejoUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Comment {
    body: String,
    commit_id: String,
    created_at: String,
    diff_hunk: String,
    html_url: String,
    id: u64,
    original_commit_id: String,
    original_position: u64,
    path: String,
    position: u64,
    pull_request_review_id: u64,
    pull_request_url: String,
    resolver: Option<ForgejoUser>,
    updated_at: String,
    user: ForgejoUser,
}

impl FormatMergeRequest for ForgejoForge {
    type Id = u64;

    fn format_merge_request_id(&self, mr_iid: Self::Id) -> String {
        format!("#{}", mr_iid)
    }

    fn mr_name(&self) -> &'static str {
        "PR"
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
            "WIP: ".to_string(),
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
            "WIP: ".to_string(),
        );

        assert!(forge.is_ok());
        let forge = forge.unwrap();
        // Trailing slash should be removed
        assert_eq!(forge.base_url, "https://codeberg.org");
    }
}
