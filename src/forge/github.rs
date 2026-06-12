mod graphql;

use std::{borrow::Cow, collections::HashMap, path::Path};

use futures::{join, try_join};
use itertools::Itertools as _;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::debug;

use crate::{
    bookmark::BookmarkRef,
    description::FormatMergeRequest,
    error::{ConfigSnafu, Error, GitHubApiSnafu, Result},
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
        github::graphql::find_pr_by_head_ref::PRNode,
    },
    utils::ResultWithWarnings,
};

/// GitHub REST API client.
pub struct GitHubForge {
    base_url: String,
    source_project_id: String,
    target_project_id: String,
    token: String,
    client: reqwest::Client,
}

/// GitHub user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    /// User ID.
    pub id: u64,

    /// Username (login).
    pub login: String,
}

impl UserLike for GitHubUser {
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

    /// Commit SHA.
    pub sha: String,

    /// Repository information (for cross-repo PRs).
    pub repo: Option<GitHubRepo>,
}

/// GitHub repository info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    /// Repository full name (owner/repo).
    pub full_name: String,
}

/// GitHub Pull Request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// PR number (GitHub's equivalent to GitLab's IID).
    pub number: u64,

    /// PR ID (unique across GitHub).
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
    pub user: GitHubUser,

    /// Created at timestamp (ISO 8601).
    pub created_at: String,

    /// Assignees of the PR.
    pub assignees: Vec<GitHubUser>,

    /// Requested reviewers (GitHub-specific).
    pub requested_reviewers: Vec<GitHubUser>,

    /// Draft status.
    pub draft: bool,

    /// Whether the PR was merged (only present in individual PR fetch, not
    /// list).
    #[serde(default)]
    pub merged: bool,
}

impl MergeRequestLike for PullRequest {
    type User = GitHubUser;

    type Id = u64;

    fn iid(&self) -> Self::Id {
        self.number
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        self.body.as_deref().unwrap_or_default()
    }

    fn source_branch(&self) -> &str {
        &self.head.ref_name
    }

    fn target_branch(&self) -> &str {
        &self.base.ref_name
    }

