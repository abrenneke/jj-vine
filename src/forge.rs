pub mod azure;
pub mod forgejo;
pub mod github;
pub mod gitlab;
pub mod test;

use std::borrow::Cow;

use bon::Builder;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use crate::{
    config::{Config, ForgeType},
    description::FormatMergeRequest,
    error::{Error, Result},
};

pub trait ForgeUser: std::fmt::Debug {
    fn id(&self) -> Option<Cow<'_, str>>;

    fn username(&self) -> Option<Cow<'_, str>>;
}

pub trait BorrowId {
    type Id<'a>;
}

impl BorrowId for String {
    type Id<'a> = Cow<'a, str>;
}

impl BorrowId for u64 {
    type Id<'a> = u64;
}

impl BorrowId for i32 {
    type Id<'a> = i32;
}

pub trait ForgeMergeRequest: std::fmt::Debug {
    type User: ForgeUser;

    type Id: BorrowId;

    fn iid<'a>(&'a self) -> <Self::Id as BorrowId>::Id<'a>;

    fn title(&self) -> &str;

    fn description(&self) -> &str;

    fn source_branch(&self) -> &str;

    fn target_branch(&self) -> &str;

    fn state(&self) -> ForgeMergeRequestState;

    fn url(&self) -> Cow<'_, str>;

    fn edit_url(&self) -> Cow<'_, str>;

    fn author_username(&self) -> &str;

    fn created_at(&self) -> jiff::Timestamp;

    fn assignees(&self) -> Vec<Self::User>;

    fn reviewers(&self) -> Vec<Self::User>;

    fn is_draft(&self) -> bool;

    fn clone_boxed(
        &self,
    ) -> Box<dyn ForgeMergeRequest<User = Self::User, Id = Self::Id> + Send + Sync>
    where
        Self: Sync + Send;
}

pub struct ForgeMergeRequestWrapper<T, Id> {
    inner: Box<dyn ForgeMergeRequest<User = T, Id = Id> + Send + Sync>,
}

impl<T, Id> std::fmt::Debug for ForgeMergeRequestWrapper<T, Id> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ForgeMergeRequestWrapper {{ inner: {:?} }}", self.inner)
    }
}

impl<T, Id> ForgeMergeRequestWrapper<T, Id> {
    pub fn new(inner: impl ForgeMergeRequest<User = T, Id = Id> + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }
}

#[derive(Debug)]
pub struct AnyForgeMergeRequest(
    Box<dyn ForgeMergeRequest<User = AnyForgeUser, Id = String> + Send + Sync>,
);

impl AnyForgeMergeRequest {
    pub fn new<T>(inner: T) -> Self
    where
        T: ForgeMergeRequest + Send + Sync + 'static,
        T::User: ForgeUser + Send + Sync + 'static,
        for<'a> <T::Id as BorrowId>::Id<'a>: ToString,
    {
        Self(Box::new(ForgeMergeRequestWrapper::new(inner)))
    }
}

impl std::ops::Deref for AnyForgeMergeRequest {
    type Target = Box<dyn ForgeMergeRequest<User = AnyForgeUser, Id = String> + Send + Sync>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AnyForgeMergeRequest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, Id> ForgeMergeRequest for ForgeMergeRequestWrapper<T, Id>
where
    T: ForgeUser + Send + Sync + 'static,
    Id: BorrowId + 'static,
    for<'a> <Id as BorrowId>::Id<'a>: ToString,
{
    type Id = String;
    type User = AnyForgeUser;

    fn iid<'a>(&'a self) -> <Self::Id as BorrowId>::Id<'a> {
        Cow::Owned(self.inner.iid().to_string())
    }

    fn title(&self) -> &str {
        self.inner.title()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn source_branch(&self) -> &str {
        self.inner.source_branch()
    }

    fn target_branch(&self) -> &str {
        self.inner.target_branch()
    }

    fn state(&self) -> ForgeMergeRequestState {
        self.inner.state()
    }

    fn url(&self) -> Cow<'_, str> {
        self.inner.url()
    }

    fn edit_url(&self) -> Cow<'_, str> {
        self.inner.edit_url()
    }

    fn author_username(&self) -> &str {
        self.inner.author_username()
    }

    fn created_at(&self) -> jiff::Timestamp {
        self.inner.created_at()
    }

    fn assignees(&self) -> Vec<Self::User> {
        self.inner
            .assignees()
            .into_iter()
            .map::<Self::User, _>(|user| Box::new(user))
            .collect()
    }

    fn reviewers(&self) -> Vec<Self::User> {
        self.inner
            .reviewers()
            .into_iter()
            .map::<Self::User, _>(|user| Box::new(user))
            .collect()
    }

    fn is_draft(&self) -> bool {
        self.inner.is_draft()
    }

    fn clone_boxed(
        &self,
    ) -> Box<dyn ForgeMergeRequest<User = Self::User, Id = Self::Id> + Send + Sync> {
        Box::new(Self {
            inner: self.inner.clone_boxed(),
        })
    }
}

