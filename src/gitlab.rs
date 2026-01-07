use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// GitLab REST API client
pub struct GitLabClient {
    base_url: String,
    project_id: String,
    token: String,
    client: reqwest::Client,
}

impl GitLabClient {
    /// Create a new GitLab client
    ///
    /// # Arguments
    /// * `base_url` - GitLab instance URL (e.g., <https://gitlab.example.com>)
    /// * `project_id` - Project ID (e.g., "group/project" or "12345")
    /// * `token` - Personal Access Token
    pub fn new(base_url: String, project_id: String, token: String) -> Self {
        Self {
            base_url,
            project_id,
            token,
            client: reqwest::Client::new(),
        }
    }

    /// URL-encode the project ID for use in API paths
    fn encode_project_id(&self) -> String {
        urlencoding::encode(&self.project_id).to_string()
    }

    /// Find merge request by source branch name
    ///
    /// Returns the first MR found with the given source branch, or None if not found
    pub async fn find_mr_by_source_branch(&self, branch: &str) -> Result<Option<MergeRequest>> {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests?source_branch={}&state=opened",
            self.base_url,
            self.encode_project_id(),
            urlencoding::encode(branch)
        );

        let response = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitLabApi {
                message: format!(
                    "Failed to query merge requests: {} - {}",
                    status,
                    text
                ),
            });
        }

        let mrs: Vec<MergeRequest> = response.json().await?;
        Ok(mrs.into_iter().next())
    }

    /// Create a new merge request
    pub async fn create_merge_request(
        &self,
        source_branch: &str,
        target_branch: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<MergeRequest> {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests",
            self.base_url,
            self.encode_project_id()
        );

        let mut payload = serde_json::json!({
            "source_branch": source_branch,
            "target_branch": target_branch,
            "title": title,
        });

        if let Some(desc) = description {
            payload["description"] = serde_json::json!(desc);
        }

        let response = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitLabApi {
                message: format!(
                    "Failed to create merge request: {} - {}",
                    status,
                    text
                ),
            });
        }

        let mr: MergeRequest = response.json().await?;
        Ok(mr)
    }

    /// Update the target branch (base) of an existing merge request
    pub async fn update_mr_base(&self, mr_iid: u64, new_target_branch: &str) -> Result<MergeRequest> {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests/{}",
            self.base_url,
            self.encode_project_id(),
            mr_iid
        );

        let payload = serde_json::json!({
            "target_branch": new_target_branch,
        });

        let response = self
            .client
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitLabApi {
                message: format!(
                    "Failed to update merge request: {} - {}",
                    status,
                    text
                ),
            });
        }

        let mr: MergeRequest = response.json().await?;
        Ok(mr)
    }

    /// Get a specific merge request by IID
    pub async fn get_merge_request(&self, mr_iid: u64) -> Result<MergeRequest> {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests/{}",
            self.base_url,
            self.encode_project_id(),
            mr_iid
        );

        let response = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitLabApi {
                message: format!(
                    "Failed to get merge request: {} - {}",
                    status,
                    text
                ),
            });
        }

        let mr: MergeRequest = response.json().await?;
        Ok(mr)
    }
}

/// GitLab Merge Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    /// MR internal ID (unique within project)
    pub iid: u64,

    /// MR global ID
    pub id: u64,

    /// MR title
    pub title: String,

    /// MR description
    pub description: Option<String>,

    /// Source branch name
    pub source_branch: String,

    /// Target branch name
    pub target_branch: String,

    /// MR state (opened, closed, merged, etc.)
    pub state: String,

    /// Web URL to view the MR
    pub web_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitlab_client_new() {
        let client = GitLabClient::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
        );

        assert_eq!(client.base_url, "https://gitlab.example.com");
        assert_eq!(client.project_id, "group/project");
        assert_eq!(client.token, "token123");
    }

    #[test]
    fn test_encode_project_id() {
        let client = GitLabClient::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
        );

        let encoded = client.encode_project_id();
        assert_eq!(encoded, "group%2Fproject");
    }

    #[test]
    fn test_merge_request_struct() {
        let mr = MergeRequest {
            iid: 123,
            id: 456,
            title: "Test MR".to_string(),
            description: Some("Test description".to_string()),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            state: "opened".to_string(),
            web_url: "https://gitlab.example.com/group/project/-/merge_requests/123".to_string(),
        };

        assert_eq!(mr.iid, 123);
        assert_eq!(mr.title, "Test MR");
        assert_eq!(mr.source_branch, "feature");
        assert_eq!(mr.target_branch, "main");
    }

    // Note: Integration tests with real GitLab API would require a test instance
    // For now, we test the structure and basic functionality
}