    fn state(&self) -> MergeRequestState {
        if self.merged {
            MergeRequestState::Merged
        } else if self.state == "open" {
            MergeRequestState::Open
        } else {
            MergeRequestState::Closed
        }
    }

    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.html_url)
    }

    fn edit_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.html_url)
    }

    fn author_username(&self) -> &str {
        &self.user.login
    }

    fn created_at(&self) -> jiff::Timestamp {
        self.created_at
            .parse()
            .expect("Failed to parse creation date as ISO 8601")
    }

    fn assignees(&self) -> Vec<Self::User> {
        self.assignees.clone()
    }

    fn reviewers(&self) -> Vec<Self::User> {
        self.requested_reviewers.clone()
    }

    fn is_draft(&self) -> bool {
        self.draft
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

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ReviewState {
    #[serde(rename = "APPROVED")]
    Approved,

    #[serde(rename = "CHANGES_REQUESTED")]
    ChangesRequested,

    #[serde(rename = "COMMENTED")]
    Commented,

    #[serde(rename = "DISMISSED")]
    Dismissed,

    #[serde(rename = "PENDING")]
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Review {
    /// Review ID.
    pub id: u64,

    /// User who submitted the review.
    pub user: GitHubUser,

    /// Review body/comment.
    pub body: Option<String>,

    /// The state of the review.
    pub state: ReviewState,

    /// HTML URL to view the review.
    pub html_url: String,

    /// Submitted at timestamp (ISO 8601).
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum BranchRuleType {
    PullRequest,

    #[serde(other)]
    Unknown,
}

/// GitHub Branch Rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchRule {
    /// Rule type.
    #[serde(rename = "type")]
    pub rule_type: BranchRuleType,

    /// Rule parameters (contains `required_approving_review_count` for
    /// `pull_request` rule).
    #[serde(default)]
    pub parameters: Option<BranchRuleParameters>,
}

/// Parameters for branch rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchRuleParameters {
    /// Required approving review count (for `pull_request` rules).
    #[serde(default)]
    pub required_approving_review_count: Option<u32>,
}

/// Response from listing check runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckRunsResponse {
    /// Total count of check runs.
    pub total_count: u32,

    /// List of check runs.
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CheckRunStatus {
    Queued,
    InProgress,
    Completed,
    Waiting,
    Requested,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CheckRunConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
}

/// GitHub Check Run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckRun {
    /// Check run ID.
    pub id: u64,

    /// Check run name.
    pub name: String,

    /// Status of the check run.
    pub status: CheckRunStatus,

    /// Conclusion (only present when status is completed).
    pub conclusion: Option<CheckRunConclusion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GraphQLResponse<T> {
    data: T,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GraphQLError {
    message: String,
}

impl GitHubForge {
    /// Create a new GitHub client.
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

        Ok(Self {
            base_url,
            source_project_id: source_project_id.into(),
            target_project_id: target_project_id.into(),
            token: token.into(),
            client,
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
        let mut req = self
            .client
            .request(method, format!("{}{}", self.base_url, path.as_ref()))
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "jj-vine");

        if let Some(payload) = payload.as_ref() {
            req = req.json(payload);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(GitHubApiSnafu {
                message: format!("Failed to get: {status} - {text}"),
            }
            .build());
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| {
            GitHubApiSnafu {
                message: format!(
                    "Failed to parse GET response to {}: {}, response: {}",
                    path.as_ref(),
                    e,
                    body
                ),
            }
            .build()
        })?;
        Ok(data)
    }

    async fn graphql<T>(&self, query: &str, variables: impl Serialize) -> Result<T>
    where
        T: DeserializeOwned,
    {
        // TODO use a real graphql client
        let graphql_url = if self.base_url.starts_with("https://api.github.com") {
            "https://api.github.com/graphql".to_owned()
        } else if self.base_url.contains("/api/v3") {
            self.base_url.replace("/api/v3", "/api/graphql")
        } else {
            format!("{}/graphql", self.base_url)
        };

        let payload = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .client
            .post(&graphql_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "jj-vine")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(GitHubApiSnafu {
                message: format!("GraphQL request failed: {status} - {text}"),
            }
            .build());
        }

        let body = response.text().await?;
        let data: GraphQLResponse<T> = serde_json::from_str(&body).map_err(|e| {
            GitHubApiSnafu {
                message: format!("Failed to parse GraphQL response: {e}, response: {body}"),
            }
            .build()
        })?;

        if let Some(errors) = data.errors {
            return Err(GitHubApiSnafu {
                message: format!(
                    "GraphQL request failed: {}",
                    errors
                        .iter()
                        .map(|error| error.message.clone())
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
            }
            .build());
        }

        Ok(data.data)
    }

    async fn add_assignees(&self, pr_number: u64, assignees: Vec<u64>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/issues/{}/assignees",
                self.target_project_id, pr_number
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
                "/repos/{}/pulls/{}/requested_reviewers",
                self.target_project_id, pr_number
            ),
            Some(serde_json::json!({
                "reviewers": reviewers,
            })),
        )
        .await?;
        Ok(())
    }

    /// Get required approval count from branch protection rules.
    async fn get_required_approvals(&self, branch: &str) -> Result<u32> {
        let rules: Vec<BranchRule> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/rules/branches/{}",
                    self.target_project_id,
                    urlencoding::encode(branch)
                ),
                None::<()>,
            )
            .await?;

        for rule in rules {
            if rule.rule_type == BranchRuleType::PullRequest
                && let Some(params) = rule.parameters
                && let Some(count) = params.required_approving_review_count
            {
                return Ok(count);
            }
        }

        Ok(0)
    }

    async fn get_discussions(
        &self,
        pr_number: u64,
    ) -> Result<Vec<graphql::GetDiscussionsQueryComment>> {
        let (owner, name) = split_project_id(&self.target_project_id)?;

        // TODO pagination, real gql client
        let response: graphql::GetDiscussionsQueryResponse = self
            .graphql(
                "
query GetDiscussions($owner: String!, $name: String!, $pr_number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $pr_number) {
      reviews(first: 100) {
        nodes {
          id
          comments(first: 100) {
            nodes {
              author {
                login
              }
              body
              createdAt
              editor {
                login
              }
              id
              lastEditedAt
              isMinimized
              minimizedReason
              publishedAt
              updatedAt
              url
              viewerCanMinimize
            }
          }
        }
      }

      comments(first: 100) {
        nodes {
          author {
            login
          }
          body
          createdAt
          editor {
            login
          }
          id
          lastEditedAt
          isMinimized
          minimizedReason
          publishedAt
          updatedAt
          url
          viewerCanMinimize
        }
      }
    }
  }
}
        ",
                serde_json::json!({
                    "owner": owner,
                    "name": name,
                    "pr_number": pr_number,
                }),
            )
            .await?;

        let pull_request = response
            .repository
            .ok_or(
                GitHubApiSnafu {
                    message: format!("Repository {} not found", self.target_project_id),
                }
                .build(),
            )?
            .pull_request
            .ok_or(
                GitHubApiSnafu {
                    message: format!("Pull request {pr_number} not found"),
                }
                .build(),
            )?;

        let root_comments = &pull_request.comments.nodes;

        let reviews = &pull_request.reviews.unwrap_or_default().nodes;
        let review_comments = reviews
            .iter()
            .flat_map(|review| review.comments.nodes.iter())
            .collect::<Vec<_>>();

        Ok(root_comments
            .iter()
            .chain(review_comments)
            .cloned()
            .collect())
    }

    fn project_url_from_id(&self, project_id: &str) -> String {
        let base_url = if self.base_url.starts_with("https://api.github.com") {
            "https://github.com"
        } else if self.base_url.contains("/api/v3") {
            self.base_url.trim_end_matches("/api/v3")
        } else {
            &self.base_url
        };
        format!("{base_url}/{project_id}")
    }
}

