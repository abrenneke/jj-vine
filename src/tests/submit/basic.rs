use crate::tests::TestRepo;

#[test]
fn test_repo_creation() {
    let repo = TestRepo::new();

    // Verify jj is initialized
    let status = repo.jj(&["status"]);
    assert!(
        status.contains("Working copy"),
        "Should have working copy status"
    );
}

#[test]
fn test_create_change() {
    let repo = TestRepo::new();

    repo.create_change("test.txt", "hello", "Test commit");

    // Verify the file exists
    let content = std::fs::read_to_string(repo.path.join("test.txt")).unwrap();
    assert_eq!(content, "hello");

    // Verify the description is set
    let log = repo.jj(&["log", "-T", "description"]);
    assert!(
        log.contains("Test commit"),
        "Should have commit message: {}",
        log
    );
}

#[test]
fn test_create_bookmark() {
    let repo = TestRepo::new();

    repo.create_change("test.txt", "hello", "Test commit")
        .create_bookmark("test-branch");

    // Verify bookmark exists
    let bookmarks = repo.jj(&["bookmark", "list"]);
    assert!(
        bookmarks.contains("test-branch"),
        "Should have bookmark: {}",
        bookmarks
    );
}
