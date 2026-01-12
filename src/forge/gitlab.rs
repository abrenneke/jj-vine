use std::path::Path;

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeMergeRequest, ForgeUser},
};

/// GitLab REST API client
pub struct GitLabForge {
    base_url: String,
    project_id: String,
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabUser {
    pub id: u64,
    pub username: String,
}

impl From<GitLabUser> for ForgeUser {
    fn from(user: GitLabUser) -> Self {
        ForgeUser {
            id: user.id.to_string(),
            username: user.username,
        }
    }
}

impl GitLabForge {
    /// Create a new GitLab client
    ///
    /// # Arguments
    /// * `base_url` - GitLab instance URL (e.g., <https://gitlab.example.com>)
    /// * `project_id` - Project ID (e.g., "group/project" or "12345")
    /// * `token` - Personal Access Token
    /// * `ca_bundle` - Optional path to CA bundle for TLS verification
    /// * `accept_non_compliant_certs` - Accept non-compliant TLS certificates
    pub fn new(
        base_url: impl Into<String>,
        project_id: impl Into<String>,
        token: impl Into<String>,
        ca_bundle: Option<impl AsRef<Path>>,
        accept_non_compliant_certs: bool,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder();

        // Accept non-compliant certificates if configured
        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        // Add custom CA bundle if provided
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

        Ok(Self {
            base_url: base_url.into(),
            project_id: project_id.into(),
            token: token.into(),
            client,
        })
    }

