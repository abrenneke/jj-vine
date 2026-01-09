/// Integration tests for deferred MR description updates
///
/// These tests verify that earlier MR descriptions are automatically updated
/// with links to later MRs after all MRs are created.
mod test_helpers;

use test_helpers::{GitLabConfig, GitLabTestHelper, TestRepo, unique_test_branch};

#[tokio::test]
async fn test_deferred_updates_linear_stack() {
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

    // Fetch from GitLab origin to get main branch
    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");

    // Track main@origin
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("deferred-a");
    let branch_b = unique_test_branch("deferred-b");
    let branch_c = unique_test_branch("deferred-c");

    // Create bookmark A
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");

    // Create bookmark B on top of A
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");

    // Create bookmark C on top of B
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_c.txt", "Content C")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_c)
        .expect("Failed to create bookmark C");

    // Submit all bookmarks at once using jj mr (submitting the tip will submit the whole stack)
    let output = repo
        .jj_mrs(&["submit", &branch_c])
        .expect("Failed to submit bookmarks");

    println!("=== SUBMISSION OUTPUT ===\n{}\n=== END OUTPUT ===", output);

    // Verify submission succeeded
    assert!(
        output.contains("Successfully submitted") || output.contains("✓ Successfully submitted"),
        "Expected successful submission, got: {}",
        output
    );

    // Query the MRs that were created
    let mr_a = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to query MR A")
        .expect("MR A should exist");

    let mr_b = gitlab
        .client
        .find_mr_by_source_branch(&branch_b)
        .await
        .expect("Failed to query MR B")
        .expect("MR B should exist");

    let mr_c = gitlab
        .client
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Failed to query MR C")
        .expect("MR C should exist");

    // Verify A's description links to B and C
    let desc_a = mr_a
        .description
        .as_ref()
        .expect("MR A should have description");
    assert!(
        desc_a.contains(&format!("{} - !{}", branch_b, mr_b.iid)),
        "MR A should link to MR B. Description:\n{}",
        desc_a
    );
    assert!(
        desc_a.contains(&format!("{} - !{}", branch_c, mr_c.iid)),
        "MR A should link to MR C. Description:\n{}",
        desc_a
    );

    // Verify B's description links to C
    let desc_b = mr_b
        .description
        .as_ref()
        .expect("MR B should have description");
    assert!(
        desc_b.contains(&format!("{} - !{}", branch_c, mr_c.iid)),
        "MR B should link to MR C. Description:\n{}",
        desc_b
    );

    // Verify C's description shows A and B
    let desc_c = mr_c
        .description
        .as_ref()
        .expect("MR C should have description");
    assert!(
        desc_c.contains(&format!("{} - !{}", branch_a, mr_a.iid)),
        "MR C should show MR A. Description:\n{}",
        desc_c
    );
    assert!(
        desc_c.contains(&format!("{} - !{}", branch_b, mr_b.iid)),
        "MR C should show MR B. Description:\n{}",
        desc_c
    );
}

#[tokio::test]
async fn test_deferred_updates_with_existing_mr() {
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

    // Fetch from GitLab origin to get main branch
    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");

    // Track main@origin
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("existing-a");
    let branch_b = unique_test_branch("existing-b");
    let branch_c = unique_test_branch("existing-c");

    // Create and submit bookmark A first
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");

    repo.jj_mrs(&["submit", &branch_a])
        .expect("Failed to submit bookmark A");

    // Get MR A
    let mr_a_initial = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to query MR A")
        .expect("MR A should exist");

    // Verify A's initial description doesn't have B or C
    // (Single bookmarks don't get stack descriptions, so it may be empty/None)
    if let Some(desc_a_initial) = &mr_a_initial.description {
        assert!(
            !desc_a_initial.contains(&branch_b),
            "MR A should not mention B yet"
        );
        assert!(
            !desc_a_initial.contains(&branch_c),
            "MR A should not mention C yet"
        );
    }

    // Create bookmarks B and C
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_c.txt", "Content C")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_c)
        .expect("Failed to create bookmark C");

    // Track bookmarks before pushing
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_b)])
        .expect("Failed to track bookmark B");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_c)])
        .expect("Failed to track bookmark C");

    // Push B and C
    repo.jj(&["git", "push", "--bookmark", &branch_b])
        .expect("Failed to push bookmark B");
    repo.jj(&["git", "push", "--bookmark", &branch_c])
        .expect("Failed to push bookmark C");

    // Submit B and C together
    repo.jj_mrs(&["submit", "--tracked"])
        .expect("Failed to submit bookmarks B and C");

    // Query updated MRs
    let mr_a_updated = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to query updated MR A")
        .expect("MR A should exist");

    let mr_b = gitlab
        .client
        .find_mr_by_source_branch(&branch_b)
        .await
        .expect("Failed to query MR B")
        .expect("MR B should exist");

    let mr_c = gitlab
        .client
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Failed to query MR C")
        .expect("MR C should exist");

    // Verify A's description was updated to include B and C
    let desc_a_updated = mr_a_updated
        .description
        .as_ref()
        .expect("MR A should have description");
    assert!(
        desc_a_updated.contains(&format!("{} - !{}", branch_b, mr_b.iid)),
        "Updated MR A should link to MR B. Description:\n{}",
        desc_a_updated
    );
    assert!(
        desc_a_updated.contains(&format!("{} - !{}", branch_c, mr_c.iid)),
        "Updated MR A should link to MR C. Description:\n{}",
        desc_a_updated
    );
}