impl ForgeMergeRequest for AnyForgeMergeRequest {
    type User = AnyForgeUser;

    type Id = String;

    fn iid<'a>(&'a self) -> <Self::Id as BorrowId>::Id<'a> {
        (**self).iid()
    }

    fn title(&self) -> &str {
        (**self).title()
    }

    fn description(&self) -> &str {
        (**self).description()
    }

    fn source_branch(&self) -> &str {
        (**self).source_branch()
    }

    fn target_branch(&self) -> &str {
        (**self).target_branch()
    }

    fn state(&self) -> ForgeMergeRequestState {
        (**self).state()
    }

    fn url(&self) -> Cow<'_, str> {
        (**self).url()
    }

    fn edit_url(&self) -> Cow<'_, str> {
        (**self).edit_url()
    }

    fn author_username(&self) -> &str {
        (**self).author_username()
    }

    fn created_at(&self) -> jiff::Timestamp {
        (**self).created_at()
    }

    fn assignees(&self) -> Vec<Self::User> {
        (**self).assignees()
    }

    fn reviewers(&self) -> Vec<Self::User> {
        (**self).reviewers()
    }

    fn is_draft(&self) -> bool {
        (**self).is_draft()
    }

    fn clone_boxed(
        &self,
    ) -> Box<dyn ForgeMergeRequest<User = Self::User, Id = Self::Id> + Send + Sync> {
        (**self).clone_boxed()
    }
}

