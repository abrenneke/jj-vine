use std::{borrow::Cow, collections::HashSet, path::Path};

use futures::{StreamExt, stream::FuturesUnordered, try_join};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    description::FormatMergeRequest,
    error::{ConfigSnafu, Error, GitLabApiSnafu, Result},
    forge::{
        AnyForgeUser,
        ApprovalSatisfaction,
        ApprovalStatus,
        CheckStatus,
        DiscussionCount,
        Forge,
        ForgeCreateMergeRequestOptions,
        ForgeMergeRequest,
        ForgeMergeRequestState,
        ForgeUpdateMergeRequestInfoOptions,
        ForgeUser,
        MergeRequestStatus,
        UserId,
    },
};

/// GitLab REST API client
pub struct GitLabForge {
    base_url: String,
    source_project_id: String,
    target_project_id: String,
    token: String,
    client: reqwest::Client,

    /// Whether to create & sync merge request dependencies.
    create_merge_request_dependencies: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabUser {
    pub id: u64,
    pub username: String,
}

impl ForgeUser for GitLabUser {
    fn id(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(self.id.to_string()))
    }

    fn username(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.username))
    }
}

impl TryFrom<AnyForgeUser> for GitLabUser {
    type Error = Error;

    fn try_from(value: AnyForgeUser) -> Result<Self> {
        Ok(Self {
            id: value
                .id()
                .ok_or_else(|| Error::new("User ID is required"))?
                .parse()?,
            username: value
                .username()
                .ok_or_else(|| Error::new("Username is required"))?
                .to_string(),
        })
    }
}

impl GitLabForge {
    /// Create a new GitLab client
    pub fn new(
        base_url: impl Into<String>,
        source_project_id: impl Into<String>,
        target_project_id: impl Into<String>,
        token: impl Into<String>,
        ca_bundle: Option<impl AsRef<Path>>,
        accept_non_compliant_certs: bool,
        create_merge_request_dependencies: bool,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder();

        // Accept non-compliant certificates if configured
        if accept_non_compliant_certs {
            client_builder = client_builder.tls_danger_accept_invalid_certs(true);
        }

        // Add custom CA bundle if provided
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
                message: format!("Failed to build HTTP client: {:?}", e),
            }
            .build()
        })?;

        Ok(Self {
            base_url: base_url.into(),
            source_project_id: source_project_id.into(),
            target_project_id: target_project_id.into(),
            token: token.into(),
            client,
            create_merge_request_dependencies,
        })
    }

    fn encoded_target_project_id(&self) -> String {
        urlencoding::encode(&self.target_project_id).to_string()
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: impl AsRef<str>,
        payload: Option<impl Serialize>,
    ) -> Result<T> {
        let mut req = self
            .client
            .request(
                method.clone(),
                format!("{}{}", self.base_url, path.as_ref()),
            )
            .header("Authorization", format!("Bearer {}", &self.token));

        if let Some(payload) = payload.as_ref() {
            req = req.json(payload);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return GitLabApiSnafu {
                message: format!("Failed to {}: {} - {}", method, status, text),
            }
            .fail();
        }

        let body = response.text().await?;
        let data: T = serde_json::from_str(&body).map_err(|e| {
            GitLabApiSnafu {
                message: format!(
                    "Failed to parse {} response to {}: {}, response: {}",
                    method,
                    path.as_ref(),
                    e,
                    body
                ),
            }
            .build()
        })?;
        Ok(data)
    }

    fn wrap_draft(&self, title: &str, draft: bool) -> String {
        let title = title.trim_start_matches("Draft: ");
        if draft {
            format!("Draft: {}", title)
        } else {
            title.to_string()
        }
    }
}

struct NoContent;

impl<'de> Deserialize<'de> for NoContent {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(NoContent)
    }
}

impl Forge for GitLabForge {
    type User = GitLabUser;

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

    fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the current authenticated user
    async fn current_user(&self) -> Result<Self::User> {
        let user: GitLabUser = self
            .request(Method::GET, "/api/v4/user", None::<()>)
            .await?;
        Ok(user)
    }

