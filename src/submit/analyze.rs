use crate::bookmark::BookmarkGraph;
use crate::config::Config;
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
/// - Validate no merge commits exist in the stack
pub async fn analyze(
    jj: &Jujutsu,
    config: &Config,
    target_bookmark: &str,
) -> Result<SubmissionAnalysis> {
    // Get only the target bookmark and its ancestors
    let revset = format!("::{}  & mine() & bookmarks()", target_bookmark);
    let relevant_bookmarks = jj.get_bookmarks_with_revset(&revset)?;

    // Build the bookmark graph
    let graph = BookmarkGraph::build(jj, &config.default_branch, relevant_bookmarks).await?;

    // Get the downstack (all bookmarks from root to target, inclusive)
    let downstack = graph.get_downstack(target_bookmark)?;

    // Validate the downstack has no merge commits
    graph.validate_bookmarks(jj, &downstack)?;

    // Find the stack and get its base branch
    let stack = graph
        .find_stack_for_bookmark(target_bookmark)
        .ok_or_else(|| crate::error::Error::BookmarkNotFound {
            name: target_bookmark.to_string(),
        })?;

    // Filter out the base branch from bookmarks to submit
    // We need the base for MR targeting, but we should never push it
    let bookmarks_to_submit: Vec<String> = downstack
        .into_iter()
        .filter(|bookmark| bookmark != &stack.base)
        .collect();

    // Validate we have at least one bookmark to submit
    if bookmarks_to_submit.is_empty() {
        return Err(crate::error::Error::Other {
            message: format!(
                "Cannot submit '{}': it appears to be the base branch. Only feature bookmarks can be submitted.",
                target_bookmark
            ),
        });
    }

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
