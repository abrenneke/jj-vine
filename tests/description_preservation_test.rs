/// Integration tests for user description preservation
///
/// These tests verify that user-edited content in MR descriptions
/// is preserved when stack information is updated.
mod test_helpers;

use test_helpers::{GitLabConfig, GitLabTestHelper, TestRepo, unique_test_branch};

#[tokio::test]
async fn test_preserve_user_content_on_update() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => return,
    };

    let repo = TestRepo::with_gitlab_remote(&config)
        .expect("Failed to create test repository with GitLab remote");

    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("preserve-a");
    let branch_b = unique_test_branch("preserve-b");

    // Create and submit branch A
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_a])
        .expect("Failed to submit A");

    // Manually edit MR description via API to add user content
    let mr_a = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to find MR A")
        .expect("MR A should exist");

    let custom_description = format!(
        "<!-- start jj-mrs stack -->\nThis MR is part of a stack of 1 MRs:\n\n1. **{} ← this MR**\n<!-- end jj-mrs stack -->\n\nMy custom notes about this MR\n\n## Implementation Details\n\nThis is important context.",
        branch_a
    );

    gitlab
        .client
        .update_mr_description(mr_a.iid, &custom_description)
        .await
        .expect("Failed to update description manually");

    // Create branch B stacked on A
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark");

    // Submit the stack - this should update A's description
    repo.jj_mrs(&["submit", &branch_b])
        .expect("Failed to submit stack");

    // Verify A's description was updated with B but preserved user content
    let mr_a_updated = gitlab
        .client
        .get_merge_request(mr_a.iid)
        .await
        .expect("Failed to get updated MR A");

    let desc = mr_a_updated
        .description
        .as_ref()
        .expect("MR A should have description");

    // Should contain updated stack info (now mentions branch B)
    assert!(
        desc.contains(&format!("{} - !", branch_b)),
        "Should contain link to branch B. Description:\n{}",
        desc
    );

    // Should preserve user content
    assert!(
        desc.contains("My custom notes about this MR"),
        "Should preserve user notes. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("## Implementation Details"),
        "Should preserve user section headers. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("This is important context."),
        "Should preserve user content. Description:\n{}",
        desc
    );
}

#[tokio::test]
async fn test_add_markers_to_description_without_markers() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => return,
    };

    let repo = TestRepo::with_gitlab_remote(&config)
        .expect("Failed to create test repository with GitLab remote");

    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("no-markers-a");
    let branch_b = unique_test_branch("no-markers-b");

    // Create and submit branch A
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_a])
        .expect("Failed to submit A");

    // Manually set description WITHOUT markers
    let mr_a = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to find MR A")
        .expect("MR A should exist");

    let user_description = "This is my custom description\n\nWith important details.";
    gitlab
        .client
        .update_mr_description(mr_a.iid, user_description)
        .await
        .expect("Failed to update description");

    // Create branch B and submit stack
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_b])
        .expect("Failed to submit stack");

    // Verify A's description now has markers at the beginning with user content after
    let mr_a_updated = gitlab
        .client
        .get_merge_request(mr_a.iid)
        .await
        .expect("Failed to get updated MR A");

    let desc = mr_a_updated
        .description
        .as_ref()
        .expect("MR A should have description");

    // Should have markers
    assert!(
        desc.contains("<!-- start jj-mrs stack -->"),
        "Should have start marker. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("<!-- end jj-mrs stack -->"),
        "Should have end marker. Description:\n{}",
        desc
    );

    // Should preserve original user content after markers
    assert!(
        desc.contains("This is my custom description"),
        "Should preserve user content. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("With important details."),
        "Should preserve user content. Description:\n{}",
        desc
    );
}

#[tokio::test]
async fn test_skip_update_when_description_unchanged() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => return,
    };

    let repo = TestRepo::with_gitlab_remote(&config)
        .expect("Failed to create test repository with GitLab remote");

    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("unchanged-a");
    let branch_b = unique_test_branch("unchanged-b");

    // Create stack and submit
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_b])
        .expect("Failed to submit stack");

    // Get initial description
    let mr_a = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to find MR A")
        .expect("MR A should exist");
    let initial_desc = mr_a.description.clone();

    // Submit again (no changes to stack structure)
    let output = repo
        .jj_mrs(&["submit", &branch_b])
        .expect("Failed to re-submit stack");

    // Output should indicate description was unchanged
    assert!(
        output.contains("Skipping MR") && output.contains("unchanged")
            || output.contains("description (unchanged)"),
        "Should skip unchanged description update. Output:\n{}",
        output
    );

    // Get description after re-submit
    let mr_a_after = gitlab
        .client
        .get_merge_request(mr_a.iid)
        .await
        .expect("Failed to get MR A after re-submit");

    // Description should be IDENTICAL (idempotent)
    assert_eq!(
        initial_desc, mr_a_after.description,
        "Re-submitting unchanged stack should not modify description"
    );
}

#[tokio::test]
async fn test_empty_mr_description_gets_stack_info() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => return,
    };

    let repo = TestRepo::with_gitlab_remote(&config)
        .expect("Failed to create test repository with GitLab remote");

    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("empty-a");
    let branch_b = unique_test_branch("empty-b");

    // Create and submit branch A
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_a])
        .expect("Failed to submit A");

    // Create stack
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark");

    repo.jj_mrs(&["submit", &branch_b])
        .expect("Failed to submit stack");

    // Verify A now has stack description
    let mr_a_updated = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to find MR A")
        .expect("MR A should exist");

    let desc = mr_a_updated
        .description
        .as_ref()
        .expect("MR A should have description after stack update");

    assert!(
        desc.contains("<!-- start jj-mrs stack -->"),
        "Should have start marker. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("<!-- end jj-mrs stack -->"),
        "Should have end marker. Description:\n{}",
        desc
    );
    assert!(
        desc.contains(&branch_b),
        "Should mention branch B. Description:\n{}",
        desc
    );
}
