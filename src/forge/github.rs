mod graphql;

use std::{collections::HashMap, path::Path};

use futures::{join, try_join};
use itertools::Itertools;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
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

/// GitHub REST API client
pub struct GitHubForge {
    base_url: String,
    source_project_id: String,
    target_project_id: String,
    token: String,
    client: reqwest::Client,
}

/// GitHub user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    /// User ID
    pub id: u64,

    /// Username (login)
    pub login: String,
}

impl From<GitHubUser> for ForgeUser {
    fn from(user: GitHubUser) -> Self {
        ForgeUser {
            id: Some(user.id.to_string()),
            username: user.login,
        }
    }
}

/// Branch reference in a pull request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRef {
    /// Branch name
    #[serde(rename = "ref")]
    pub ref_name: String,

    /// Commit SHA
    pub sha: String,

    /// Repository information (for cross-repo PRs)
    pub repo: Option<GitHubRepo>,
}

/// GitHub repository info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    /// Repository full name (owner/repo)
    pub full_name: String,
}

/// GitHub Pull Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// PR number (GitHub's equivalent to GitLab's IID)
    pub number: u64,

    /// PR ID (unique across GitHub)
    pub id: u64,

    /// PR title
    pub title: String,

    /// PR body/description
    pub body: Option<String>,

    /// Head branch information
    pub head: BranchRef,

    /// Base branch information
    pub base: BranchRef,

    /// PR state (open, closed)
    pub state: String,

    /// HTML URL to view the PR
    pub html_url: String,

    /// User who created the PR
    pub user: GitHubUser,

    /// Created at timestamp (ISO 8601)
    pub created_at: String,

    /// Assignees of the PR
    pub assignees: Vec<GitHubUser>,

    /// Requested reviewers (GitHub-specific)
    pub requested_reviewers: Vec<GitHubUser>,

    /// Draft status
    pub draft: bool,

    /// Whether the PR was merged (only present in individual PR fetch, not
    /// list)
    #[serde(default)]
    pub merged: bool,
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
    /// Review ID
    pub id: u64,

    /// User who submitted the review
    pub user: GitHubUser,

    /// Review body/comment
    pub body: Option<String>,

    /// Review state: APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED, PENDING
    pub state: ReviewState,

    /// HTML URL to view the review
    pub html_url: String,

    /// Submitted at timestamp (ISO 8601)
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum BranchRuleType {
    PullRequest,

    #[serde(other)]
    Unknown,
}

/// GitHub Branch Rule
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchRule {
    /// Rule type
    #[serde(rename = "type")]
    pub rule_type: BranchRuleType,

    /// Rule parameters (contains required_approving_review_count for
    /// pull_request rule)
    #[serde(default)]
    pub parameters: Option<BranchRuleParameters>,
}

/// Parameters for branch rules
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchRuleParameters {
    /// Required approving review count (for pull_request rules)
    #[serde(default)]
    pub required_approving_review_count: Option<u32>,
}