impl Clone for AnyForgeMergeRequest {
    fn clone(&self) -> Self {
        Self(self.0.clone_boxed())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ForgeMergeRequestState {
    #[default]
    Open,
    Closed,
    Merged,
}

/// Status of CI/Pipeline checks
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CheckStatus {
    /// All checks passed
    Success,
    /// Checks are still running
    Pending,
    /// Some checks failed
    Failed,
    /// No checks configured or required
    #[default]
    None,
}

/// Satisfaction of approval requirements
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalSatisfaction {
    /// All approval requirements are satisfied
    Satisfied,

    /// Some approval requirements are not satisfied
    Unsatisfied,

    /// Approval requirements are unknown
    Unknown,
}

/// Approval status of a merge request
#[derive(Debug, Clone)]
pub struct ApprovalStatus {
    /// Number of approvals received
    pub approved_count: u32,

    /// Number of approvals required
    pub required_count: u32,

    /// Number of approvals that are blocking the merge request
    pub blocking_count: u32,

    /// Whether approval requirements are satisfied
    pub satisfaction: ApprovalSatisfaction,
}

impl Default for ApprovalStatus {
    fn default() -> Self {
        Self {
            approved_count: 0,
            required_count: 0,
            blocking_count: 0,
            satisfaction: ApprovalSatisfaction::Unknown,
        }
    }
}

/// Complete status information for a merge request
#[derive(Debug, Clone)]
pub struct MergeRequestStatus {
    /// The internal ID of the merge request
    pub iid: String,

    /// CI/Pipeline check status
    pub check_status: CheckStatus,

    /// Approval status
    pub approval_status: ApprovalStatus,
}

impl MergeRequestStatus {
    pub fn ready_to_merge(&self) -> bool {
        self.approval_status.satisfaction == ApprovalSatisfaction::Satisfied
            && (self.check_status == CheckStatus::Success || self.check_status == CheckStatus::None)
    }
}

#[derive(Builder, Default)]
pub struct ForgeCreateMergeRequestOptions<User> {
    /// The source branch of the merge request
    pub source_branch: String,

    /// The target branch of the merge request
    pub target_branch: String,

    /// The title of the merge request
    pub title: String,

    /// The description of the merge request
    #[builder(required)]
    pub description: Option<String>,

    /// The initial assignees of the merge request
    pub assignees: Vec<User>,

    /// The initial reviewers of the merge request
    pub reviewers: Vec<User>,

    /// Whether to remove the source branch after the merge request is merged
    pub remove_source_branch: bool,

    /// Whether to squash the commits into a single commit
    pub squash: bool,

    /// Whether to open the merge request as a draft
    pub open_as_draft: bool,
}

impl<T> ForgeCreateMergeRequestOptions<T> {
    pub fn from_other<U>(
        options: ForgeCreateMergeRequestOptions<U>,
    ) -> Result<Self, <T as TryFrom<U>>::Error>
    where
        T: TryFrom<U>,
        T::Error: std::error::Error,
    {
        Ok(Self {
            source_branch: options.source_branch,
            target_branch: options.target_branch,
            title: options.title,
            description: options.description,
            assignees: options
                .assignees
                .into_iter()
                .map(|user| T::try_from(user))
                .collect::<Result<Vec<_>, <T as TryFrom<U>>::Error>>()?,
            reviewers: options
                .reviewers
                .into_iter()
                .map(|user| T::try_from(user))
                .collect::<Result<Vec<_>, <T as TryFrom<U>>::Error>>()?,
            remove_source_branch: options.remove_source_branch,
            squash: options.squash,
            open_as_draft: options.open_as_draft,
        })
    }

    pub fn into_other<U>(
        self,
    ) -> Result<ForgeCreateMergeRequestOptions<U>, <U as TryFrom<T>>::Error>
    where
        U: TryFrom<T>,
        U::Error: std::error::Error,
    {
        ForgeCreateMergeRequestOptions::from_other(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserId<T>(pub T);

impl<T> TryFrom<AnyForgeUser> for UserId<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    type Error = Error;

    fn try_from(user: AnyForgeUser) -> Result<Self, Error> {
        Ok(Self(
            user.id()
                .ok_or(Error::new("User ID is required"))?
                .parse()
                .map_err(|e: <T as std::str::FromStr>::Err| Error::new(e.to_string()))?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserName<T>(pub T);

impl<T> TryFrom<AnyForgeUser> for UserName<T>
where
    T: for<'a> From<Cow<'a, str>>,
{
    type Error = Error;

    fn try_from(user: AnyForgeUser) -> Result<Self, Error> {
        Ok(Self(
            user.username()
                .ok_or(Error::new("User name is required"))?
                .into(),
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscussionCount {
    /// The total number of discussions.
    pub all: u32,

    /// The number of unresolved (resolvable) discussions.
    pub unresolved: u32,

    /// The number of resolved discussions.
    pub resolved: u32,
}

/// A trait for a code forge (e.g. GitLab, GitHub, Forgejo, etc.)
#[enum_dispatch]
pub trait Forge: Send + Sync + FormatMergeRequest {
    type User: Send + Sync + ForgeUser;

    type MergeRequest: Send + Sync + ForgeMergeRequest<User = Self::User, Id = Self::Id>;

    type UserId: Send + Sync;

    /// The project ID of the project in the forge. E.g. "group/project" or
    /// "12345" for a numeric project ID. Combined with the base URL, this forms
    /// the full URL to the project in the forge.
    fn project_id(&self) -> &str;

    /// The project ID where branches are pushed (source/fork project).
    fn source_project_id(&self) -> &str;

    /// The project ID where MRs/PRs are created (target/upstream project).
    fn target_project_id(&self) -> &str;

    /// The base URL of the forge. E.g. <https://gitlab.example.com>.
    fn base_url(&self) -> &str;

    /// The full URL to the project in the forge. E.g. <https://gitlab.example.com/group/project>.
    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url(), self.project_id())
    }

    /// Get the current authenticated user in the forge.
    async fn current_user(&self) -> Result<Self::User>;

    /// Gets a user in the forge by their username.
    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>>;

    /// Find merge request by source branch name. Returns the first MR found.
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<Self::MergeRequest>>;

    /// Create a new merge request in the forge for the project.
    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest>;

    /// Update the target branch (base) of an existing merge request.
    async fn update_merge_request_base<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
        new_base: &str,
    ) -> Result<Self::MergeRequest>;

    /// Update the description of an existing merge request.
    async fn update_merge_request_description<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
        new_description: &str,
    ) -> Result<Self::MergeRequest>;

    /// Get a specific merge request by IID.
    async fn get_merge_request<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
    ) -> Result<Self::MergeRequest>;

    /// Get approval status for a merge request.
    async fn get_approval_status<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
    ) -> Result<ApprovalStatus>;

    /// Get CI/pipeline check status for a merge request.
    async fn get_check_status<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
    ) -> Result<CheckStatus>;

    /// Get complete status information for a merge request.
    async fn get_merge_request_status<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
    ) -> Result<MergeRequestStatus>;

    /// Get the number of open discussions for a merge request.
    async fn num_open_discussions<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
    ) -> Result<DiscussionCount>;

    /// Sync dependent merge requests for a merge request.
    /// Only currently supported for GitLab. No-op for other forges.
    /// Returns true if any changes were made.
    async fn sync_dependent_merge_requests<'a>(
        &'a self,
        merge_request_iid: <Self::Id as BorrowId>::Id<'a>,
        dependent_merge_request_iids: &[<Self::Id as BorrowId>::Id<'a>],
    ) -> Result<bool>;
}

pub enum ForgeImpl {
    GitLab(gitlab::GitLabForge),
    GitHub(github::GitHubForge),
    Forgejo(forgejo::ForgejoForge),
    Test(test::TestForge),
    AzureDevOps(azure::AzureDevOpsForge),
}

impl ForgeUser for AnyForgeUser {
    fn id(&self) -> Option<Cow<'_, str>> {
        (**self).id()
    }

    fn username(&self) -> Option<Cow<'_, str>> {
        (**self).username()
    }
}

pub type AnyForgeUser = Box<dyn ForgeUser + Send + Sync>;

impl Forge for ForgeImpl {
    type User = AnyForgeUser;

