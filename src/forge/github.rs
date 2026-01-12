use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeMergeRequest, ForgeUser},
};

/// GitHub REST API client
pub struct GitHubForge {
    base_url: String,
    project_id: String,
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
            id: user.id.to_string(),
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
        base_url: String,
        project_id: String,
        token: String,
        ca_bundle: Option<String>,
        accept_non_compliant_certs: bool,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder();

        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        if let Some(ca_path) = ca_bundle {
            let ca_cert = std::fs::read(&ca_path).map_err(|e| Error::Config {
                message: format!("Failed to read CA bundle at {}: {}", ca_path, e),
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
        let base_url = base_url.trim_end_matches('/').to_string();

        Ok(Self {
            base_url,
            project_id,
            token,
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
}

#[async_trait]
impl Forge for GitHubForge {
    fn project_id(&self) -> &str {
        &self.project_id
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
        format!("{}/{}", base_url, self.project_id)
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
        let owner = self.project_id.split('/').next().unwrap();

        let prs: Vec<PullRequest> = self
            .request(
                Method::GET,
                format!(
                    "/repos/{}/pulls?head={}:{}&state=open",
                    self.project_id,
                    owner,
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
        let url = format!("/repos/{}/pulls", self.project_id);

        let mut payload = serde_json::json!({
            "title": options.title,
            "head": options.source_branch,
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
        pr_number: u64,
        new_base: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!("/repos/{}/pulls/{}", self.project_id, pr_number),
                Some(serde_json::json!({
                    "base": new_base,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn update_merge_request_description(
        &self,
        pr_number: u64,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::PATCH,
                format!("/repos/{}/pulls/{}", self.project_id, pr_number),
                Some(serde_json::json!({
                    "body": new_description,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }

    async fn get_merge_request(&self, pr_number: u64) -> Result<ForgeMergeRequest> {
        let pr: PullRequest = self
            .request(
                Method::GET,
                format!("/repos/{}/pulls/{}", self.project_id, pr_number),
                None::<()>,
            )
            .await?;

        Ok(ForgeMergeRequest::GitHub(pr))
    }
}

impl FormatMergeRequest for GitHubForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("#{}", mr_iid)
    }
}

impl GitHubForge {
    async fn add_assignees(&self, pr_number: u64, assignee_usernames: Vec<String>) -> Result<()> {
        self.request::<serde_json::Value>(
            Method::POST,
            format!("/repos/{}/issues/{}/assignees", self.project_id, pr_number),
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
                self.project_id, pr_number
            ),
            Some(serde_json::json!({
                "reviewers": reviewer_usernames,
            })),
        )
        .await?;
        Ok(())
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
            "ghp_token123".to_string(),
            None,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(client.base_url, "https://api.github.com");
        assert_eq!(client.project_id, "owner/repo");
        assert_eq!(client.token, "ghp_token123");
    }

    #[test]
    fn test_project_url() {
        let client = GitHubForge::new(
            "https://api.github.com".to_string(),
            "owner/repo".to_string(),
            "token".to_string(),
            None,
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
            "token".to_string(),
            None,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(
            client.project_url(),
            "https://github.example.com/owner/repo"
        );
    }
}
