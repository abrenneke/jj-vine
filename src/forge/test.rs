#![cfg(test)]
#![allow(clippy::missing_panics_doc, reason = "tests")]
#![allow(clippy::module_name_repetitions, reason = "it's fine")]

use std::{borrow::Cow, collections::HashMap, sync::RwLock};

use bon::bon;

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{
        AnyForgeMergeRequest,
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

#[derive(Debug)]
pub struct TestForge {
    project_id: String,
    source_project_id: String,
    target_project_id: String,
    base_url: String,
    id_expands_title: bool,
    users: HashMap<String, TestForgeUser>,
    current_user: TestForgeUser,
    state: RwLock<TestForgeState>,
}

#[derive(Debug, Clone)]
pub struct TestForgeUser {
    pub id: String,
    pub username: String,
}

impl UserLike for TestForgeUser {
    fn id(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.id))
    }

    fn username(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.username))
    }
}

#[derive(Debug)]
struct TestForgeState {
    next_merge_request_id: u64,
    merge_requests: HashMap<String, MergeRequest>,
}

#[bon]
impl TestForge {
    #[builder]
    #[expect(clippy::single_call_fn, reason = "necessary")]
    pub fn new(
        #[builder(default)] project_id: String,
        #[builder(default)] source_project_id: String,
        #[builder(default)] target_project_id: String,
        #[builder(default)] base_url: String,
        #[builder(default)] id_expands_title: bool,

        current_user: Option<TestForgeUser>,
        #[builder(default)] users: HashMap<String, TestForgeUser>,
        #[builder(default)] merge_requests: HashMap<String, MergeRequest>,
    ) -> Self {
        Self {
            project_id,
            source_project_id,
            target_project_id,
            base_url,
            id_expands_title,
            users,
            current_user: current_user.unwrap_or_else(|| TestForgeUser {
                id: "test".to_owned(),
                username: "test".to_owned(),
            }),
            state: RwLock::new(TestForgeState {
                next_merge_request_id: 1,
                merge_requests,
            }),
        }
    }

    pub fn add_merge_request(&mut self, mr: MergeRequest) {
        self.state
            .write()
            .unwrap()
            .merge_requests
            .insert(mr.id.clone(), mr);
    }

    pub fn merge_request_lookup(&self) -> HashMap<String, AnyForgeMergeRequest> {
        self.state
            .read()
            .unwrap()
            .merge_requests
            .iter()
            .map(|(k, v)| (k.clone(), AnyForgeMergeRequest::new(v.clone())))
            .collect()
    }
}

impl Forge for TestForge {
    type User = TestForgeUser;

    type MergeRequest = MergeRequest;

    type UserId = UserId<String>;