    type MergeRequest = AnyForgeMergeRequest;

    type UserId = AnyForgeUser;

    /// The project ID of the project in the forge. E.g. "group/project" or
    /// "12345" for a numeric project ID. Combined with the base URL, this forms
    /// the full URL to the project in the forge.
    fn project_id(&self) -> &str {
        match self {
            ForgeImpl::GitLab(forge) => forge.project_id(),
            ForgeImpl::GitHub(forge) => forge.project_id(),
            ForgeImpl::Forgejo(forge) => forge.project_id(),
            ForgeImpl::Test(forge) => forge.project_id(),
            ForgeImpl::AzureDevOps(forge) => forge.project_id(),
        }
    }

    /// The project ID where branches are pushed (source/fork project).
    fn source_project_id(&self) -> &str {
        match self {
            ForgeImpl::GitLab(forge) => forge.source_project_id(),
            ForgeImpl::GitHub(forge) => forge.source_project_id(),
            ForgeImpl::Forgejo(forge) => forge.source_project_id(),
            ForgeImpl::Test(forge) => forge.source_project_id(),
            ForgeImpl::AzureDevOps(forge) => forge.source_project_id(),
        }
    }

    /// The project ID where MRs/PRs are created (target/upstream project).
    fn target_project_id(&self) -> &str {
        match self {
            ForgeImpl::GitLab(forge) => forge.target_project_id(),
            ForgeImpl::GitHub(forge) => forge.target_project_id(),
            ForgeImpl::Forgejo(forge) => forge.target_project_id(),
            ForgeImpl::Test(forge) => forge.target_project_id(),
            ForgeImpl::AzureDevOps(forge) => forge.target_project_id(),
        }
    }

