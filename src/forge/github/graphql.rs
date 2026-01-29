use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConnection<T> {
    pub nodes: Vec<T>,
    pub page_info: Option<RelayPageInfo>,
}

impl<T> Default for RelayConnection<T> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            page_info: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RelayPageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
    // We probably won't need startCursor and hasPreviousPage
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryResponse {
    pub repository: Option<GetDiscussionsQueryRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryRepository {
    pub pull_request: Option<GetDiscussionsQueryPullRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryPullRequest {
    pub reviews: Option<RelayConnection<GetDiscussionsQueryReview>>,
    pub comments: RelayConnection<GetDiscussionsQueryComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryReview {
    pub comments: RelayConnection<GetDiscussionsQueryComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryUser {
    pub login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GetDiscussionsQueryCommentMinimizedReason {
    Abuse,
    OffTopic,
    Outdated,
    Resolved,
    Duplicate,
    Spam,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDiscussionsQueryComment {
    pub author: Option<GetDiscussionsQueryUser>,
    pub body: String,
    pub created_at: String,
    pub editor: Option<GetDiscussionsQueryUser>,
    pub id: String,
    pub last_edited_at: Option<String>,
    pub is_minimized: bool,
    pub minimized_reason: Option<GetDiscussionsQueryCommentMinimizedReason>,
    pub published_at: Option<String>,
    pub viewer_can_minimize: bool,
}
