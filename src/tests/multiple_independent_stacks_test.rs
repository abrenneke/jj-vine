/// Test that multiple independent stacks don't incorrectly retarget each
/// other's MRs
use crate::tests::{GitLabConfig, GitLabTestHelper, TestRepo};

#[tokio::test]
async fn test_multiple_independent_stacks_dont_incorrectly_retarget() {
    GitLabTestHelper::from_env()
        .await
        .expect("Failed to create GitLab test helper");

    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => return,
    };

    let repo = TestRepo::with_gitlab_remote(&config)
        .expect("Failed to create test repository with GitLab remote");

    // Fetch from GitLab origin to get main branch
    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");

    // Track main@origin
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    // Create two independent stacks:
    // Stack 1: main -> stack1-a -> stack1-b
    // Stack 2: main -> stack2-a

    // Stack 1, bookmark A
    repo.jj(&["new", "main"]).expect("Failed to new from main");
    repo.create_file("file1.txt", "stack1-a content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Stack 1 A"])
        .expect("Failed to describe");
    let branch_1a = crate::tests::unique_test_branch("stack1-a");
    repo.jj(&["bookmark", "create", &branch_1a])
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_1a)])
        .expect("Failed to track stack1-a");
    repo.jj(&["git", "push", "--bookmark", &branch_1a])
        .expect("Failed to push stack1-a");

    // Stack 1, bookmark B
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file2.txt", "stack1-b content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Stack 1 B"])
        .expect("Failed to describe");
    let branch_1b = crate::tests::unique_test_branch("stack1-b");
    repo.jj(&["bookmark", "create", &branch_1b])
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_1b)])
        .expect("Failed to track stack1-b");
    repo.jj(&["git", "push", "--bookmark", &branch_1b])
        .expect("Failed to push stack1-b");

    // Stack 2, bookmark A (independent from stack 1)
    repo.jj(&["new", "main"])
        .expect("Failed to create new change from main");
    repo.create_file("file3.txt", "stack2-a content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Stack 2 A"])
        .expect("Failed to describe");
    let branch_2a = crate::tests::unique_test_branch("stack2-a");
    repo.jj(&["bookmark", "create", &branch_2a])
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_2a)])
        .expect("Failed to track stack2-a");
    repo.jj(&["git", "push", "--bookmark", &branch_2a])
        .expect("Failed to push stack2-a");

    // Dry run submission to see what the tool wants to do
    let output = strip_ansi_escapes::strip_str(
        repo.jj_mrs(&["submit", "--tracked", "--dry-run", "-v"])
            .expect("Failed to run jj mr submit"),
    );

    // Verify that stack2-a targets main, not stack1-b
    assert!(
        output.contains(&format!("Would create {} -> main", branch_1a)),
        "{} should target main, output:\n{}",
        branch_1a,
        output
    );
    assert!(
        output.contains(&format!("Would create {} -> {}", branch_1b, branch_1a)),
        "{} should target {}, output:\n{}",
        branch_1b,
        branch_1a,
        output
    );
    assert!(
        output.contains(&format!("Would create {} -> main", branch_2a)),
        "{} should target main (not {}!), output:\n{}",
        branch_2a,
        branch_1b,
        output
    );
}