impl Forge for GitHubForge {
    type User = GitHubUser;

    type MergeRequest = PullRequest;

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
        self.project_url_from_id(&self.target_project_id)
    }

    async fn current_user(&self) -> Result<Self::User> {
        let user: GitHubUser = self.request(Method::GET, "/user", None::<()>).await?;
        Ok(user)
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>> {
        match self
            .request::<GitHubUser>(Method::GET, format!("/users/{username}"), None::<()>)
            .await
        {
            Ok(user) => Ok(Some(user)),
            Err(Error::GitHubApi { message, .. }) if message.contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        let (target_owner, target_name) = split_project_id(&self.target_project_id)?;

        debug!(
            branch,
            target_project = %self.target_project_id,
            source_project = %self.source_project_id,
            "Looking up PR by source branch via GraphQL"
        );

        let response: graphql::find_pr_by_head_ref::Response = self
            .graphql(
                graphql::find_pr_by_head_ref::query(),
                serde_json::json!({
                    "owner": target_owner,
                    "repositoryName": target_name,
                    "headRefName": branch,
                }),
            )
            .await?;

        let prs: Vec<_> = response
            .repository
            .into_iter()
            .flat_map(|r| r.pull_requests.nodes)
            .filter(|pr| {
                pr.head_repository.as_ref().is_some_and(|r| {
                    r.name_with_owner
                        .eq_ignore_ascii_case(&self.source_project_id)
                })
            })
            .collect();

        debug!(
            count = prs.len(),
            source_project = %self.source_project_id,
            "PR lookup result (filtered by head repository)"
        );

        Ok(prs.into_iter().next().map(PRNode::into_pull_request))
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

            // In GitHub, removing the source branch is handled at *merge time*. Default is a
            // project setting only.
            remove_source_branch: _remove_source_branch,

            // In GitHub, squashing is handled at *merge time*. Default is a project setting only.
            squash: _squash,
        }: CreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            title: String,
            head: String,
            head_repo: String,
            base: String,
            draft: bool,

            #[serde(skip_serializing_if = "Option::is_none")]
            #[expect(clippy::struct_field_names, reason = "serialized")]
            body: Option<String>,
        }

        // head_repo in "owner/repo" format tells GitHub which repository the
        // branch lives in, handling same-repo, same-org fork, and cross-org
        // fork cases uniformly.
        let head_repo = &self.source_project_id;

        let payload = Body {
            title,
            head: source_branch,
            head_repo: head_repo.clone(),
            base: target_branch,
            draft: open_as_draft,
            body: description,
        };

        let pr: PullRequest = self
            .request(
                Method::POST,
                format!("/repos/{}/pulls", self.target_project_id),
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

        Ok(pr)
    }

    async fn update_merge_request_base(
        &self,
        merge_request_iid: Self::Id,
        new_base: &str,
    ) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/pulls/{}",
                    self.target_project_id, merge_request_iid
                ),
                Some(serde_json::json!({
                    "base": new_base,
                })),
            )
            .await?;

        Ok(pr)
    }

    async fn update_merge_request_info(
        &self,
        merge_request_iid: Self::Id,
        UpdateMergeRequestInfoOptions {
            title,
            description,
            draft,
            current_title: _current_title,       // Unneeded for GitHub
            current_is_draft: _current_is_draft, // Unneeded for GitHub
        }: UpdateMergeRequestInfoOptions,
    ) -> Result<Self::MergeRequest> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            #[serde(skip_serializing_if = "Option::is_none")]
            title: Option<String>,

            #[serde(skip_serializing_if = "Option::is_none")]
            body: Option<String>,
        }

        let body = Body {
            title,
            body: description,
        };

        let mut pr: PullRequest = self
            .request(
                Method::PATCH,
                format!(
                    "/repos/{}/pulls/{}",
                    self.target_project_id, merge_request_iid
                ),
                Some(body),
            )
            .await?;

        match draft {
            Some(true) => {
                let response: graphql::set_pr_is_draft::Response = self
                    .graphql(
                        graphql::set_pr_is_draft::query(),
                        graphql::set_pr_is_draft::Variables {
                            id: pr.id.to_string(),
                        },
                    )
                    .await?;
                pr.draft = response.convert_pull_request_to_draft.pull_request.is_draft;
            }
            Some(false) => {
                let response: graphql::set_pr_not_draft::Response = self
                    .graphql(
                        graphql::set_pr_not_draft::query(),
                        graphql::set_pr_not_draft::Variables {
                            id: pr.id.to_string(),
                        },
                    )
                    .await?;
                pr.draft = response.convert_pull_request_to_draft.pull_request.is_draft;
            }
            None => {}
        }

        Ok(pr)
    }

    async fn get_merge_request(&self, merge_request_iid: Self::Id) -> Result<Self::MergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/pulls/{}",
                    self.target_project_id, merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(pr)
    }

    async fn get_approval_status(&self, merge_request_iid: Self::Id) -> Result<ApprovalStatus> {
        let pr = self.get_merge_request(merge_request_iid).await?;
        let base_branch = pr.base.ref_name;

        let (reviews, required_count): (Result<Vec<Review>, _>, _) = join!(
            self.request(
                Method::GET,
                format!(
                    "/repos/{}/pulls/{}/reviews",
                    self.target_project_id, merge_request_iid
                ),
                None::<()>,
            ),
            self.get_required_approvals(&base_branch),
        );

        let user_reviews: Result<HashMap<u64, ReviewState>, _> = reviews.map(|reviews| {
            reviews
                .iter()
                .filter(|review| review.submitted_at.is_some())
                .sorted_by_key(|review| review.submitted_at.as_ref().unwrap())
                .map(|review| (review.user.id, review.state.clone()))
                .collect()
        });

        let approved_count = user_reviews.as_ref().map(|reviews| {
            reviews
                .values()
                .filter(|state| matches!(state, ReviewState::Approved))
                .count()
                .try_into()
                .expect("too large")
        });

        let blocking_count = user_reviews.as_ref().map(|reviews| {
            reviews
                .values()
                .filter(|state| matches!(state, ReviewState::ChangesRequested))
                .count()
                .try_into()
                .expect("too large")
        });

        Ok(ApprovalStatus {
            approved_count: approved_count.unwrap_or(0),
            required_count: *required_count.as_ref().unwrap_or(&0),
            blocking_count: blocking_count.unwrap_or(0),
            satisfaction: match (required_count, approved_count) {
                (Ok(count), Ok(approved_count)) => {
                    if approved_count >= count {
                        ApprovalSatisfaction::Satisfied
                    } else {
                        ApprovalSatisfaction::Unsatisfied
                    }
                }
                (_, _) => ApprovalSatisfaction::Unknown,
            },
        })
    }

    async fn get_check_status(&self, merge_request_iid: Self::Id) -> Result<CheckStatus> {
        let head_sha = self.get_merge_request(merge_request_iid).await?.head.sha;

        let response: CheckRunsResponse = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/commits/{}/check-runs",
                    self.target_project_id,
                    urlencoding::encode(&head_sha)
                ),
                None::<()>,
            )
            .await?;

        if response.total_count == 0 {
            return Ok(CheckStatus::None);
        }

        let mut has_pending = false;
        let mut has_failed = false;

        for check_run in response.check_runs {
            match (check_run.status, check_run.conclusion) {
                (
                    CheckRunStatus::Completed,
                    Some(
                        CheckRunConclusion::Failure
                        | CheckRunConclusion::Cancelled
                        | CheckRunConclusion::TimedOut
                        | CheckRunConclusion::ActionRequired,
                    ),
                ) => {
                    has_failed = true;
                }
                (CheckRunStatus::Completed, _) => {}
                (
                    CheckRunStatus::Queued
                    | CheckRunStatus::InProgress
                    | CheckRunStatus::Waiting
                    | CheckRunStatus::Pending
                    | CheckRunStatus::Requested,
                    _,
                ) => {
                    has_pending = true;
                }
            }
        }

        // Return the aggregated status
        if has_failed {
            Ok(CheckStatus::Failed)
        } else if has_pending {
            Ok(CheckStatus::Pending)
        } else {
            Ok(CheckStatus::Success)
        }
    }

    async fn get_merge_request_status(
        &self,
        merge_request_iid: Self::Id,
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

    async fn num_open_discussions(&self, merge_request_iid: Self::Id) -> Result<DiscussionCount> {
        let discussions = self.get_discussions(merge_request_iid).await?;
        Ok(discussions.iter().fold(
            DiscussionCount {
                all: 0,
                unresolved: 0,
                resolved: 0,
            },
            |mut acc, comment| {
                acc.all = acc.all.strict_add(1);

                if comment.is_minimized {
                    acc.resolved = acc.resolved.strict_add(1);
                    // Close enough?
                } else if comment.viewer_can_minimize {
                    acc.unresolved = acc.unresolved.strict_add(1);
                } else {
                    // Only increment all
                }

                acc
            },
        ))
    }

    async fn sync_dependent_merge_requests(
        &self,
        _merge_request_iid: Self::Id,
        _dependent_merge_request_iids: &[Self::Id],
    ) -> ResultWithWarnings<bool> {
        // Only supported for GitLab
        Ok(false).into()
    }
}

