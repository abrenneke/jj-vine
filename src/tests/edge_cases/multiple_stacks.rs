use crate::tests::{TestRepo, unique_branch};

/// Test that multiple independent stacks don't incorrectly retarget each
/// other's MRs
#[tokio::test]
async fn test_multiple_independent_stacks_dont_incorrectly_retarget() {
    let repo = TestRepo::with_gitlab_remote();

    // Create two independent stacks:
    // Stack 1: main -> stack1-a -> stack1-b
    // Stack 2: main -> stack2-a

    // Stack 1, bookmark A
    repo.jj(["new", "main"]);
    let branch_1a = unique_branch("stack1-a");
    repo.create_change("file1.txt", "stack1-a content", "Stack 1 A")
        .create_bookmark(&branch_1a);
    repo.jj(["git", "push", "--bookmark", &branch_1a]);

    // Stack 1, bookmark B
    repo.jj(["new"]);
    let branch_1b = unique_branch("stack1-b");
    repo.create_change("file2.txt", "stack1-b content", "Stack 1 B")
        .create_bookmark(&branch_1b);
    repo.jj(["git", "push", "--bookmark", &branch_1b]);

    // Stack 2, bookmark A (independent from stack 1)
    repo.jj(["new", "main"]);
    let branch_2a = unique_branch("stack2-a");
    repo.create_change("file3.txt", "stack2-a content", "Stack 2 A")
        .create_bookmark(&branch_2a);
    repo.jj(["git", "push", "--bookmark", &branch_2a]);

    // Dry run submission to see what the tool wants to do
    let output = repo
        .submit(crate::commands::submit::SubmitCommandConfig {
            tracked: true,
            dry_run: true,
            ..Default::default()
        })
        .await;

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