    /// Get user by username
    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>> {
        let users: Vec<GitLabUser> = self
            .request(
                Method::GET,
                format!("/api/v4/users?username={}", urlencoding::encode(username)),
                None::<()>,
            )
            .await?;
        Ok(users.into_iter().next())
    }

    /// Find merge request by source branch name. Returns the first MR found
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        let mrs: Vec<MergeRequest> = self
            .request(
                Method::GET,
                format!(
                    "/api/v4/projects/{}/merge_requests?source_branch={}&state=opened",
                    self.encoded_target_project_id(),
                    urlencoding::encode(branch)
                ),
                None::<()>,
            )
            .await?;
        Ok(mrs.into_iter().next())
    }

    /// Create a new merge request
    async fn create_merge_request(
        &self,
        ForgeCreateMergeRequestOptions {
            assignees,
            description,
            open_as_draft,
            remove_source_branch,
            reviewers,
            source_branch,
            squash,
            target_branch,
            title,
        }: ForgeCreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        #[derive(Serialize)]
        struct Body {
            source_branch: String,
            target_branch: String,
            title: String,
            remove_source_branch: bool,
            squash: bool,

            #[serde(skip_serializing_if = "Option::is_none")]
            source_project_id: Option<String>,

            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<String>,

            #[serde(skip_serializing_if = "Option::is_none")]
            assignee_ids: Option<Vec<u64>>,

            #[serde(skip_serializing_if = "Option::is_none")]
            reviewer_ids: Option<Vec<u64>>,
        }

        let payload = Body {
            source_branch,
            target_branch,
            // I think? Gitlab be weird
            title: self.wrap_draft(&title, open_as_draft),
            remove_source_branch,
            squash,
            source_project_id: if self.source_project_id != self.target_project_id {
                Some(self.source_project_id.clone())
            } else {
                None
            },
            description,
            assignee_ids: if !assignees.is_empty() {
                Some(assignees.into_iter().map(|user| user.0).collect())
            } else {
                None
            },
            reviewer_ids: if !reviewers.is_empty() {
                Some(reviewers.into_iter().map(|user| user.0).collect())
            } else {
                None
            },
        };

        let mr: MergeRequest = self
            .request(
                Method::POST,
                format!(
                    "/api/v4/projects/{}/merge_requests",
                    self.encoded_target_project_id()
                ),
                Some(payload),
            )
            .await?;

        Ok(mr)
    }

    /// Update the target branch (base) of an existing merge request
    async fn update_merge_request_base(
        &self,
        merge_request_iid: Self::Id,
        new_target_branch: &str,
    ) -> Result<Self::MergeRequest> {
        let mr: MergeRequest = self
            .request(
                Method::PUT,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_target_project_id(),
                    merge_request_iid
                ),
                Some(serde_json::json!({
                    "target_branch": new_target_branch,
                })),
            )
            .await?;

        Ok(mr)
    }

    /// Update the description of an existing merge request
    async fn update_merge_request_info(
        &self,
        merge_request_iid: Self::Id,
        ForgeUpdateMergeRequestInfoOptions {
            title,
            description,
            draft,
            current_title,
            current_is_draft,
        }: ForgeUpdateMergeRequestInfoOptions,
    ) -> Result<Self::MergeRequest> {
        #[derive(Serialize)]
        struct Body {
            #[serde(skip_serializing_if = "Option::is_none")]
            title: Option<String>,

            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<String>,
        }
        let body = Body {
            title: match (draft, title) {
                (Some(draft), Some(title)) => Some(self.wrap_draft(&title, draft)),
                (Some(draft), None) => Some(self.wrap_draft(&current_title, draft)),
                (None, Some(title)) => Some(self.wrap_draft(&title, current_is_draft)),
                (None, None) => None,
            },
            description: description.map(|description| description.to_string()),
        };

        let mr: MergeRequest = self
            .request(
                Method::PUT,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_target_project_id(),
                    merge_request_iid,
                ),
                Some(body),
            )
            .await?;

        Ok(mr)
    }

    /// Get a specific merge request by IID
    async fn get_merge_request(&self, merge_request_iid: Self::Id) -> Result<Self::MergeRequest> {
        let mr: MergeRequest = self
            .request(
                Method::GET,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}",
                    self.encoded_target_project_id(),
                    merge_request_iid
                ),
                None::<()>,
            )
            .await?;

        Ok(mr)
    }

    /// Get approval status for a merge request
    async fn get_approval_status(&self, merge_request_iid: Self::Id) -> Result<ApprovalStatus> {
        let approvals: Result<MergeRequestApprovals, _> = self
            .request(
                Method::GET,
                format!(
                    "/api/v4/projects/{}/merge_requests/{}/approvals",
                    self.encoded_target_project_id(),
                    merge_request_iid
                ),
                None::<()>,
            )
            .await;

        // Can't figure out how to get the blocking count
        // https://stackoverflow.com/questions/78573772/how-to-get-changes-requested-info-on-gitlab-mr

        let approved_count = approvals
            .as_ref()
            .map(|approvals| approvals.approved_by.len() as u32)
            .unwrap_or(0);
        let required_count = approvals
            .as_ref()
            .map(|approvals| approvals.approvals_required)
            .unwrap_or(0);

        Ok(ApprovalStatus {
            blocking_count: 0,
            approved_count,
            required_count,
            satisfaction: match approvals {
                Ok(approvals) if approvals.approvals_left == 0 => ApprovalSatisfaction::Satisfied,
                Ok(_) => ApprovalSatisfaction::Unsatisfied,
                Err(_) => ApprovalSatisfaction::Unknown,
            },
        })
    }

    /// Get CI/pipeline check status for a merge request
    async fn get_check_status(&self, merge_request_iid: Self::Id) -> Result<CheckStatus> {
        let source_branch = self
            .get_merge_request(merge_request_iid)
            .await?
            .source_branch;

        // So for whatever reason for us, `/pipelines/latest` just... doesn't work?
        // `/pipelines` will return something in the array, but not `/latest` (403) 🤷
        let response = self
            .client
            .request(
                Method::GET,
                format!(
                    "{}/api/v4/projects/{}/pipelines?ref={}",
                    self.base_url,
                    self.encoded_target_project_id(),
                    urlencoding::encode(&source_branch)
                ),
            )
            .header("Authorization", format!("Bearer {}", &self.token))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {
                let pipelines: Vec<Pipeline> = response.json().await?;
                match pipelines.first() {
                    Some(Pipeline {
                        status: PipelineStatus::Success,
                        ..
                    }) => Ok(CheckStatus::Success),
                    Some(Pipeline {
                        status: PipelineStatus::Failed | PipelineStatus::Canceled,
                        ..
                    }) => Ok(CheckStatus::Failed),
                    Some(_) => Ok(CheckStatus::Pending),
                    _ => Ok(CheckStatus::None),
                }
            }
            StatusCode::NOT_FOUND => Ok(CheckStatus::None),
            // Shrug, guess this means no pipeline is configured
            StatusCode::FORBIDDEN => Ok(CheckStatus::None),
            _ => Err(GitLabApiSnafu {
                message: format!("Failed to get pipeline status: {}", response.status()),
            }
            .build()),
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
        Ok(discussions
            .iter()
            .filter_map(|discussion| match &discussion.notes[..] {
                // Notes with no type are like "added commits" and "changed description", not a
                // discussion
                []
                | [
                    DiscussionNote {
                        note_type: None, ..
                    },
                ] => None,
                // We only need the root note
                [note, ..] => Some(note),
            })
            .fold(Default::default(), |mut acc, first_note| {
                acc.all += 1;

                if first_note.resolved {
                    acc.resolved += 1;
                } else if first_note.resolvable {
                    acc.unresolved += 1;
                }
                acc
            }))
    }

    fn supports_dependent_merge_requests(&self) -> bool {
        true
    }

    async fn sync_dependent_merge_requests(
        &self,
        merge_request_iid: Self::Id,
        dependent_merge_request_iids: &[Self::Id],
    ) -> Result<bool> {
        if !self.create_merge_request_dependencies {
            return Ok(false);
        }

        let needed_deps: HashSet<u64> = dependent_merge_request_iids.iter().copied().collect();

        // Now we need to get the ID for all needed deps... not the IID :/
        let needed_deps: HashSet<u64> = needed_deps
            .into_iter()
            .map(|dep| async move { Ok(self.get_merge_request(dep).await?.id) })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<Result<_>>>()
            .await
            .into_iter()
            .collect::<Result<_>>()?;

        let current_deps = self
            .get_merge_request_dependencies(merge_request_iid)
            .await?;
        let current_deps_set: HashSet<_> = current_deps
            .iter()
            .map(|dep| dep.blocking_merge_request.id)
            .collect();

        let unneeded_deps: HashSet<_> = current_deps_set.difference(&needed_deps).collect();
        current_deps
            .iter()
            .filter(|dep| unneeded_deps.contains(&dep.blocking_merge_request.id))
            .map(|dep| self.delete_merge_request_dependency(merge_request_iid, dep.id))
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<Result<_>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        let new_deps: HashSet<_> = needed_deps.difference(&current_deps_set).collect();
        new_deps
            .iter()
            .map(|id| self.create_merge_request_dependency(merge_request_iid, **id))
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<Result<_>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        Ok(!(unneeded_deps.is_empty() && new_deps.is_empty()))
    }
}