    fn project_id(&self) -> &str {
        &self.project_id
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

    fn current_user(&self) -> impl Future<Output = Result<Self::User>> {
        core::future::ready(Ok(self.current_user.clone()))
    }

    fn user_by_username(&self, username: &str) -> impl Future<Output = Result<Option<Self::User>>> {
        core::future::ready(Ok(self.users.get(username).cloned()))
    }

    fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> impl Future<Output = Result<Option<Self::MergeRequest>>> {
        core::future::ready(Ok(self
            .state
            .read()
            .unwrap()
            .merge_requests
            .values()
            .find(|mr| mr.source_branch == branch)
            .cloned()))
    }

    async fn create_merge_request(
        &self,
        options: CreateMergeRequestOptions<Self::UserId>,
    ) -> Result<Self::MergeRequest> {
        let mr = MergeRequest::builder()
            .id(self
                .state
                .write()
                .unwrap()
                .next_merge_request_id
                .to_string())
            .title(options.title)
            .maybe_description(options.description)
            .source_branch(options.source_branch)
            .target_branch(options.target_branch)
            .assignees(
                options
                    .assignees
                    .into_iter()
                    .map(|id| self.users[&id.0].clone())
                    .collect(),
            )
            .reviewers(
                options
                    .reviewers
                    .into_iter()
                    .map(|id| self.users[&id.0].clone())
                    .collect(),
            )
            .build();
        {
            let mut state = self.state.write().unwrap();
            state.next_merge_request_id = state.next_merge_request_id.strict_add(1);
        };
        self.state
            .write()
            .unwrap()
            .merge_requests
            .insert(mr.id.clone(), mr.clone());
        Ok(mr)
    }

    async fn update_merge_request_base(
        &self,
        merge_request_iid: Cow<'_, str>,
        new_base: &str,
    ) -> Result<Self::MergeRequest> {
        let mut state = self.state.write().unwrap();
        let mr = state
            .merge_requests
            .get_mut(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        mr.target_branch = new_base.to_owned();
        Ok(mr.clone())
    }

    async fn update_merge_request_info(
        &self,
        merge_request_iid: Cow<'_, str>,
        UpdateMergeRequestInfoOptions {
            title,
            description,
            draft,
            current_title: _current_title, // Unneeded for TestForge
            current_is_draft: _current_is_draft, // Unneeded for TestForge
        }: UpdateMergeRequestInfoOptions,
    ) -> Result<Self::MergeRequest> {
        let mut state = self.state.write().unwrap();
        let mr = state
            .merge_requests
            .get_mut(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;

        if let Some(description) = description {
            mr.description = Some(description.clone());
        }
        if let Some(title) = title {
            mr.title = title.clone();
        }
        if let Some(draft) = draft {
            mr.draft = draft;
        }
        Ok(mr.clone())
    }

    async fn get_merge_request(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<Self::MergeRequest> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.clone())
    }

    async fn get_approval_status(&self, merge_request_iid: Cow<'_, str>) -> Result<ApprovalStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.approval_status.clone())
    }

    async fn get_check_status(&self, merge_request_iid: Cow<'_, str>) -> Result<CheckStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.check_status.clone())
    }

    async fn get_merge_request_status(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<MergeRequestStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        Ok(MergeRequestStatus {
            iid: mr.id.clone(),
            approval_status: mr.approval_status.clone(),
            check_status: mr.check_status.clone(),
        })
    }

    async fn num_open_discussions(
        &self,
        merge_request_iid: Cow<'_, str>,
    ) -> Result<DiscussionCount> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid.as_ref())
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.num_open_discussions.clone())
    }

    async fn sync_dependent_merge_requests(
        &self,
        _merge_request_iid: Cow<'_, str>,
        _dependent_merge_request_iids: &[Cow<'_, str>],
    ) -> ResultWithWarnings<bool> {
        // Only supported for GitLab
        Ok(false).into()
    }

    #[doc = " The full URL to the project in the forge. E.g. <https://gitlab.example.com/group/project>."]
    fn project_url(&self) -> String {
        format!("{}/{}", self.base_url(), self.project_id())
    }

    #[doc = " Find merge request by source branch name and base branch name. Returns"]
    #[doc = " the first MR found. More optimized than"]
    #[doc = " `find_merge_request_by_source_branch` for some forges."]
    async fn find_merge_request_by_source_branch_base_branch(
        &self,
        source_branch: &str,
        #[expect(unused, reason = "keep argument name")] base_branch: &str,
    ) -> Result<Option<Self::MergeRequest>> {
        self.find_merge_request_by_source_branch(source_branch)
            .await
    }

    #[doc = " Returns true if the forge supports dependent merge requests."]
    fn supports_dependent_merge_requests(&self) -> bool {
        false
    }
}

impl Default for TestForge {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl FormatMergeRequest for TestForge {
    type Id = String;

    fn format_merge_request_id(&self, mr_iid: Cow<'_, str>) -> String {
        format!("#{mr_iid}")
    }

    fn mr_name(&self) -> &'static str {
        "MR"
    }

    fn id_expands_title(&self) -> bool {
        self.id_expands_title
    }
}

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct MergeRequest {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub source_branch: String,
    pub target_branch: String,

    #[builder(default)]
    pub state: MergeRequestState,

    #[builder(default)]
    pub created_at: jiff::Timestamp,

    #[builder(default)]
    pub author_username: String,

    #[builder(default)]
    pub assignees: Vec<TestForgeUser>,

    #[builder(default)]
    pub reviewers: Vec<TestForgeUser>,

    #[builder(default)]
    pub url: String,

    #[builder(default)]
    pub approval_count: u32,

    #[builder(default)]
    pub required_approval_count: u32,

    #[builder(default)]
    pub approval_status: ApprovalStatus,

    #[builder(default)]
    pub check_status: CheckStatus,

    #[builder(default)]
    pub num_open_discussions: DiscussionCount,

    #[builder(default)]
    pub draft: bool,
}

impl MergeRequestLike for MergeRequest {
    type User = TestForgeUser;

    type Id = String;

    fn iid(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.id)
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

    fn state(&self) -> MergeRequestState {
        self.state
    }

    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }

    fn edit_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }

    fn author_username(&self) -> &str {
        &self.author_username
    }

    fn created_at(&self) -> jiff::Timestamp {
        self.created_at
    }

    fn assignees(&self) -> Vec<Self::User> {
        self.assignees.clone()
    }

    fn reviewers(&self) -> Vec<Self::User> {
        self.reviewers.clone()
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