/// Response from listing check runs
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckRunsResponse {
    /// Total count of check runs
    pub total_count: u32,

    /// List of check runs
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CheckRunStatus {
    Queued,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CheckRunConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipper,
    TimedOut,
    ActionRequired,
}

/// GitHub Check Run
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckRun {
    /// Check run ID
    pub id: u64,

    /// Check run name
    pub name: String,

    /// Status of the check run
    pub status: CheckRunStatus,

    /// Conclusion (only present when status is completed)
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
    path: Vec<String>,
}

impl GitHubForge {
    /// Create a new GitHub client
    ///
    /// # Arguments
    /// * `base_url` - GitHub API URL (e.g., <https://api.github.com> or <https://github.example.com/api/v3>)
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

        Ok(Self {
            base_url,
            source_project_id: source_project_id.into(),
            target_project_id: target_project_id.into(),
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
        let mut req = self
            .client
            .request(method, format!("{}{}", self.base_url, path.as_ref()))
            .header("Authorization", format!("token {}", &self.token))
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
            return Err(Error::GitHubApi {
                message: format!("Failed to get: {} - {}", status, text),
            });
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| Error::GitHubApi {
            message: format!(
                "Failed to parse GET response to {}: {}, response: {}",
                path.as_ref(),
                e,
                body
            ),
        })?;
        Ok(data)
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T> {
        // TODO use a real graphql client
        let graphql_url = if self.base_url.starts_with("https://api.github.com") {
            "https://api.github.com/graphql".to_string()
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
            .header("Authorization", format!("Bearer {}", &self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "jj-vine")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitHubApi {
                message: format!("GraphQL request failed: {} - {}", status, text),
            });
        }

        let body = response.text().await?;
        let data: GraphQLResponse<T> =
            serde_json::from_str(&body).map_err(|e| Error::GitHubApi {
                message: format!(
                    "Failed to parse GraphQL response: {}, response: {}",
                    e, body
                ),
            })?;

        if let Some(errors) = data.errors {
            return Err(Error::GitHubApi {
                message: format!(
                    "GraphQL request failed: {}",
                    errors
                        .iter()
                        .map(|error| error.message.clone())
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
            });
        }

        Ok(data.data)
    }
}

impl Forge for GitHubForge {
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
        let base_url = if self.base_url.starts_with("https://api.github.com") {
            "https://github.com"
        } else if self.base_url.contains("/api/v3") {
            self.base_url.trim_end_matches("/api/v3")
        } else {
            &self.base_url
        };
        format!("{}/{}", base_url, self.target_project_id)
    }

    async fn current_user(&self) -> Result<ForgeUser> {
        let user: GitHubUser = self.request(Method::GET, "/user", None::<()>).await?;
        Ok(user.into())
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>> {
        match self
            .request::<GitHubUser>(Method::GET, format!("/users/{}", username), None::<()>)
            .await
        {
            Ok(user) => Ok(Some(user.into())),
            Err(Error::GitHubApi { message }) if message.contains("404") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>> {
        let source_owner = self.source_project_id.split('/').next().unwrap();

        let prs: Vec<PullRequest> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/pulls?head={}:{}&state=open",
                    self.target_project_id,
                    source_owner,
                    urlencoding::encode(branch)
                ),
                None::<()>,
            )
            .await?;
        Ok(prs.into_iter().next().map(ForgeMergeRequest::GitHub))
    }

    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest> {
        let url = format!("/repos/{}/pulls", self.target_project_id);

        // For fork workflows, head needs to be "owner:branch"
        let head = if self.source_project_id != self.target_project_id {
            let source_owner = self.source_project_id.split('/').next().unwrap();
            format!("{}:{}", source_owner, options.source_branch)
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

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn update_merge_request_base(
        &self,
        pr_number: &str,
        new_base: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!("/repos/{}/pulls/{}", self.target_project_id, pr_number),
                Some(serde_json::json!({
                    "base": new_base,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn update_merge_request_description(
        &self,
        pr_number: &str,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!("/repos/{}/pulls/{}", self.target_project_id, pr_number),
                Some(serde_json::json!({
                    "body": new_description,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn get_merge_request(&self, pr_number: &str) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::GET,
                format!("/repos/{}/pulls/{}", self.target_project_id, pr_number),
                None::<()>,
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn get_approval_status(&self, pr_number: &str) -> Result<ApprovalStatus> {
        let pr = self.get_merge_request(pr_number).await?;
        let base_branch = pr.target_branch();

        let (reviews, required_count): (Result<Vec<Review>, _>, _) = join!(
            self.request(
                Method::GET,
                format!(
                    "/repos/{}/pulls/{}/reviews",
                    self.target_project_id, pr_number
                ),
                None::<()>,
            ),
            self.get_required_approvals(base_branch),
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
                .count() as u32
        });

        let blocking_count = user_reviews.as_ref().map(|reviews| {
            reviews
                .values()
                .filter(|state| matches!(state, ReviewState::ChangesRequested))
                .count() as u32
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

    async fn get_check_status(&self, pr_number: &str) -> Result<CheckStatus> {
        let pr = self.get_merge_request(pr_number).await?;

        let head_sha = match pr {
            ForgeMergeRequest::GitHub(pr) => pr.head.sha,
            _ => unreachable!(),
        };

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
                (CheckRunStatus::Queued, _) | (CheckRunStatus::InProgress, _) => {
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

    async fn num_open_discussions(&self, pr_number: &str) -> Result<DiscussionCount> {
        let discussions = self.get_discussions(pr_number).await?;
        Ok(discussions.iter().fold(
            DiscussionCount {
                all: 0,
                unresolved: 0,
                resolved: 0,
            },
            |mut acc, comment| {
                acc.all += 1;

                if comment.is_minimized {
                    acc.resolved += 1;
                    // Close enough?
                } else if comment.viewer_can_minimize {
                    acc.unresolved += 1;
                }

                acc
            },
        ))
    }
}

impl FormatMergeRequest for GitHubForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("#{}", mr_iid)
    }

    fn mr_name(&self) -> &'static str {
        "PR"
    }
}

impl GitHubForge {
    async fn add_assignees(&self, pr_number: u64, assignee_usernames: Vec<String>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!(
                "/repos/{}/issues/{}/assignees",
                self.target_project_id, pr_number
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
                "/repos/{}/pulls/{}/requested_reviewers",
                self.target_project_id, pr_number
            ),
            Some(serde_json::json!({
                "reviewers": reviewer_usernames,
            })),
        )
        .await?;
        Ok(())
    }

    /// Get required approval count from branch protection rules
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
        pr_number: &str,
    ) -> Result<Vec<graphql::GetDiscussionsQueryComment>> {
        let (owner, name) = self.target_project_id.split("/").collect_tuple().unwrap();

        // TODO pagination, real gql client
        let response: graphql::GetDiscussionsQueryResponse = self
            .graphql(
                r#"
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
        "#,
                serde_json::json!({
                    "owner": owner,
                    "name": name,
                    "pr_number": pr_number,
                }),
            )
            .await?;

        let pull_request = response
            .repository
            .ok_or(Error::GitHubApi {
                message: format!("Repository {} not found", self.target_project_id),
            })?
            .pull_request
            .ok_or(Error::GitHubApi {
                message: format!("Pull request {} not found", pr_number),
            })?;

        let root_comments = &pull_request.comments.nodes;

        let reviews = &pull_request.reviews.unwrap_or_default().nodes;
        let review_comments = reviews
            .iter()
            .flat_map(|review| review.comments.nodes.iter())
            .collect::<Vec<_>>();

        Ok(root_comments
            .iter()
            .chain(review_comments.into_iter())
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_client_new() {
        let client = GitHubForge::new(
            "https://api.github.com".to_string(),
            "owner/repo".to_string(),
            "owner/repo".to_string(),
            "ghp_token123".to_string(),
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
    fn test_project_url() {
        let client = GitHubForge::new(
            "https://api.github.com".to_string(),
            "owner/repo".to_string(),
            "owner/repo".to_string(),
            "token".to_string(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(client.project_url(), "https://github.com/owner/repo");
    }

    #[test]
    fn test_github_enterprise_url() {
        let client = GitHubForge::new(
            "https://github.example.com/api/v3".to_string(),
            "owner/repo".to_string(),
            "owner/repo".to_string(),
            "token".to_string(),
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