impl GitLabForge {
    async fn get_discussions(&self, merge_request_iid: u64) -> Result<Vec<Discussion>> {
        self.request(
            Method::GET,
            format!(
                "/api/v4/projects/{}/merge_requests/{}/discussions",
                self.encoded_target_project_id(),
                merge_request_iid
            ),
            None::<()>,
        )
        .await
    }

    pub async fn get_merge_request_dependencies(
        &self,
        merge_request_iid: u64,
    ) -> Result<Vec<MergeRequestDependency>> {
        self.request(
            Method::GET,
            format!(
                "/api/v4/projects/{}/merge_requests/{}/blocks",
                self.encoded_target_project_id(),
                merge_request_iid
            ),
            None::<()>,
        )
        .await
    }

    async fn delete_merge_request_dependency(
        &self,
        merge_request_iid: u64,
        dependency_id: u64,
    ) -> Result<NoContent> {
        self.request(
            Method::DELETE,
            format!(
                "/api/v4/projects/{}/merge_requests/{}/blocks/{}",
                self.encoded_target_project_id(),
                merge_request_iid,
                dependency_id
            ),
            None::<()>,
        )
        .await
    }

    async fn create_merge_request_dependency(
        &self,
        merge_request_iid: u64,
        blocking_merge_request_global_id: u64,
    ) -> Result<MergeRequestDependency> {
        self.request(
            Method::POST,
            format!(
                "/api/v4/projects/{}/merge_requests/{}/blocks",
                self.encoded_target_project_id(),
                merge_request_iid
            ),
            Some(serde_json::json!({
                "blocking_merge_request_id": blocking_merge_request_global_id
            })),
        )
        .await
    }
}

