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

pub mod find_pr_by_head_ref {
    use serde::{Deserialize, Serialize};

    use super::{
        super::{BranchRef, GitHubRepo, GitHubUser, PullRequest},
        RelayConnection,
    };

    pub fn query() -> &'static str {
        r#"
query FindPRByHeadRef($owner: String!, $repositoryName: String!, $headRefName: String!) {
  repository(owner: $owner, name: $repositoryName) {
    pullRequests(states: OPEN, headRefName: $headRefName, first: 10) {
      nodes {
        number
        databaseId
        title
        body
        headRefName
        headRepository { nameWithOwner }
        headRefOid
        baseRefName
        baseRepository { nameWithOwner }
        baseRefOid
        state
        url
        isDraft
        merged
        createdAt
        author { login }
        assignees(first: 100) { nodes { databaseId login } }
        reviewRequests(first: 100) {
          nodes {
            requestedReviewer {
              __typename
              ... on User { databaseId login }
            }
          }
        }
      }
    }
  }
}
"#
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub repository: Option<PRLookupRepo>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRLookupRepo {
        pub pull_requests: RelayConnection<PRNode>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRNode {
        pub number: u64,
        pub database_id: u64,
        pub title: String,
        pub body: Option<String>,
        pub head_ref_name: String,
        pub head_repository: Option<PRRepo>,
        pub head_ref_oid: String,
        pub base_ref_name: String,
        pub base_repository: Option<PRRepo>,
        pub base_ref_oid: String,
        pub state: String,
        pub url: String,
        pub is_draft: bool,
        pub merged: bool,
        pub created_at: String,
        pub author: Option<PRAuthor>,
        pub assignees: RelayConnection<PRUser>,
        pub review_requests: RelayConnection<PRReviewRequest>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRRepo {
        pub name_with_owner: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRAuthor {
        pub login: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRUser {
        pub database_id: u64,
        pub login: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PRReviewRequest {
        pub requested_reviewer: Option<RequestedReviewer>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "__typename")]
    pub enum RequestedReviewer {
        User(PRUser),
        #[serde(other)]
        Other,
    }

    impl PRNode {
        pub fn into_pull_request(self) -> PullRequest {
            PullRequest {
                number: self.number,
                id: self.database_id,
                title: self.title,
                body: self.body,
                head: BranchRef {
                    ref_name: self.head_ref_name,
                    sha: self.head_ref_oid,
                    repo: self.head_repository.map(|r| GitHubRepo {
                        full_name: r.name_with_owner,
                    }),
                },
                base: BranchRef {
                    ref_name: self.base_ref_name,
                    sha: self.base_ref_oid,
                    repo: self.base_repository.map(|r| GitHubRepo {
                        full_name: r.name_with_owner,
                    }),
                },
                state: match self.state.as_str() {
                    "OPEN" => "open".to_string(),
                    "CLOSED" => "closed".to_string(),
                    "MERGED" => "closed".to_string(),
                    other => other.to_lowercase(),
                },
                html_url: self.url,
                user: GitHubUser {
                    id: 0,
                    login: self.author.map_or_else(String::new, |a| a.login),
                },
                created_at: self.created_at,
                assignees: self
                    .assignees
                    .nodes
                    .into_iter()
                    .map(|u| GitHubUser {
                        id: u.database_id,
                        login: u.login,
                    })
                    .collect(),
                requested_reviewers: self
                    .review_requests
                    .nodes
                    .into_iter()
                    .filter_map(|rr| match rr.requested_reviewer? {
                        RequestedReviewer::User(u) => Some(GitHubUser {
                            id: u.database_id,
                            login: u.login,
                        }),
                        _ => None,
                    })
                    .collect(),
                draft: self.is_draft,
                merged: self.merged,
            }
        }
    }
}

pub mod set_pr_is_draft {
    use serde::{Deserialize, Serialize};

    pub fn query() -> &'static str {
        r#"
mutation SetPRIsDraft($id: ID!) {
  convertPullRequestToDraft(input: { pullRequestId: $id }) {
    pullRequest {
      id
      isDraft
    }
  }
}
"#
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Variables {
        pub id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub convert_pull_request_to_draft: ConvertPullRequestToDraft,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConvertPullRequestToDraft {
        pub pull_request: PullRequest,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PullRequest {
        pub id: String,
        pub is_draft: bool,
    }
}

pub mod set_pr_not_draft {
    use serde::{Deserialize, Serialize};

    pub fn query() -> &'static str {
        r#"
mutation SetPRNotDraft($id: ID!) {
  markPullRequestReadyForReview(input: { pullRequestId: $id }) {
    pullRequest {
      id
      isDraft
    }
  }
}
"#
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Variables {
        pub id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub convert_pull_request_to_draft: ConvertPullRequestToDraft,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConvertPullRequestToDraft {
        pub pull_request: PullRequest,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PullRequest {
        pub id: String,
        pub is_draft: bool,
    }
}
