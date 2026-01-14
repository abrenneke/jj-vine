use crate::{
    commands::submit::SubmitCommandConfig,
    forge::Forge,
    tests::{TestRepo, unique_branch},
};

/// Test that PR descriptions include stack information
#[tokio::test]
async fn test_pr_description_includes_stack_info() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("desc-a");
    let branch_b = unique_branch("desc-b");

    // Create stack: main -> A -> B
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    // Submit the stack
    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Verify PR A has stack markers
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let desc_a = pr_a.description();
    assert!(
        desc_a.contains("<!-- start jj-vine stack -->"),
        "PR A should have stack markers. Description:\n{}",
        desc_a
    );
    assert!(
        desc_a.contains("<!-- end jj-vine stack -->"),
        "PR A should have end marker. Description:\n{}",
        desc_a
    );
}

/// Test that PR A's description links to PR B when they're in a stack
#[tokio::test]
async fn test_pr_description_links_to_dependent_prs() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("link-a");
    let branch_b = unique_branch("link-b");

    // Create stack: main -> A -> B
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    // Submit the stack
    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Get both PRs
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let pr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await
        .expect("Query failed")
        .expect("PR B should exist");

    // PR A's description should link to PR B
    let desc_a = pr_a.description();
    assert!(
        desc_a.contains(&format!("#{}", pr_b.iid())),
        "PR A should link to PR B (#{}). Description:\n{}",
        pr_b.iid(),
        desc_a
    );
}

/// Test that user content in PR description is preserved on resubmit
#[tokio::test]
async fn test_user_content_preserved_on_resubmit() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("preserve-a");
    let branch_b = unique_branch("preserve-b");

    // Create and submit branch A
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.submit(SubmitCommandConfig {
        revset: Some(branch_a.clone()),
        ..Default::default()
    })
    .await;

    // Add custom user content to PR A's description
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let user_content = "My important notes about this PR";
    let new_desc = format!("{}\n\n{}", pr_a.description(), user_content);
    repo.forge()
        .update_merge_request_description(pr_a.iid().as_ref(), &new_desc)
        .await
        .expect("Failed to update description");

    // Create branch B and resubmit
    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Verify user content is still present
    let pr_a_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let desc = pr_a_updated.description();
    assert!(
        desc.contains(user_content),
        "User content should be preserved. Description:\n{}",
        desc
    );
}

/// Test that markers are added to description that doesn't have them
#[tokio::test]
async fn test_add_markers_to_description_without_markers() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("markers-a");
    let branch_b = unique_branch("markers-b");

    // Create and submit branch A
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.submit(SubmitCommandConfig {
        revset: Some(branch_a.clone()),
        ..Default::default()
    })
    .await;

    // Set description WITHOUT markers
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let user_description = "Custom description without markers";
    repo.forge()
        .update_merge_request_description(pr_a.iid().as_ref(), user_description)
        .await
        .expect("Failed to update description");

    // Create branch B and submit stack
    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Verify markers were added
    let pr_a_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let desc = pr_a_updated.description();

    assert!(
        desc.contains("<!-- start jj-vine stack -->"),
        "Should have start marker. Description:\n{}",
        desc
    );
    assert!(
        desc.contains("<!-- end jj-vine stack -->"),
        "Should have end marker. Description:\n{}",
        desc
    );
    assert!(
        desc.contains(user_description),
        "Should preserve user content. Description:\n{}",
        desc
    );
}

/// Test that resubmitting unchanged stack skips description update
#[tokio::test]
async fn test_skip_update_when_description_unchanged() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("unchanged-a");
    let branch_b = unique_branch("unchanged-b");

    // Create and submit stack
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Get initial description
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");
    let initial_desc = pr_a.description();

    // Resubmit (no changes)
    repo.submit(SubmitCommandConfig {
        revset: Some(branch_b.clone()),
        ..Default::default()
    })
    .await;

    // Description should be unchanged
    let pr_a_after = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    assert_eq!(
        initial_desc,
        pr_a_after.description(),
        "Description should be unchanged after resubmit"
    );
}

/// Test deferred updates with multiple stacks (diamond structure)
#[tokio::test]
async fn test_deferred_updates_multiple_stacks() {
    let repo = TestRepo::with_github_remote();

    let branch_a = unique_branch("diamond-a");
    let branch_b = unique_branch("diamond-b");
    let branch_c = unique_branch("diamond-c");
    let branch_d = unique_branch("diamond-d");

    // Create structure: A → B → C and A → B → D (diamond)
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj(["new"]);
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    // Go back to B and create D
    repo.jj(["new", &branch_b]);
    repo.create_change("d.txt", "d", "Commit D")
        .create_and_push_bookmark(&branch_d);

    // Submit all tracked bookmarks
    repo.submit(SubmitCommandConfig {
        tracked: true,
        ..Default::default()
    })
    .await;

    // Get all PRs
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Query failed")
        .expect("PR A should exist");

    let pr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await
        .expect("Query failed")
        .expect("PR B should exist");

    let pr_c = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await
        .expect("Query failed")
        .expect("PR C should exist");

    let pr_d = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_d)
        .await
        .expect("Query failed")
        .expect("PR D should exist");

    // A and B should indicate they're part of 2 stacks
    let desc_a = pr_a.description();
    assert!(
        desc_a.contains("2 stacks"),
        "PR A should indicate 2 stacks. Description:\n{}",
        desc_a
    );

    let desc_b = pr_b.description();
    assert!(
        desc_b.contains("2 stacks"),
        "PR B should indicate 2 stacks. Description:\n{}",
        desc_b
    );

    // C should only show its stack (A→B→C)
    let desc_c = pr_c.description();
    assert!(
        !desc_c.contains("2 stacks"),
        "PR C should not indicate multiple stacks. Description:\n{}",
        desc_c
    );
    assert!(
        desc_c.contains(&format!("#{}", pr_a.iid())),
        "PR C should link to PR A. Description:\n{}",
        desc_c
    );

    // D should only show its stack (A→B→D)
    let desc_d = pr_d.description();
    assert!(
        !desc_d.contains("2 stacks"),
        "PR D should not indicate multiple stacks. Description:\n{}",
        desc_d
    );
    assert!(
        desc_d.contains(&format!("#{}", pr_a.iid())),
        "PR D should link to PR A. Description:\n{}",
        desc_d
    );
}