impl FormatMergeRequest for GitLabForge {
    type Id = u64;

    fn format_merge_request_id(&self, mr_iid: Self::Id) -> String {
        format!("!{}", mr_iid)
    }

    fn mr_name(&self) -> &'static str {
        "MR"
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

impl ForgeMergeRequest for MergeRequest {
    type User = GitLabUser;

    type Id = u64;

    fn iid(&self) -> Self::Id {
        self.iid
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        self.description.as_deref().unwrap_or_default()
    }

    fn source_branch(&self) -> &str {
        &self.source_branch
    }

    fn target_branch(&self) -> &str {
        &self.target_branch
    }

    fn state(&self) -> ForgeMergeRequestState {
        if self.state == "opened" {
            ForgeMergeRequestState::Open
        } else if self.state == "closed" {
            ForgeMergeRequestState::Closed
        } else if self.state == "merged" {
            ForgeMergeRequestState::Merged
        } else {
            ForgeMergeRequestState::Open
        }
    }

    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.web_url)
    }

    fn edit_url(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}/edit", self.web_url))
    }

    fn author_username(&self) -> &str {
        &self.author.username
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
        self.reviewers.clone()
    }

    fn is_draft(&self) -> bool {
        self.title.starts_with("Draft:")
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

/// GitLab MR approval information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MergeRequestApprovals {
    /// Number of approvals required
    pub approvals_required: u32,

    /// Number of approvals still needed
    pub approvals_left: u32,

    /// Whether the MR is approved (approvals_left == 0)
    pub approved: bool,

    /// List of users who approved
    pub approved_by: Vec<ApprovedBy>,
}

