use std::{borrow::Cow, collections::HashMap, path::Path};

use futures::try_join;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    bookmark::BookmarkRef,
    config::Config,
    description::FormatMergeRequest,
    error::{ConfigSnafu, Error, ForgejoApiSnafu, Result},
    forge::{
        ApprovalSatisfaction,
        ApprovalStatus,
        CheckStatus,
        CreateMergeRequestOptions,
        DiscussionCount,
        Forge,
        MergeRequestLike,
        MergeRequestState,
        MergeRequestStatus,
        UpdateMergeRequestInfoOptions,
        UserId,
        UserLike,
    },
    utils::ResultWithWarnings,
};

/// Forgejo/Gitea REST API client.
#[expect(clippy::module_name_repetitions, reason = "meh")]
pub struct ForgejoForge {
    base_url: String,
    source_project_id: String,
    target_project_id: String,
    source_owner: String,

    #[cfg_attr(not(test), expect(dead_code, reason = "keep it around"))]
    source_repo: String,

    target_owner: String,
    target_repo: String,
    token: String,
    client: reqwest::Client,

    /// Prefix to add for draft merge requests. Configurable per-repository in
    /// Forgejo.
    wip_prefix: String,
}

/// Forgejo/Gitea user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// User ID.
    pub id: u64,

    /// Username (login).
    pub login: String,
}

impl UserLike for User {
    fn id(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(self.id.to_string()))
    }

    fn username(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.login))
    }
}

/// Branch reference in a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRef {
    /// Branch name.
    #[serde(rename = "ref")]
    pub ref_name: String,

    /// Repository information (for cross-repo PRs).
    pub repo: Option<Repo>,
}

/// Forgejo/Gitea repository info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    /// Repository full name (owner/repo).
    pub full_name: String,
}

/// Forgejo/Gitea Pull Request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// PR number (index within repo).
    pub number: u64,

    /// PR ID (unique globally).
    pub id: u64,

    /// PR title.
    pub title: String,

    /// PR body/description.
    pub body: Option<String>,

    /// Head branch information.
    pub head: BranchRef,

    /// Base branch information.
    pub base: BranchRef,

    /// PR state (open, closed).
    pub state: String,

    /// HTML URL to view the PR.
    pub html_url: String,

    /// User who created the PR.
    pub user: User,

    /// Created at timestamp (ISO 8601).
    pub created_at: String,

    /// Assignees of the PR.
    pub assignees: Option<Vec<User>>,

    /// Requested reviewers.
    pub requested_reviewers: Option<Vec<User>>,

    /// Whether the PR was merged.
    #[serde(default)]
    pub merged: bool,
}

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub pull_request: PullRequest,

    pub wip_prefix: String,
}

impl MergeRequestLike for MergeRequest {
    type User = User;

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

    fn state(&self) -> MergeRequestState {
        if self.pull_request.merged {
            MergeRequestState::Merged
        } else if self.pull_request.state == "open" {
            MergeRequestState::Open
        } else {
            MergeRequestState::Closed
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
    ) -> Box<dyn MergeRequestLike<User = Self::User, Id = Self::Id> + Send + Sync>
    where
        Self: Sync + Send,
    {
        Box::new(self.clone())
    }
}

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

/// Commit status state (pending, success, error, failure).
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

/// Individual commit status.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommitStatus {
    /// Status state.
    pub state: CommitStatusState,
}

/// Combined status for a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CombinedStatus {
    /// Combined state across all statuses.
    pub state: CommitStatusState,

    /// Individual statuses.
    #[serde(default)]
    pub statuses: Vec<CommitStatus>,

    /// Total count of statuses.
    #[serde(default)]
    pub total_count: i64,
}

pub fn validate_config(config: &Config) -> Result<()> {
    if config.forgejo.host.is_empty() {
        return Err(ConfigSnafu {
            message: "forgejo.host is required when forge is forgejo".to_owned(),
        }
        .build());
    }
    if config.forgejo.project.is_empty() {
        return Err(ConfigSnafu {
            message: "forgejo.project is required when forge is forgejo".to_owned(),
        }
        .build());
    }
    if config.forgejo.token.is_empty() {
        return Err(ConfigSnafu {
            message: "forgejo.token is required when forge is forgejo".to_owned(),
        }
        .build());
    }

    Ok(())
}

impl ForgejoForge {
    pub fn new_from_config(config: &Config) -> Result<Self> {
        let source = config.forgejo.source_project();
        let target = config.forgejo.target_project();
        Self::new(
            config.forgejo.host.clone(),
            source.to_owned(),
            target.to_owned(),
            config.forgejo.token.clone(),
            config.ca_bundle.clone(),
            config.tls_accept_non_compliant_certs,
            config.forgejo.wip_prefix.clone(),
        )
    }