    fn encoded_project_id(&self) -> String {
        urlencoding::encode(&self.project_id).to_string()
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
            .header("Authorization", format!("Bearer {}", &self.token));

        if let Some(payload) = payload.as_ref() {
            req = req.json(payload);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(Error::GitLabApi {
                message: format!("Failed to get: {} - {}", status, text),
            });
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| Error::GitLabApi {
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
impl Forge for GitLabForge {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the current authenticated user
    async fn current_user(&self) -> Result<ForgeUser> {
        let user: GitLabUser = self
            .request(Method::GET, "/api/v4/user", None::<()>)
            .await?;
        Ok(user.into())
    }

    /// Get user by username
    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>> {
        let users: Vec<GitLabUser> = self
            .request(
                Method::GET,
                format!("/api/v4/users?username={}", urlencoding::encode(username)),
                None::<()>,
            )
            .await?;
        Ok(users.into_iter().next().map(ForgeUser::from))
    }

    /// Find merge request by source branch name. Returns the first MR found
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>> {
        let mrs: Vec<MergeRequest> = self
            .request(
                Method::GET,
                format!(
                    "/api/v4/projects/{}/merge_requests?source_branch={}&state=opened",
                    self.encoded_project_id(),
                    urlencoding::encode(branch)
                ),
                None::<()>,
            )
            .await?;
        Ok(mrs.into_iter().next().map(ForgeMergeRequest::GitLab))
    }

    /// Create a new merge request
    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest> {
        let url = format!(
            "/api/v4/projects/{}/merge_requests",
            self.encoded_project_id()
        );

        let mut payload = serde_json::json!({
            "source_branch": options.source_branch,
            "target_branch": options.target_branch,
            "title": options.title,
            "remove_source_branch": options.remove_source_branch.unwrap_or(true),
            "squash": options.squash.unwrap_or(false),
        });

        if let Some(desc) = options.description {
            payload["description"] = serde_json::json!(desc);
        }

        if let Some(assignees) = options.assignee_ids
            && !assignees.is_empty()
        {
            payload["assignee_ids"] = serde_json::json!(assignees);
        }

        if let Some(reviewers) = options.reviewer_ids
            && !reviewers.is_empty()
        {
            payload["reviewer_ids"] = serde_json::json!(reviewers);
        }

        let mr: MergeRequest = self.request(Method::POST, url, Some(payload)).await?;
        Ok(ForgeMergeRequest::GitLab(mr))
    }

    /// Update the target branch (base) of an existing merge request
    async fn update_merge_request_base(
        &self,
        mr_iid: u64,
        new_target_branch: &str,
    ) -> Result<ForgeMergeRequest> {
        let mr: MergeRequest = self
            .request(
                Method::PUT,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_project_id(),
                    mr_iid
                ),
                Some(serde_json::json!({
                    "target_branch": new_target_branch,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitLab(mr))
    }

    /// Update the description of an existing merge request
    async fn update_merge_request_description(
        &self,
        mr_iid: u64,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let mr: MergeRequest = self
            .request(
                Method::PUT,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_project_id(),
                    mr_iid,
                ),
                Some(serde_json::json!({
                    "description": new_description,
                })),
            )
            .await?;

        Ok(ForgeMergeRequest::GitLab(mr))
    }

    /// Get a specific merge request by IID
    async fn get_merge_request(&self, merge_request_iid: u64) -> Result<ForgeMergeRequest> {
        let mr: MergeRequest = self
            .request(
                Method::GET,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_project_id(),
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(ForgeMergeRequest::GitLab(mr))
    }
}

impl FormatMergeRequest for GitLabForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("!{}", mr_iid)
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

    /// User of the author of the MR
    pub author: GitLabUser,

    /// Created at timestamp of the MR (ISO 8601)
    pub created_at: String,

    /// Assignees of the MR
    pub assignees: Vec<GitLabUser>,

    /// Reviewers of the MR
    pub reviewers: Vec<GitLabUser>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitlab_client_new() {
        let client = GitLabForge::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        assert_eq!(client.base_url, "https://gitlab.example.com");
        assert_eq!(client.project_id, "group/project");
        assert_eq!(client.token, "token123");
    }

    #[test]
    fn test_encode_project_id() {
        let client = GitLabForge::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            None::<&str>,
            false,
        )
        .expect("Failed to create client");

        let encoded = client.encoded_project_id();
        assert_eq!(encoded, "group%2Fproject");
    }

    #[test]
    fn test_ca_bundle_with_multiple_certificates() {
        use std::io::Write;

        use tempfile::NamedTempFile;

        // Create a temporary file with multiple certificates
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

        // Write two valid PEM certificates (generated with openssl x509 v3)
        let cert_bundle = "-----BEGIN CERTIFICATE-----
MIIDeTCCAmGgAwIBAgIUfO3nrSE5qNWMV+TDTa+tkCwUd04wDQYJKoZIhvcNAQEL
BQAwWTELMAkGA1UEBhMCVVMxDTALBgNVBAgMBFRlc3QxDTALBgNVBAcMBFRlc3Qx
DTALBgNVBAoMBFRlc3QxDTALBgNVBAsMBFRlc3QxDjAMBgNVBAMMBVRlc3QxMB4X
DTI2MDEwODAxMDQxNFoXDTI3MDEwODAxMDQxNFowWTELMAkGA1UEBhMCVVMxDTAL
BgNVBAgMBFRlc3QxDTALBgNVBAcMBFRlc3QxDTALBgNVBAoMBFRlc3QxDTALBgNV
BAsMBFRlc3QxDjAMBgNVBAMMBVRlc3QxMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A
MIIBCgKCAQEAo53dK+I1wLb2ck2zOGRDTAQXrXUazxJVPfCVdedJ+pOx4eIR1V8u
iffOsxjWG/hxoIlZpj0+OGj3GdL3wUi7KUqJUcpzVjqylAfYgBIGruQI9qLtmZSx
ZwKhLDRm++83SCRjkwe7daSAgvSlc/0cAWUQcczRPJG1WnG42+V2Tngy6z+FJck4
F8+3dPVGy0tQs0BA6BhMDYffwkRfcx3qI+9rsHb1MdMZ9GDUpG4PNO023jRsPjk3
4kvizo/XyTc6ip6OGFmu3fnXoaO2YkpvHLR5Fgryo5fGoV1J2Wub+caDSC4oJBsq
rAdf5hGE8NxsuauORkMi5cg9h/7Ojn6RpQIDAQABozkwNzAJBgNVHRMEAjAAMAsG
A1UdDwQEAwIF4DAdBgNVHQ4EFgQUr7AeMhGxmPYhMCnJK1Hm6ehZDbkwDQYJKoZI
hvcNAQELBQADggEBAJzhJqfv9RN1HDDPDl5SpG3yZpJYqARe5iuT5O8voLwiGUI+
MdbTO4u0x9khK9tIduW8/oP6DRVqUkvdRuUET414YWq2odYgD7D/3eo14BVnqazx
0UhziLFpW6SGMuS2VrUJDXGk8RLuP5xXZxl2yc8Mhh9n6XwX1QRhWQ+z0anUUDep
Tfcio5swcUsOQGa+9Q2V7Y0Yx2XIVreFi6MAHq/i8vP4CF+zrC1MS+ZEQO/yB1ZB
eH39/z8yA0qBPucG97NBAfWMdqvKU72jV/7flPl6hRiFnDACovPDqqWRDGofeuvS
nrRPwpkJh9lCnuFSMaCybOMgx1tZ9YP0vpAtdA8=
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIIDeTCCAmGgAwIBAgIUN9oyphH7WiltV+bgl5GVEX05MEQwDQYJKoZIhvcNAQEL
BQAwWTELMAkGA1UEBhMCVVMxDTALBgNVBAgMBFRlc3QxDTALBgNVBAcMBFRlc3Qx
DTALBgNVBAoMBFRlc3QxDTALBgNVBAsMBFRlc3QxDjAMBgNVBAMMBVRlc3QyMB4X
DTI2MDEwODAxMDQzM1oXDTI3MDEwODAxMDQzM1owWTELMAkGA1UEBhMCVVMxDTAL
BgNVBAgMBFRlc3QxDTALBgNVBAcMBFRlc3QxDTALBgNVBAoMBFRlc3QxDTALBgNV
BAsMBFRlc3QxDjAMBgNVBAMMBVRlc3QyMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A
MIIBCgKCAQEAtMP3dttGNAbZkDvWuqVf6JBVzHtGY1Jrq1Yohcbbz2tRH13pmnsZ
ml6rnYw2BzJo8PuuLwVvI8yNOtX95XT1MoidW5Marh3MIr6AJ4zgIfNsoC4v32gZ
wfRTiU0E4Y0l6W/McA8DzN25gBUswkd9iPtosM+H5P/fF2xlXlH9TkMz/JxL9haI
wcJcFaLvmJLuO5j1byLplKefjTCVSCvMK+5Z9iP3FxVFkS/Dmjtw1aJwMBNIRXLL
+KDnRStmqqbMPwgNz28BKaif3QThGfa03lLrINQ2OOL3ZaULj5pllpOgf3SL3h54
zviV5VitLLTXowAJkpjgSjBjGHTS5MmW3wIDAQABozkwNzAJBgNVHRMEAjAAMAsG
A1UdDwQEAwIF4DAdBgNVHQ4EFgQUOoaNpsdD/j+YxvwbUDsGR/IjGfYwDQYJKoZI
hvcNAQELBQADggEBABmCPwOnbaTSbShJqFDscoRQo8nuPuSNP76pu+TB14O+vsJq
a8KIRiCTycs72zxaJbdB+5knZs+p3QnDRH3YXhDq8T6xJzDW+mDwrO/xcpdDfEkO
hkLenuLhRNuhwhqAkcdaBvrnZHI7wuI6FAx5EK6MnFaCVvNrFhF/XZRKWH0D022j
wNLLlmTiHEaSCWW/FNYfkwzF+oamHunxZ0TRfFFnVpE1ADMVt9CGe/K1eLoJ9ZLW
zAAjdQJFYiiLIdUrYat1Jz+NlrTCI5/KEIs3/+aS4HwRnM3h3w6taQKDg2q2Hiez
uYyBeUf6LmQswHqXfxOmAoy1HbXDtNvmClznsb0=
-----END CERTIFICATE-----";

        temp_file
            .write_all(cert_bundle.as_bytes())
            .expect("Failed to write to temp file");
        let path = temp_file.path().to_str().unwrap().to_string();

        // This should succeed with from_pem_bundle() but would fail with from_pem()
        GitLabForge::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            Some(&path),
            false,
        )
        .expect("Failed to create client with multi-cert bundle");
    }
}
