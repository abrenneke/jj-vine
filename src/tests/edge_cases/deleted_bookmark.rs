use crate::{bookmark::BookmarkGraph, tests::TestRepo};

/// Test that deleting a middle bookmark in a stack correctly updates ancestry
///
/// When you have stack [a, b, c] and delete bookmark b, c's parent should
/// become a (not main).
#[tokio::test]
async fn test_deleted_middle_bookmark() {
    let repo = TestRepo::new();

    // Create stack: a -> b -> c
    repo.commit_with_bookmark("file1.txt", "content1", "Commit A", "bookmark-a")
        .commit_with_bookmark("file2.txt", "content2", "Commit B", "bookmark-b")
        .commit_with_bookmark("file3.txt", "content3", "Commit C", "bookmark-c");

    // Delete middle bookmark
    repo.jj(["bookmark", "delete", "bookmark-b"]);

    // Build the bookmark graph
    let jj = repo.jujutsu();
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph = BookmarkGraph::build(&jj, "main", bookmarks)
        .await
        .expect("Failed to build graph");

    // Verify that bookmark-c's parent is bookmark-a
    let parent = graph.get_parent("bookmark-c");
    assert_eq!(
        parent,
        Some(&"bookmark-a".to_string()),
        "bookmark-c should have bookmark-a as parent after bookmark-b is deleted"
    );

    // Verify the stack structure
    let stack = graph
        .find_stack_for_bookmark("bookmark-c")
        .expect("bookmark-c should be in a stack");

    assert_eq!(
        stack.bookmarks,
        vec!["bookmark-a", "bookmark-c"],
        "Stack should contain bookmark-a and bookmark-c in order"
    );
}