/// Represents an approval on a merge request
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovedBy {
    /// User who approved
    pub user: GitLabUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PipelineStatus {
    Created,
    WaitingForResource,
    Preparing,
    Pending,
    Running,
    Success,
    Failed,
    Canceled,
    Skipped,
    Manual,
    Scheduled,
}

/// GitLab pipeline information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pipeline {
    /// Pipeline ID
    pub id: u64,

    /// Pipeline status
    pub status: PipelineStatus,

    /// Reference (branch/tag) the pipeline ran on
    #[serde(rename = "ref")]
    pub ref_name: String,

    /// Commit SHA
    pub sha: String,

    /// Web URL to view the pipeline
    pub web_url: String,
}

/// A collection, often called a thread, of `DiscussionNote`s in an issue, merge
/// request, commit, or snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Discussion {
    id: String,
    individual_note: bool,
    notes: Vec<DiscussionNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DiscussionNoteType {
    DiscussionNote,
    DiffNote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotePosition {
    base_sha: String,
    start_sha: String,
    head_sha: String,
    old_path: Option<String>,
    new_path: Option<String>,
    position_type: String,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_range: Option<NotePositionLineRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotePositionLineRange {
    start: Option<NotePositionLine>,
    length: Option<NotePositionLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotePositionLine {
    line_code: String,

    #[serde(rename = "type")]
    position_type: Option<String>,

    old_line: Option<u32>,
    new_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NoteSuggestion {
    id: String,
    from_line: u32,
    to_line: u32,
    appliable: bool,
    applied: bool,
    from_content: String,
    to_content: String,
}

/// An individual item in a discussion on an issue, merge request, commit, or
/// snippet. Items of type DiscussionNote are not returned as part of the Note
/// API. Not available in the Events API. https://docs.gitlab.com/api/discussions/#list-project-merge-request-discussion-items
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscussionNote {
    /// The ID of the note.
    id: u64,

    /// The type of note.
    /// (DiscussionNote should probably be an
    /// enum of { DiscussionNote, DiffNote } instead technically)
    #[serde(rename = "type")]
    note_type: Option<DiscussionNoteType>,

    /// The content of the note.
    body: String,

    /// The author of the note.
    author: GitLabUser,

    /// When the note was created (ISO 8601 format).
    created_at: String,

    /// When the note was last updated (ISO 8601 format).
    updated_at: String,

    /// If `true`, a system note.
    system: bool,

    /// The ID of the noteable object.
    noteable_id: u64,

    /// The type of the noteable object.
    noteable_type: String,

    /// The ID of the project.
    project_id: u64,

    /// If `true`, the note is resolved (merge requests only).
    #[serde(default)]
    resolved: bool,

    /// If `true`, the note can be resolved.
    resolvable: bool,

    /// The user who resolved the note.
    resolved_by: Option<GitLabUser>,

    /// When the note was resolved (ISO 8601 format).
    resolved_at: Option<String>,

    /// Position information for diff notes.
    position: Option<NotePosition>,

    /// Array of suggestion objects for the note.
    #[serde(default)]
    suggestions: Vec<NoteSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequestDependency {
    pub id: u64,

    pub blocking_merge_request: MergeRequest,

    pub project_id: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitlab_client_new() {
        let client = GitLabForge::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            None::<&str>,
            false,
            true,
        )
        .expect("Failed to create client");

        assert_eq!(client.base_url, "https://gitlab.example.com");
        assert_eq!(client.source_project_id, "group/project");
        assert_eq!(client.target_project_id, "group/project");
        assert_eq!(client.token, "token123");
    }

    #[test]
    fn test_encode_project_id() {
        let client = GitLabForge::new(
            "https://gitlab.example.com".to_string(),
            "group/project".to_string(),
            "group/project".to_string(),
            "token123".to_string(),
            None::<&str>,
            false,
            true,
        )
        .expect("Failed to create client");

        let encoded = client.encoded_target_project_id();
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
            "group/project".to_string(),
            "token123".to_string(),
            Some(path.as_str()),
            false,
            true,
        )
        .expect("Failed to create client with multi-cert bundle");
    }
}
