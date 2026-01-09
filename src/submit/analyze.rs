use crate::bookmark::BookmarkGraph;
use crate::config::Config;
use crate::error::Result;
use crate::jj::Jujutsu;

/// Result of analyzing bookmarks for submission
#[derive(Debug, Clone)]
pub struct SubmissionAnalysis {
    /// All bookmarks that need to be submitted (union of all downstacks, deduplicated)
    pub bookmarks_to_submit: Vec<String>,

    /// The base branch for the stack (e.g., "main")
    pub base_branch: String,
}

/// Analyze multiple bookmarks and determine what needs to be submitted
///
/// Phase 1 of the three-phase submission process:
/// - Build bookmark graph for all target bookmarks
/// - Determine the union of all downstacks
/// - Validate no merge commits exist
pub async fn analyze(
    jj: &Jujutsu,
    config: &Config,
    target_bookmarks: &[String],
) -> Result<SubmissionAnalysis> {
    if target_bookmarks.is_empty() {
        return Err(crate::error::Error::Other {
            message: "No bookmarks provided for analysis".to_string(),
        });
    }

    let bookmark_revsets: Vec<String> = target_bookmarks
        .iter()
        .map(|b| format!("::{}", b))
        .collect();
    let revset = format!("({}) & mine() & bookmarks()", bookmark_revsets.join(" | "));
    let relevant_bookmarks = jj.get_bookmarks_with_revset(&revset)?;

    // Build the bookmark graph
    let graph = BookmarkGraph::build(jj, &config.default_branch, relevant_bookmarks).await?;

    // Get the downstack for each target bookmark and collect them
    let mut all_downstack_bookmarks = Vec::new();
    for target_bookmark in target_bookmarks {
        let downstack = graph.get_downstack(target_bookmark)?;
        all_downstack_bookmarks.extend(downstack);
    }

    // Deduplicate while maintaining order
    let mut seen = std::collections::HashSet::new();
    let mut bookmarks_to_submit = Vec::new();
    for bookmark in all_downstack_bookmarks {
        if !seen.contains(&bookmark) {
            seen.insert(bookmark.clone());
            bookmarks_to_submit.push(bookmark);
        }
    }

    // Validate the bookmarks have no merge commits
    graph.validate_bookmarks(jj, &bookmarks_to_submit)?;

    // Find the stack for the first bookmark to get the base branch
    // All bookmarks should share the same base branch since they're tracked
    let first_bookmark = &target_bookmarks[0];
    let stack = graph
        .find_stack_for_bookmark(first_bookmark)
        .ok_or_else(|| crate::error::Error::BookmarkNotFound {
            name: first_bookmark.to_string(),
        })?;

    // Filter out the base branch from bookmarks to submit
    // We need the base for MR targeting, but we should never push it
    let bookmarks_to_submit: Vec<String> = bookmarks_to_submit
        .into_iter()
        .filter(|bookmark| bookmark != &stack.base)
        .collect();

    // Validate we have at least one bookmark to submit
    if bookmarks_to_submit.is_empty() {
        return Err(crate::error::Error::Other {
            message: "No feature bookmarks to submit (all appear to be base branches)".to_string(),
        });
    }

    Ok(SubmissionAnalysis {
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
            bookmarks_to_submit: vec!["feature-a".to_string(), "feature-b".to_string()],
            base_branch: "main".to_string(),
        };

        assert_eq!(analysis.bookmarks_to_submit.len(), 2);
        assert_eq!(analysis.base_branch, "main");
    }

    // Note: Testing the analyze function requires setting up a temporary jj repo
    // with bookmarks, which is integration testing territory
}