    /// Create a new Forgejo/Gitea client.
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
                    message: format!("Failed to parse CA bundle: {e}"),
                }
                .build()
            })?;

            for cert in certs {
                client_builder = client_builder.add_root_certificate(cert);
            }
        }

        let client = client_builder.build().map_err(|e| {
            ConfigSnafu {
                message: format!("Failed to build HTTP client: {e:?}"),
            }
            .build()
        })?;

        // Strip trailing slashes from base_url to avoid double slashes in constructed
        // URLs
        let base_url = base_url.into().trim_end_matches('/').to_owned();

        let source_project_id = source_project_id.into();
        let target_project_id = target_project_id.into();

        let source_project_id_clone = source_project_id.clone();
        let (source_owner, source_repo) = source_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid source project ID: {source_project_id}"),
            }
            .build(),
        )?;

        let target_project_id_clone = target_project_id.clone();
        let (target_owner, target_repo) = target_project_id_clone.split_once('/').ok_or(
            ConfigSnafu {
                message: format!("Invalid target project ID: {target_project_id}"),
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

    async fn request<T>(
        &self,
        method: Method,
        path: impl AsRef<str>,
        payload: Option<impl Serialize>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let url = format!("{}/api/v1{}", self.base_url, path.as_ref());
        let mut req = self
            .client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", self.token))
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
                message: format!("Failed request: {status} - {text}"),
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

    fn wrap_draft(&self, title: &str, draft: bool) -> String {
        let title = title.trim_start_matches(&self.wip_prefix);
        if draft {
            format!("{}{}", self.wip_prefix, title)
        } else {
            title.to_owned()
        }
    }

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

impl Forge for ForgejoForge {
    type User = User;

    type MergeRequest = MergeRequest;

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

    fn is_fork(&self) -> bool {
        self.source_project_id != self.target_project_id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url, self.target_project_id)
    }

    async fn current_user(&self) -> Result<Self::User> {
        let user: User = self.request(Method::GET, "/user", None::<()>).await?;
        Ok(user)
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>> {
        match self
            .request::<User>(
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

        let mut page = 1_i32;
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

            let is_fork = self.source_project_id != self.target_project_id;
            if let Some(pr) = prs.into_iter().find(|pr| {
                pr.head.ref_name == branch
                    && (!is_fork
                        || pr
                            .head
                            .repo
                            .as_ref()
                            .is_some_and(|r| r.full_name == self.source_project_id))
            }) {
                return Ok(Some(MergeRequest {
                    pull_request: pr,
                    wip_prefix: self.wip_prefix.clone(),
                }));
            }

            if !has_more {
                break;
            }

            page = page.strict_add(1);
        }

        Ok(None)
    }

    async fn find_merge_request_by_source_branch_base_branch(
        &self,
        source_branch: &str,
        base_branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        // Forgejo can look up a PR directly when both source and base branches
        // are known, avoiding the paginated listing needed by the branch-only
        // method. For cross-repo (fork) PRs the head parameter must use the
        // "owner:branch" format so Forgejo resolves the correct repository.
        let head = if self.source_project_id == self.target_project_id {
            source_branch.to_owned()
        } else {
            format!("{}:{source_branch}", self.source_owner)
        };

        let pr: Result<PullRequest> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/{}/pulls/{}/{}",
                    self.target_owner,
                    self.target_repo,
                    urlencoding::encode(base_branch),
                    urlencoding::encode(&head),
                ),
                None::<()>,
            )
            .await;

        match pr {
            Ok(pr) => Ok(Some(MergeRequest {
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
        CreateMergeRequestOptions {
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
        }: CreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            title: String,
            head: String,
            base: String,

            #[serde(skip_serializing_if = "Option::is_none")]
            #[expect(clippy::struct_field_names, reason = "serialized")]
            body: Option<String>,
        }

        let head = if self.source_project_id == self.target_project_id {
            source_branch.clone()
        } else {
            format!("{}:{}", self.source_owner, source_branch)
        };

        let payload = Body {
            // Not exactly documented, but Forgejo detects this based on a repository-configurable
            // prefix.
            title: self.wrap_draft(&title, open_as_draft),
            head,
            base: target_branch,
            body: description,
        };

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

        Ok(MergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn update_merge_request_base(
        &self,
        merge_request_iid: u64,
        new_base: &str,
    ) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, merge_request_iid
                ),
                Some(serde_json::json!({
                    "base": new_base,
                })),
            )
            .await?;

        Ok(MergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn update_merge_request_info(
        &self,
        merge_request_iid: u64,
        UpdateMergeRequestInfoOptions {
            title,
            description,
            draft,
            current_title,
            current_is_draft,
        }: UpdateMergeRequestInfoOptions,
    ) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, merge_request_iid
                ),
                Some(serde_json::json!({
                    "title": match (draft, title) {
                        (Some(draft), Some(title)) => Some(self.wrap_draft(&title, draft)),
                        (Some(draft), None) => Some(self.wrap_draft(&current_title, draft)),
                        (None, Some(title)) => Some(self.wrap_draft(&title, current_is_draft)),
                        (None, None) => None,
                    },
                    "body": description,
                })),
            )
            .await?;

        Ok(MergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn get_merge_request(&self, merge_request_iid: u64) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/{}/pulls/{}",
                    self.target_owner, self.target_repo, merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(MergeRequest {
            pull_request: pr,
            wip_prefix: self.wip_prefix.clone(),
        })
    }

    async fn get_approval_status(&self, merge_request_iid: u64) -> Result<ApprovalStatus> {
        let reviews = self.pull_request_reviews(merge_request_iid).await;

        let Ok(reviews) = reviews else {
            // Can't access reviews, fall back to unknown
            return Ok(ApprovalStatus::default());
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
            .count()
            .try_into()
            .expect("too large");

        let blocking_count = user_reviews
            .values()
            .filter(|review| review.state == ReviewStateType::RequestChanges)
            .count()
            .try_into()
            .expect("too large");

        // Forgejo doesn't expose required approval count via API
        Ok(ApprovalStatus {
            approved_count,
            required_count: 0,
            blocking_count,
            satisfaction: ApprovalSatisfaction::Unknown,
        })
    }

    async fn get_check_status(&self, merge_request_iid: u64) -> Result<CheckStatus> {
        let pr = self.get_merge_request(merge_request_iid).await?;
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
            .header("Authorization", format!("Bearer {}", self.token))
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
                message: format!("Failed to get commit status: {status}"),
                status,
            }
            .build()),
        }
    }

    async fn get_merge_request_status(&self, merge_request_iid: u64) -> Result<MergeRequestStatus> {
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

    async fn num_open_discussions(&self, merge_request_iid: u64) -> Result<DiscussionCount> {
        let reviews = self.pull_request_reviews(merge_request_iid).await?;

        // Let's not overload the API by default
        let mut comments = Vec::new();
        for review in reviews {
            let review_comments = self
                .pull_request_comments(merge_request_iid, review.id)
                .await?;
            comments.push((review, review_comments));
        }

        Ok(comments.iter().fold(
            DiscussionCount {
                all: 0,
                unresolved: 0,
                resolved: 0,
            },
            |mut acc, (_review, comments)| {
                acc.all = acc
                    .all
                    .strict_add(comments.len().try_into().expect("too large"));

                for comment in comments {
                    if comment.resolver.is_some() {
                        acc.unresolved = acc.unresolved.strict_add(1);
                    } else {
                        acc.resolved = acc.resolved.strict_add(1);
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
    ) -> ResultWithWarnings<bool> {
        // Only supported for GitLab
        Ok(false).into()
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
    user: User,
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
    resolver: Option<User>,
    updated_at: String,
    user: User,
}

impl FormatMergeRequest for ForgejoForge {
    type Id = u64;

    fn format_merge_request_id(&self, mr_iid: Self::Id) -> String {
        format!("#{mr_iid}")
    }

    fn mr_name(&self) -> &'static str {
        "PR"
    }

    fn mr_diff_url(
        &self,
        from: &BookmarkRef,
        to: &BookmarkRef,
        default_branch: &str,
    ) -> Result<String> {
        let project_id = if from.parent_name(default_branch) == default_branch {
            &self.target_project_id
        } else {
            &self.source_project_id
        };

        // Forgejo doesn't like `target/project:ref...source/project:ref` but it does
        // seem to be okay with `ref...source/project:ref` even if source ==
        // target
        Ok(format!(
            "{}/{}/compare/{}...{}:{}",
            self.base_url,
            project_id,
            to.name().unwrap_or(default_branch),
            self.source_project_id,
            from.name().unwrap_or(default_branch)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgejo_client_new() {
        let forge = ForgejoForge::new(
            "https://codeberg.org".to_owned(),
            "owner/repo".to_owned(),
            "owner/repo".to_owned(),
            "test-token".to_owned(),
            None::<&str>,
            false,
            "WIP: ".to_owned(),
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
    fn forgejo_client_new_with_trailing_slash() {
        let forge = ForgejoForge::new(
            "https://codeberg.org/".to_owned(),
            "owner/repo".to_owned(),
            "owner/repo".to_owned(),
            "test-token".to_owned(),
            None::<&str>,
            false,
            "WIP: ".to_owned(),
        );

        assert!(forge.is_ok());
        let forge = forge.unwrap();
        // Trailing slash should be removed
        assert_eq!(forge.base_url, "https://codeberg.org");
    }
}