impl FormatMergeRequest for GitHubForge {
    type Id = u64;

    fn format_merge_request_id(&self, mr_iid: Self::Id) -> String {
        format!("#{mr_iid}")
    }

    fn mr_name(&self) -> &'static str {
        "PR"
    }

    fn id_expands_title(&self) -> bool {
        true
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

        Ok(format!(
            "{}/compare/{}..{}",
            self.project_url_from_id(project_id),
            to.name().unwrap_or(default_branch),
            from.name().unwrap_or(default_branch)
        ))
    }
}

fn split_project_id(project_id: &str) -> Result<(&str, &str)> {
    project_id.split_once('/').ok_or_else(|| {
        ConfigSnafu {
            message: format!("Invalid project ID '{project_id}': expected 'owner/repo' format"),
        }
        .build()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_client_new() {
        let client = GitHubForge::new(
            "https://api.github.com".to_owned(),
            "owner/repo".to_owned(),
            "owner/repo".to_owned(),
            "ghp_token123".to_owned(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(client.base_url, "https://api.github.com");
        assert_eq!(client.source_project_id, "owner/repo");
        assert_eq!(client.target_project_id, "owner/repo");
        assert_eq!(client.token, "ghp_token123");
    }

    #[test]
    fn project_url() {
        let client = GitHubForge::new(
            "https://api.github.com".to_owned(),
            "owner/repo".to_owned(),
            "owner/repo".to_owned(),
            "token".to_owned(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(client.project_url(), "https://github.com/owner/repo");
    }

    #[test]
    fn github_enterprise_url() {
        let client = GitHubForge::new(
            "https://github.example.com/api/v3".to_owned(),
            "owner/repo".to_owned(),
            "owner/repo".to_owned(),
            "token".to_owned(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(
            client.project_url(),
            "https://github.example.com/owner/repo"
        );
    }
}
