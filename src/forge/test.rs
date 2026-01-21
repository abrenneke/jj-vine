use std::{collections::HashMap, sync::RwLock};

use bon::bon;

use crate::{
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{
        ApprovalStatus,
        CheckStatus,
        DiscussionCount,
        Forge,
        ForgeCreateMergeRequestOptions,
        ForgeMergeRequest,
        ForgeMergeRequestState,
        ForgeUser,
        MergeRequestStatus,
    },
};

#[derive(Debug)]
pub struct TestForge {
    project_id: String,
    source_project_id: String,
    target_project_id: String,
    base_url: String,
    users: HashMap<String, ForgeUser>,
    current_user: ForgeUser,
    state: RwLock<TestForgeState>,
}

#[derive(Debug)]
struct TestForgeState {
    next_merge_request_id: u64,
    merge_requests: HashMap<String, MergeRequest>,
}

#[bon]
impl TestForge {
    #[builder]
    pub fn new(
        #[builder(default)] project_id: String,
        #[builder(default)] source_project_id: String,
        #[builder(default)] target_project_id: String,
        #[builder(default)] base_url: String,

        current_user: Option<ForgeUser>,
        #[builder(default)] users: HashMap<String, ForgeUser>,
        #[builder(default)] merge_requests: HashMap<String, MergeRequest>,
    ) -> Self {
        Self {
            project_id,
            source_project_id,
            target_project_id,
            base_url,
            users,
            current_user: current_user.unwrap_or_else(|| ForgeUser {
                id: None,
                username: "test".to_string(),
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

    pub fn merge_request_lookup(&self) -> HashMap<String, ForgeMergeRequest> {
        self.state
            .read()
            .unwrap()
            .merge_requests
            .iter()
            .map(|(k, v)| (k.clone(), ForgeMergeRequest::Test(v.clone())))
            .collect()
    }
}

impl Forge for TestForge {
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

    async fn current_user(&self) -> Result<ForgeUser> {
        Ok(self.current_user.clone())
    }

    async fn user_by_username(&self, username: &str) -> Result<Option<ForgeUser>> {
        Ok(self.users.get(username).cloned())
    }

    async fn find_merge_request_by_source_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ForgeMergeRequest>> {
        Ok(self
            .state
            .read()
            .unwrap()
            .merge_requests
            .values()
            .find(|mr| mr.source_branch == branch)
            .map(|mr| ForgeMergeRequest::Test(mr.clone())))
    }

    async fn create_merge_request(
        &self,
        options: ForgeCreateMergeRequestOptions,
    ) -> Result<ForgeMergeRequest> {
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
            .build();
        self.state.write().unwrap().next_merge_request_id += 1;
        self.state
            .write()
            .unwrap()
            .merge_requests
            .insert(mr.id.clone(), mr.clone());
        Ok(ForgeMergeRequest::Test(mr))
    }

    async fn update_merge_request_base(
        &self,
        merge_request_iid: &str,
        new_base: &str,
    ) -> Result<ForgeMergeRequest> {
        let mut state = self.state.write().unwrap();
        let mr = state
            .merge_requests
            .get_mut(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        mr.target_branch = new_base.to_string();
        Ok(ForgeMergeRequest::Test(mr.clone()))
    }

    async fn update_merge_request_description(
        &self,
        merge_request_iid: &str,
        new_description: &str,
    ) -> Result<ForgeMergeRequest> {
        let mut state = self.state.write().unwrap();
        let mr = state
            .merge_requests
            .get_mut(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        mr.description = Some(new_description.to_string());
        Ok(ForgeMergeRequest::Test(mr.clone()))
    }

    async fn get_merge_request(&self, merge_request_iid: &str) -> Result<ForgeMergeRequest> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        Ok(ForgeMergeRequest::Test(mr.clone()))
    }

    async fn get_approval_status(&self, merge_request_iid: &str) -> Result<ApprovalStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.approval_status.clone())
    }

    async fn get_check_status(&self, merge_request_iid: &str) -> Result<CheckStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.check_status.clone())
    }

    async fn get_merge_request_status(
        &self,
        merge_request_iid: &str,
    ) -> Result<MergeRequestStatus> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        Ok(MergeRequestStatus {
            iid: mr.id.clone(),
            approval_status: mr.approval_status.clone(),
            check_status: mr.check_status.clone(),
        })
    }

    async fn num_open_discussions(&self, merge_request_iid: &str) -> Result<DiscussionCount> {
        let state = self.state.read().unwrap();
        let mr = state
            .merge_requests
            .get(merge_request_iid)
            .ok_or(Error::new("Merge request not found"))?;
        Ok(mr.num_open_discussions.clone())
    }
}

impl Default for TestForge {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl FormatMergeRequest for TestForge {
    fn format_merge_request_id(&self, mr_iid: &str) -> String {
        format!("#{}", mr_iid)
    }

    fn mr_name(&self) -> &'static str {
        "MR"
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
    pub state: ForgeMergeRequestState,

    #[builder(default)]
    pub created_at: jiff::Timestamp,

    #[builder(default)]
    pub author_username: String,

    #[builder(default)]
    pub assignees: Vec<ForgeUser>,

    #[builder(default)]
    pub reviewers: Vec<ForgeUser>,

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
}
