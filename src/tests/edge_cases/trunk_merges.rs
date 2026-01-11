use crate::{bookmark::BookmarkGraph, jj::Jujutsu, tests::TestRepo};

/// Test that merge commits in trunk history don't fail validation for linear bookmarks
///
/// When a repository has old merge commits in its trunk/main history, creating
/// a linear bookmark on top should be valid. The validation should only check
/// NEW commits, not the entire trunk history.
///
/// BUG: This test currently fails because validation checks ALL ancestors,
/// including trunk history. The fix would be to only validate commits between
/// the bookmark and its base, not the entire ancestry chain.
#[tokio::test]
#[ignore = "Known bug: validation traverses trunk history including old merges"]
async fn test_linear_bookmark_on_trunk_with_merge_history() {
    let repo = TestRepo::new();

    // Create two branches that will be merged
    repo.create_change("file1.txt", "content1", "Branch A commit");
    let branch_a_output = repo.jj(["log", "-r", "@", "--no-graph", "-T", "commit_id"]);
    let branch_a_id = branch_a_output.trim();

    repo.jj(["new", "root()"]);
    repo.create_change("file2.txt", "content2", "Branch B commit");
    let branch_b_output = repo.jj(["log", "-r", "@", "--no-graph", "-T", "commit_id"]);
    let branch_b_id = branch_b_output.trim();

    // Create merge commit (will be in trunk history)
    repo.jj(["new", branch_a_id, branch_b_id]);
    repo.create_change("merged.txt", "merged content", "Merge commit in trunk");

    // Add more commits on top
    repo.jj(["new"]);
    repo.create_change("post-merge.txt", "content", "Post-merge commit");

    // Create trunk bookmark pointing to current commit
    repo.create_bookmark("trunk");

    // Verify trunk has a merge in its history
    let merges = repo.jj(["log", "-r", "::trunk & merges()", "--no-graph", "-T", "description"]);
    assert!(
        merges.contains("Merge commit in trunk"),
        "Trunk should have merge commit in history"
    );

    // Create a linear bookmark on top of trunk
    repo.jj(["new", "trunk"]);
    repo.create_change("feature.txt", "feature", "Feature commit");
    repo.jj(["new"]);
    repo.create_bookmark("my-feature");

    // Verify the new commits are linear (no merges)
    let new_merges = repo.jj(["log", "-r", "::my-feature ~ ::trunk & merges()", "--no-graph"]);
    assert!(
        new_merges.trim().is_empty(),
        "New commits should not contain merges"
    );

    // Build bookmark graph and validate
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu");
    let bookmarks = jj
        .get_bookmarks_with_revset("my-feature & bookmarks()")
        .expect("Failed to get bookmarks");

    let graph = BookmarkGraph::build(&jj, "trunk", bookmarks)
        .await
        .expect("Failed to build graph");

    // Validation should PASS because merge is in trunk, not in new commits
    let result = graph.validate_bookmarks(&jj, &["my-feature".to_string()]);

    assert!(
        result.is_ok(),
        "Validation should pass for linear bookmark on trunk with merge history: {:?}",
        result.err()
    );
}
