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

        // Accept non-compliant certificates if configured
        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        // Add custom CA bundle if provided
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

        Ok(Self {
            base_url,
            project_id,
            token,
            client,
        })
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
                message: format!("Failed to query merge requests: {} - {}", status, text),
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
                message: format!("Failed to create merge request: {} - {}", status, text),
            });
        }

        let mr: MergeRequest = response.json().await?;
        Ok(mr)
    }

    /// Update the target branch (base) of an existing merge request
    pub async fn update_mr_base(
        &self,
        mr_iid: u64,
        new_target_branch: &str,
    ) -> Result<MergeRequest> {
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
                message: format!("Failed to update merge request: {} - {}", status, text),
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
                message: format!("Failed to get merge request: {} - {}", status, text),
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
            None,
            false,
        )
        .expect("Failed to create client");

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
            None,
            false,
        )
        .expect("Failed to create client");

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
        let result = GitLabClient::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            Some(path),
            false,
        );

        match result {
            Ok(_) => {}
            Err(e) => panic!("Failed to create client with multi-cert bundle: {:?}", e),
        }
    }

    // Note: Integration tests with real GitLab API would require a test instance
    // For now, we test the structure and basic functionality
}