    /// The base URL of the forge. E.g. <https://gitlab.example.com>.
    fn base_url(&self) -> &str {
        match self {
            ForgeImpl::GitLab(forge) => forge.base_url(),
            ForgeImpl::GitHub(forge) => forge.base_url(),
            ForgeImpl::Forgejo(forge) => forge.base_url(),
            ForgeImpl::Test(forge) => forge.base_url(),
            ForgeImpl::AzureDevOps(forge) => forge.base_url(),
        }
    }

    /// The full URL to the project in the forge. E.g. <https://gitlab.example.com/group/project>.
    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url(), self.project_id())
    }

    /// Get the current authenticated user in the forge.
    async fn current_user(&self) -> Result<Self::User> {
        let user: Self::User = match self {
            ForgeImpl::GitLab(forge) => Box::new(forge.current_user().await?),
            ForgeImpl::GitHub(forge) => Box::new(forge.current_user().await?),
            ForgeImpl::Forgejo(forge) => Box::new(forge.current_user().await?),
            ForgeImpl::Test(forge) => Box::new(forge.current_user().await?),
            ForgeImpl::AzureDevOps(forge) => Box::new(forge.current_user().await?),
        };
        Ok(user)
    }

    /// Gets a user in the forge by their username.
    async fn user_by_username(&self, username: &str) -> Result<Option<Self::User>> {
        let user: Option<Self::User> = match self {
            ForgeImpl::GitLab(forge) => match forge.user_by_username(username).await? {
                Some(user) => Some(Box::new(user)),
                None => None,
            },
            ForgeImpl::GitHub(forge) => match forge.user_by_username(username).await? {
                Some(user) => Some(Box::new(user)),
                None => None,
            },
            ForgeImpl::Forgejo(forge) => match forge.user_by_username(username).await? {
                Some(user) => Some(Box::new(user)),
                None => None,
            },
            ForgeImpl::Test(forge) => match forge.user_by_username(username).await? {
                Some(user) => Some(Box::new(user)),
                None => None,
            },
            ForgeImpl::AzureDevOps(forge) => match forge.user_by_username(username).await? {
                Some(user) => Some(Box::new(user)),
                None => None,
            },
        };
        Ok(user)
    }

    /// Find merge request by source branch name. Returns the first MR found.
    /// with the given source branch, or None if not found
    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        let mr: Option<Self::MergeRequest> = match self {
            ForgeImpl::GitLab(forge) => forge
                .find_merge_request_by_source_branch(branch)
                .await?
                .map(AnyForgeMergeRequest::new),
            ForgeImpl::GitHub(forge) => forge
                .find_merge_request_by_source_branch(branch)
                .await?
                .map(AnyForgeMergeRequest::new),
            ForgeImpl::Forgejo(forge) => forge
                .find_merge_request_by_source_branch(branch)
                .await?
                .map(AnyForgeMergeRequest::new),
            ForgeImpl::Test(forge) => forge
                .find_merge_request_by_source_branch(branch)
                .await?
                .map(AnyForgeMergeRequest::new),
            ForgeImpl::AzureDevOps(forge) => forge
                .find_merge_request_by_source_branch(branch)
                .await?
                .map(AnyForgeMergeRequest::new),
        };
        Ok(mr)
    }

    /// Create a new merge request in the forge for the project.
    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        let mr: Self::MergeRequest = match self {
            ForgeImpl::GitLab(forge) => {
                AnyForgeMergeRequest::new(forge.create_merge_request(options.into_other()?).await?)
            }
            ForgeImpl::GitHub(forge) => {
                AnyForgeMergeRequest::new(forge.create_merge_request(options.into_other()?).await?)
            }
            ForgeImpl::Forgejo(forge) => {
                AnyForgeMergeRequest::new(forge.create_merge_request(options.into_other()?).await?)
            }
            ForgeImpl::Test(forge) => {
                AnyForgeMergeRequest::new(forge.create_merge_request(options.into_other()?).await?)
            }
            ForgeImpl::AzureDevOps(forge) => {
                AnyForgeMergeRequest::new(forge.create_merge_request(options.into_other()?).await?)
            }
        };
        Ok(mr)
    }

    /// Update the target branch (base) of an existing merge request.
    async fn update_merge_request_base(
        &self,
        merge_request_iid: Cow<'_, str>,
        new_base: &str,
    ) -> Result<Self::MergeRequest> {
        let mr: Self::MergeRequest = match self {
            ForgeImpl::GitLab(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_base(merge_request_iid.parse::<u64>()?, new_base)
                    .await?,
            ),

            ForgeImpl::GitHub(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_base(merge_request_iid.parse::<u64>()?, new_base)
                    .await?,
            ),

            ForgeImpl::Forgejo(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_base(merge_request_iid.parse::<u64>()?, new_base)
                    .await?,
            ),

            ForgeImpl::Test(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_base(merge_request_iid, new_base)
                    .await?,
            ),

            ForgeImpl::AzureDevOps(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_base(merge_request_iid.parse::<i32>()?, new_base)
                    .await?,
            ),
        };
        Ok(mr)
    }

    /// Update the description of an existing merge request.
    async fn update_merge_request_description(
        &self,
        merge_request_iid: Cow<'_, str>,
        new_description: &str,
    ) -> Result<Self::MergeRequest> {
        let mr: Self::MergeRequest = match self {
            ForgeImpl::GitLab(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_description(
                        merge_request_iid.parse::<u64>()?,
                        new_description,
                    )
                    .await?,
            ),

            ForgeImpl::GitHub(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_description(
                        merge_request_iid.parse::<u64>()?,
                        new_description,
                    )
                    .await?,
            ),

            ForgeImpl::Forgejo(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_description(
                        merge_request_iid.parse::<u64>()?,
                        new_description,
                    )
                    .await?,
            ),

            ForgeImpl::Test(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_description(merge_request_iid, new_description)
                    .await?,
            ),

            ForgeImpl::AzureDevOps(forge) => AnyForgeMergeRequest::new(
                forge
                    .update_merge_request_description(
                        merge_request_iid.parse::<i32>()?,
                        new_description,
                    )
                    .await?,
            ),
        };
        Ok(mr)
    }

    /// Get a specific merge request by IID.
    async fn get_merge_request(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<Self::MergeRequest> {
        let mr: Self::MergeRequest = match self {
            ForgeImpl::GitLab(forge) => AnyForgeMergeRequest::new(
                forge
                    .get_merge_request(merge_request_iid.parse::<u64>()?)
                    .await?,
            ),
            ForgeImpl::GitHub(forge) => AnyForgeMergeRequest::new(
                forge
                    .get_merge_request(merge_request_iid.parse::<u64>()?)
                    .await?,
            ),
            ForgeImpl::Forgejo(forge) => AnyForgeMergeRequest::new(
                forge
                    .get_merge_request(merge_request_iid.parse::<u64>()?)
                    .await?,
            ),
            ForgeImpl::Test(forge) => {
                AnyForgeMergeRequest::new(forge.get_merge_request(merge_request_iid).await?)
            }
            ForgeImpl::AzureDevOps(forge) => AnyForgeMergeRequest::new(
                forge
                    .get_merge_request(merge_request_iid.parse::<i32>()?)
                    .await?,
            ),
        };
        Ok(mr)
    }

    /// Get approval status for a merge request.
    async fn get_approval_status(&self, merge_request_iid: Cow<'_, str>) -> Result<ApprovalStatus> {
        match self {
            ForgeImpl::GitLab(forge) => {
                forge
                    .get_approval_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::GitHub(forge) => {
                forge
                    .get_approval_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Forgejo(forge) => {
                forge
                    .get_approval_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Test(forge) => forge.get_approval_status(merge_request_iid).await,
            ForgeImpl::AzureDevOps(forge) => {
                forge
                    .get_approval_status(merge_request_iid.parse::<i32>()?)
                    .await
            }
        }
    }

    /// Get CI/pipeline check status for a merge request.
    async fn get_check_status(&self, merge_request_iid: Cow<'_, str>) -> Result<CheckStatus> {
        match self {
            ForgeImpl::GitLab(forge) => {
                forge
                    .get_check_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::GitHub(forge) => {
                forge
                    .get_check_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Forgejo(forge) => {
                forge
                    .get_check_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Test(forge) => forge.get_check_status(merge_request_iid).await,
            ForgeImpl::AzureDevOps(forge) => {
                forge
                    .get_check_status(merge_request_iid.parse::<i32>()?)
                    .await
            }
        }
    }

    /// Get complete status information for a merge request.
    async fn get_merge_request_status(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<MergeRequestStatus> {
        match self {
            ForgeImpl::GitLab(forge) => {
                forge
                    .get_merge_request_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::GitHub(forge) => {
                forge
                    .get_merge_request_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Forgejo(forge) => {
                forge
                    .get_merge_request_status(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Test(forge) => forge.get_merge_request_status(merge_request_iid).await,
            ForgeImpl::AzureDevOps(forge) => {
                forge
                    .get_merge_request_status(merge_request_iid.parse::<i32>()?)
                    .await
            }
        }
    }

    /// Get the number of open discussions for a merge request.
    async fn num_open_discussions(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<DiscussionCount> {
        match self {
            ForgeImpl::GitLab(forge) => {
                forge
                    .num_open_discussions(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::GitHub(forge) => {
                forge
                    .num_open_discussions(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Forgejo(forge) => {
                forge
                    .num_open_discussions(merge_request_iid.parse::<u64>()?)
                    .await
            }
            ForgeImpl::Test(forge) => forge.num_open_discussions(merge_request_iid).await,
            ForgeImpl::AzureDevOps(forge) => {
                forge
                    .num_open_discussions(merge_request_iid.parse::<i32>()?)
                    .await
            }
        }
    }

    /// Sync dependent merge requests for a merge request.
    /// Only currently supported for GitLab. No-op for other forges.
    /// Returns true if any changes were made.
    async fn sync_dependent_merge_requests(
        &self,
        merge_request_iid: Cow<'_, str>,
        dependent_merge_request_iids: &[Cow<'_, str>],
    ) -> Result<bool> {
        match self {
            ForgeImpl::GitLab(forge) => {
                forge
                    .sync_dependent_merge_requests(
                        merge_request_iid.parse::<u64>()?,
                        dependent_merge_request_iids
                            .iter()
                            .map(|s| s.parse::<u64>().map_err(Error::from))
                            .collect::<Result<Vec<_>>>()?
                            .as_slice(),
                    )
                    .await
            }
            ForgeImpl::GitHub(forge) => {
                forge
                    .sync_dependent_merge_requests(
                        merge_request_iid.parse::<u64>()?,
                        dependent_merge_request_iids
                            .iter()
                            .map(|s| s.parse::<u64>().map_err(Error::from))
                            .collect::<Result<Vec<_>>>()?
                            .as_slice(),
                    )
                    .await
            }
            ForgeImpl::Forgejo(forge) => {
                forge
                    .sync_dependent_merge_requests(
                        merge_request_iid.parse::<u64>()?,
                        dependent_merge_request_iids
                            .iter()
                            .map(|s| s.parse::<u64>().map_err(Error::from))
                            .collect::<Result<Vec<_>>>()?
                            .as_slice(),
                    )
                    .await
            }
            ForgeImpl::Test(forge) => {
                forge
                    .sync_dependent_merge_requests(merge_request_iid, dependent_merge_request_iids)
                    .await
            }
            ForgeImpl::AzureDevOps(forge) => {
                forge
                    .sync_dependent_merge_requests(
                        merge_request_iid.parse::<i32>()?,
                        dependent_merge_request_iids
                            .iter()
                            .map(|s| s.parse::<i32>().map_err(Error::from))
                            .collect::<Result<Vec<_>>>()?
                            .as_slice(),
                    )
                    .await
            }
        }
    }
}

impl FormatMergeRequest for ForgeImpl {
    type Id = String;

    fn format_merge_request_id(&self, mr_iid: Cow<'_, str>) -> String {
        match self {
            ForgeImpl::GitLab(forge) => forge
                .format_merge_request_id(mr_iid.parse::<u64>().expect("Invalid merge request ID")),
            ForgeImpl::GitHub(forge) => forge
                .format_merge_request_id(mr_iid.parse::<u64>().expect("Invalid pull request ID")),
            ForgeImpl::Forgejo(forge) => forge
                .format_merge_request_id(mr_iid.parse::<u64>().expect("Invalid pull request ID")),
            ForgeImpl::Test(forge) => forge.format_merge_request_id(mr_iid),
            ForgeImpl::AzureDevOps(forge) => forge
                .format_merge_request_id(mr_iid.parse::<i32>().expect("Invalid pull request ID")),
        }
    }

    fn mr_name(&self) -> &'static str {
        match self {
            ForgeImpl::GitLab(forge) => forge.mr_name(),
            ForgeImpl::GitHub(forge) => forge.mr_name(),
            ForgeImpl::Forgejo(forge) => forge.mr_name(),
            ForgeImpl::Test(forge) => forge.mr_name(),
            ForgeImpl::AzureDevOps(forge) => forge.mr_name(),
        }
    }
}

impl ForgeImpl {
    /// Create a new forge. Looks for a jj-vine config in the current directory.
    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let config = Config::load(&cwd)?;
        Self::new(&config)
    }

    pub fn new(config: &Config) -> Result<Self> {
        config.validate()?;

        match config.forge {
            ForgeType::GitLab => {
                let source = config.gitlab.source_project();
                let target = config.gitlab.target_project();
                gitlab::GitLabForge::new(
                    config.gitlab.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.gitlab.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                    config.gitlab.create_merge_request_dependencies,
                )
                .map(ForgeImpl::GitLab)
            }
            ForgeType::GitHub => {
                let source = config.github.source_project();
                let target = config.github.target_project();
                github::GitHubForge::new(
                    config.github.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.github.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                )
                .map(ForgeImpl::GitHub)
            }
            ForgeType::Forgejo => {
                let source = config.forgejo.source_project();
                let target = config.forgejo.target_project();
                forgejo::ForgejoForge::new(
                    config.forgejo.host.clone(),
                    source.to_string(),
                    target.to_string(),
                    config.forgejo.token.clone(),
                    config.ca_bundle.clone(),
                    config.tls_accept_non_compliant_certs,
                    config.forgejo.wip_prefix.clone(),
                )
                .map(ForgeImpl::Forgejo)
            }
            ForgeType::AzureDevOps => azure::AzureDevOpsForge::builder()
                .base_url(config.azure.host.clone())
                .vssps_base_url(config.azure.vssps_host.clone())
                .source_project_id(config.azure.source_project_id())
                .target_project_id(config.azure.target_project_id())
                .token(config.azure.token.clone())
                .maybe_source_repository_name(config.azure.source_repository_name.clone())
                .maybe_target_repository_name(
                    config.azure.target_repository_name().map(str::to_string),
                )
                .maybe_source_repository_id(config.azure.source_repository_id.clone())
                .maybe_target_repository_id(config.azure.target_repository_id().map(str::to_string))
                .accept_non_compliant_certs(config.tls_accept_non_compliant_certs)
                .maybe_ca_bundle(config.ca_bundle.clone())
                .build()
                .map(ForgeImpl::AzureDevOps),
        }
    }
}