#[tokio::test]
async fn test_deferred_updates_multiple_stacks() {
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

    // Fetch from GitLab origin to get main branch
    repo.jj(&["git", "fetch"])
        .expect("Failed to fetch from origin");

    // Track main@origin
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main branch");

    let branch_a = unique_test_branch("multi-a");
    let branch_b = unique_test_branch("multi-b");
    let branch_c = unique_test_branch("multi-c");
    let branch_d = unique_test_branch("multi-d");

    // Create bookmark A
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");

    // Create bookmark B on top of A
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");

    // Create bookmark C on top of B
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file_c.txt", "Content C")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_c)
        .expect("Failed to create bookmark C");

    // Go back to B and create branch D (creating a split: A→B→C and A→B→D)
    repo.jj(&["new", &branch_b])
        .expect("Failed to create new change on B");
    repo.create_file("file_d.txt", "Content D")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit D"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_d)
        .expect("Failed to create bookmark D");

    // Track bookmarks before pushing
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_a)])
        .expect("Failed to track bookmark A");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_b)])
        .expect("Failed to track bookmark B");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_c)])
        .expect("Failed to track bookmark C");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_d)])
        .expect("Failed to track bookmark D");

    // Push all bookmarks to remote
    repo.jj(&["git", "push", "--bookmark", &branch_a])
        .expect("Failed to push bookmark A");
    repo.jj(&["git", "push", "--bookmark", &branch_b])
        .expect("Failed to push bookmark B");
    repo.jj(&["git", "push", "--bookmark", &branch_c])
        .expect("Failed to push bookmark C");
    repo.jj(&["git", "push", "--bookmark", &branch_d])
        .expect("Failed to push bookmark D");

    // Submit all tracked bookmarks at once
    let output = repo
        .jj_mrs(&["submit", "--tracked"])
        .expect("Failed to submit bookmarks");

    // Verify submission succeeded
    assert!(
        output.contains("Successfully submitted"),
        "Expected successful submission, got: {}",
        output
    );

    // Query the MRs
    let mr_a = gitlab
        .client
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to query MR A")
        .expect("MR A should exist");

    let mr_b = gitlab
        .client
        .find_mr_by_source_branch(&branch_b)
        .await
        .expect("Failed to query MR B")
        .expect("MR B should exist");

    let mr_c = gitlab
        .client
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Failed to query MR C")
        .expect("MR C should exist");

    let mr_d = gitlab
        .client
        .find_mr_by_source_branch(&branch_d)
        .await
        .expect("Failed to query MR D")
        .expect("MR D should exist");

    // Verify A's description shows both stacks
    let desc_a = mr_a
        .description
        .as_ref()
        .expect("MR A should have description");
    assert!(
        desc_a.contains("This MR is part of 2 stacks"),
        "MR A should indicate 2 stacks. Description:\n{}",
        desc_a
    );
    assert!(
        desc_a.contains(&format!("{} - !{}", branch_c, mr_c.iid)),
        "MR A should link to MR C. Description:\n{}",
        desc_a
    );
    assert!(
        desc_a.contains(&format!("{} - !{}", branch_d, mr_d.iid)),
        "MR A should link to MR D. Description:\n{}",
        desc_a
    );

    // Verify B's description shows both stacks
    let desc_b = mr_b
        .description
        .as_ref()
        .expect("MR B should have description");
    assert!(
        desc_b.contains("This MR is part of 2 stacks"),
        "MR B should indicate 2 stacks. Description:\n{}",
        desc_b
    );
    assert!(
        desc_b.contains(&format!("{} - !{}", branch_c, mr_c.iid)),
        "MR B should link to MR C. Description:\n{}",
        desc_b
    );
    assert!(
        desc_b.contains(&format!("{} - !{}", branch_d, mr_d.iid)),
        "MR B should link to MR D. Description:\n{}",
        desc_b
    );

    // Verify C's description shows only its stack (A→B→C)
    let desc_c = mr_c
        .description
        .as_ref()
        .expect("MR C should have description");
    assert!(
        !desc_c.contains("This MR is part of 2 stacks"),
        "MR C should not indicate multiple stacks. Description:\n{}",
        desc_c
    );
    assert!(
        desc_c.contains(&format!("{} - !{}", branch_a, mr_a.iid)),
        "MR C should show MR A. Description:\n{}",
        desc_c
    );
    assert!(
        desc_c.contains(&format!("{} - !{}", branch_b, mr_b.iid)),
        "MR C should show MR B. Description:\n{}",
        desc_c
    );
    assert!(
        !desc_c.contains(&branch_d),
        "MR C should not mention branch D. Description:\n{}",
        desc_c
    );

    // Verify D's description shows only its stack (A→B→D)
    let desc_d = mr_d
        .description
        .as_ref()
        .expect("MR D should have description");
    assert!(
        !desc_d.contains("This MR is part of 2 stacks"),
        "MR D should not indicate multiple stacks. Description:\n{}",
        desc_d
    );
    assert!(
        desc_d.contains(&format!("{} - !{}", branch_a, mr_a.iid)),
        "MR D should show MR A. Description:\n{}",
        desc_d
    );
    assert!(
        desc_d.contains(&format!("{} - !{}", branch_b, mr_b.iid)),
        "MR D should show MR B. Description:\n{}",
        desc_d
    );
    assert!(
        !desc_d.contains(&branch_c),
        "MR D should not mention branch C. Description:\n{}",
        desc_d
    );
}
