use crate::bookmark::BookmarkGraph;
use crate::error::Result;
use crate::jj::Jujutsu;

/// Result of analyzing a bookmark for submission
#[derive(Debug, Clone)]
pub struct SubmissionAnalysis {
    /// The target bookmark that was requested
    pub target_bookmark: String,

    /// All bookmarks that need to be submitted (downstack, inclusive)
    pub bookmarks_to_submit: Vec<String>,

    /// The base branch for the stack (e.g., "main")
    pub base_branch: String,
}

/// Analyze a bookmark and determine what needs to be submitted
///
/// Phase 1 of the three-phase submission process:
/// - Build bookmark graph
/// - Find the stack containing the target bookmark
/// - Determine the downstack that needs to be submitted
pub async fn analyze(jj: &Jujutsu, target_bookmark: &str) -> Result<SubmissionAnalysis> {
    // Build the bookmark graph
    let graph = BookmarkGraph::build(jj).await?;

    // Get the downstack (all bookmarks from root to target, inclusive)
    let bookmarks_to_submit = graph.get_downstack(target_bookmark)?;

    // Find the stack and get its base branch
    let stack = graph
        .find_stack_for_bookmark(target_bookmark)
        .ok_or_else(|| crate::error::Error::BookmarkNotFound {
            name: target_bookmark.to_string(),
        })?;

    Ok(SubmissionAnalysis {
        target_bookmark: target_bookmark.to_string(),
        bookmarks_to_submit,
        base_branch: stack.base.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submission_analysis_struct() {
        let analysis = SubmissionAnalysis {
            target_bookmark: "feature-b".to_string(),
            bookmarks_to_submit: vec!["feature-a".to_string(), "feature-b".to_string()],
            base_branch: "main".to_string(),
        };

        assert_eq!(analysis.target_bookmark, "feature-b");
        assert_eq!(analysis.bookmarks_to_submit.len(), 2);
        assert_eq!(analysis.base_branch, "main");
    }

    // Note: Testing the analyze function requires setting up a temporary jj repo
    // with bookmarks, which is integration testing territory
}
