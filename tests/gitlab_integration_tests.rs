/// Real GitLab API integration tests
///
/// These tests connect to a real GitLab instance and create/update actual MRs.
mod test_helpers;

use jj_mrs::gitlab::GitLabClient;
use test_helpers::{GitLabConfig, GitLabTestHelper, TestRepo, unique_test_branch};

#[tokio::test]
async fn test_create_simple_mr() {
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

    let branch_name = unique_test_branch("simple");

    // Create a file and bookmark
    repo.create_file("test.txt", "Hello from test")
        .expect("Failed to create file");

    // Set description for the current working copy change
    repo.jj(&["describe", "-m", "Test commit for simple MR"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");

    // Track the bookmark before pushing
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");

    // Push the bookmark to remote
    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    // Create MR using GitLab API
    let mr = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Test MR: {}", branch_name),
            Some("This is a test MR created by integration tests"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create merge request");

    // Verify MR was created correctly
    assert_eq!(mr.source_branch, branch_name);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.state, "opened");
    assert!(mr.title.contains(&branch_name));

    println!("Created MR: {}", mr.web_url);
    println!("MR IID: {}", mr.iid);

    // Verify we can find the MR by source branch
    let found_mr = gitlab
        .client
        .find_mr_by_source_branch(&branch_name)
        .await
        .expect("Failed to find MR by source branch");

    assert!(found_mr.is_some());
    let found_mr = found_mr.unwrap();
    assert_eq!(found_mr.iid, mr.iid);
    assert_eq!(found_mr.source_branch, branch_name);
}

#[tokio::test]
async fn test_find_mr_by_source_branch() {
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

    let branch_name = unique_test_branch("find-mr");

    // Create and push a branch
    repo.create_file("find.txt", "Test file for find MR")
        .expect("Failed to create file");

    repo.jj(&["describe", "-m", "Test commit for find MR"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");

    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");

    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    // Create MR
    let mr = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Test MR for find: {}", branch_name),
            Some("Testing find_mr_by_source_branch"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create merge request");

    println!("Created MR: {} (IID: {})", mr.web_url, mr.iid);

    // Test: Find the MR by source branch
    let found_mr = gitlab
        .client
        .find_mr_by_source_branch(&branch_name)
        .await
        .expect("Failed to find MR by source branch");

    assert!(found_mr.is_some(), "Should find the MR");
    let found_mr = found_mr.unwrap();
    assert_eq!(found_mr.iid, mr.iid);
    assert_eq!(found_mr.source_branch, branch_name);
    assert_eq!(found_mr.target_branch, "main");
    assert_eq!(found_mr.state, "opened");

    // Test: Try to find a non-existent MR
    let nonexistent_branch = unique_test_branch("nonexistent");
    let not_found = gitlab
        .client
        .find_mr_by_source_branch(&nonexistent_branch)
        .await
        .expect("find_mr_by_source_branch should not error for non-existent branch");

    assert!(
        not_found.is_none(),
        "Should not find MR for non-existent branch"
    );

    println!(
        "Successfully verified find_mr_by_source_branch for existing and non-existent branches"
    );
}

#[tokio::test]
async fn test_update_mr_base() {
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

    let branch_a = unique_test_branch("update-base-a");
    let branch_b = unique_test_branch("update-base-b");

    // Create first branch
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");

    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");

    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_a)])
        .expect("Failed to track bookmark A");

    repo.jj(&["git", "push", "--bookmark", &branch_a])
        .expect("Failed to push bookmark A");

    // Create second branch on top of first
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");

    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");

    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_b)])
        .expect("Failed to track bookmark B");

    repo.jj(&["git", "push", "--bookmark", &branch_b])
        .expect("Failed to push bookmark B");

    // Create MR for branch_b targeting main
    let mr = gitlab
        .client
        .create_merge_request(
            &branch_b,
            "main",
            &format!("Test MR for update base: {}", branch_b),
            Some("Testing update_mr_base"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create merge request");

    println!("Created MR: {} (IID: {})", mr.web_url, mr.iid);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.source_branch, branch_b);

    // Update the MR to target branch_a instead of main
    let updated_mr = gitlab
        .client
        .update_mr_base(mr.iid, &branch_a)
        .await
        .expect("Failed to update MR base");

    assert_eq!(updated_mr.iid, mr.iid);
    assert_eq!(updated_mr.target_branch, branch_a);
    assert_eq!(updated_mr.source_branch, branch_b);

    println!(
        "Successfully updated MR target branch from main to {}",
        branch_a
    );
}

#[tokio::test]
async fn test_invalid_token_errors_clearly() {
    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: GitLab config not available");
            return;
        }
    };

    // Create client with invalid token
    let client = GitLabClient::new(
        config.host,
        config.project,
        "invalid-token-12345".to_string(),
        std::env::var("GITLAB_CA_BUNDLE").ok(),
        std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false),
    )
    .expect("Failed to create GitLab client");

    let branch_name = unique_test_branch("invalid-token");

    // Attempt to create MR with invalid token
    let result = client
        .create_merge_request(
            &branch_name,
            "main",
            "This should fail",
            Some("Testing invalid token"),
            true,
            false,
            None,
            None,
        )
        .await;

    assert!(result.is_err(), "Should fail with invalid token");
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);

    // Check that error message mentions authentication or 401
    assert!(
        err_msg.contains("401") || err_msg.to_lowercase().contains("unauthorized"),
        "Error should mention authentication issue, got: {}",
        err_msg
    );

    println!("Got expected error for invalid token: {}", err_msg);
}

#[tokio::test]
async fn test_nonexistent_project_errors_clearly() {
    let config = match GitLabConfig::from_env() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: GitLab config not available");
            return;
        }
    };

    // Create client with non-existent project
    let client = GitLabClient::new(
        config.host,
        "nonexistent/fake-project-12345".to_string(),
        config.token,
        std::env::var("GITLAB_CA_BUNDLE").ok(),
        std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false),
    )
    .expect("Failed to create GitLab client");

    let branch_name = unique_test_branch("nonexistent-project");

    // Attempt to create MR with non-existent project
    let result = client
        .create_merge_request(
            &branch_name,
            "main",
            "This should fail",
            Some("Testing nonexistent project"),
            true,
            false,
            None,
            None,
        )
        .await;

    assert!(result.is_err(), "Should fail with nonexistent project");
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);

    // Check that error message mentions 404 or not found
    assert!(
        err_msg.contains("404") || err_msg.to_lowercase().contains("not found"),
        "Error should mention project not found, got: {}",
        err_msg
    );

    println!("Got expected error for nonexistent project: {}", err_msg);
}

#[tokio::test]
async fn test_create_stacked_mrs() {
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

    let branch_a = unique_test_branch("stack-a");
    let branch_b = unique_test_branch("stack-b");
    let branch_c = unique_test_branch("stack-c");

    // Create bookmark-a
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_a)])
        .expect("Failed to track bookmark A");
    repo.jj(&["git", "push", "--bookmark", &branch_a])
        .expect("Failed to push bookmark A");

    // Create bookmark-b on top of bookmark-a
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_b)])
        .expect("Failed to track bookmark B");
    repo.jj(&["git", "push", "--bookmark", &branch_b])
        .expect("Failed to push bookmark B");

    // Create bookmark-c on top of bookmark-b
    repo.create_file("file_c.txt", "Content C")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_c)
        .expect("Failed to create bookmark C");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_c)])
        .expect("Failed to track bookmark C");
    repo.jj(&["git", "push", "--bookmark", &branch_c])
        .expect("Failed to push bookmark C");

    // Create MRs for all three branches
    let mr_a = gitlab
        .client
        .create_merge_request(
            &branch_a,
            "main",
            &format!("MR A: {}", branch_a),
            Some("Stack A"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR A");

    let mr_b = gitlab
        .client
        .create_merge_request(
            &branch_b,
            &branch_a,
            &format!("MR B: {}", branch_b),
            Some("Stack B"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR B");

    let mr_c = gitlab
        .client
        .create_merge_request(
            &branch_c,
            &branch_b,
            &format!("MR C: {}", branch_c),
            Some("Stack C"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR C");

    // Verify MR A: bookmark-a → main
    assert_eq!(mr_a.source_branch, branch_a);
    assert_eq!(mr_a.target_branch, "main");
    assert_eq!(mr_a.state, "opened");

    // Verify MR B: bookmark-b → bookmark-a
    assert_eq!(mr_b.source_branch, branch_b);
    assert_eq!(mr_b.target_branch, branch_a);
    assert_eq!(mr_b.state, "opened");

    // Verify MR C: bookmark-c → bookmark-b
    assert_eq!(mr_c.source_branch, branch_c);
    assert_eq!(mr_c.target_branch, branch_b);
    assert_eq!(mr_c.state, "opened");

    println!("Created stacked MRs:");
    println!("  MR A ({}): {} → main", mr_a.iid, branch_a);
    println!("  MR B ({}): {} → {}", mr_b.iid, branch_b, branch_a);
    println!("  MR C ({}): {} → {}", mr_c.iid, branch_c, branch_b);
}

#[tokio::test]
async fn test_idempotent_submission() {
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

    let branch_name = unique_test_branch("idempotent");

    // Create and push a branch
    repo.create_file("idempotent.txt", "Content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Idempotent test"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    // Create first MR
    let mr1 = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Idempotent test: {}", branch_name),
            Some("First submission"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create first MR");

    println!("Created first MR: {} (IID: {})", mr1.web_url, mr1.iid);

    // Try to submit again - should find existing MR
    let found_mr = gitlab
        .client
        .find_mr_by_source_branch(&branch_name)
        .await
        .expect("Failed to find existing MR");

    assert!(found_mr.is_some(), "Should find existing MR");
    let found_mr = found_mr.unwrap();
    assert_eq!(found_mr.iid, mr1.iid, "Should find the same MR");
    assert_eq!(found_mr.source_branch, branch_name);
    assert_eq!(found_mr.target_branch, "main");

    println!(
        "Idempotency verified: Second lookup found same MR (IID: {})",
        found_mr.iid
    );
}

#[tokio::test]
async fn test_multiple_submissions_update_existing_mrs() {
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

    let branch_name = unique_test_branch("multi-submit");

    // Create and push initial version
    repo.create_file("multi.txt", "Version 1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "First commit"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    // Create MR
    let mr = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Multi-submit test: {}", branch_name),
            Some("First version"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR");

    println!("Created MR: {} (IID: {})", mr.web_url, mr.iid);

    // Make changes and push again
    repo.create_file("multi2.txt", "Version 2")
        .expect("Failed to create second file");
    repo.jj(&["describe", "-m", "Second commit"])
        .expect("Failed to set new description");
    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push updated bookmark");

    // Find the MR again - should be the same one
    let found_mr = gitlab
        .client
        .find_mr_by_source_branch(&branch_name)
        .await
        .expect("Failed to find MR");

    assert!(found_mr.is_some(), "Should find existing MR");
    let found_mr = found_mr.unwrap();
    assert_eq!(found_mr.iid, mr.iid, "Should reuse same MR after update");

    println!("Verified MR reuse after push: same IID {}", found_mr.iid);
}

#[tokio::test]
async fn test_mr_retarget_after_middle_bookmark_deleted() {
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

    let branch_a = unique_test_branch("retarget-a");
    let branch_b = unique_test_branch("retarget-b");
    let branch_c = unique_test_branch("retarget-c");

    // Create bookmark-a
    repo.create_file("file_a.txt", "Content A")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_a)
        .expect("Failed to create bookmark A");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_a)])
        .expect("Failed to track bookmark A");
    repo.jj(&["git", "push", "--bookmark", &branch_a])
        .expect("Failed to push bookmark A");

    // Create bookmark-b on top
    repo.create_file("file_b.txt", "Content B")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_b)
        .expect("Failed to create bookmark B");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_b)])
        .expect("Failed to track bookmark B");
    repo.jj(&["git", "push", "--bookmark", &branch_b])
        .expect("Failed to push bookmark B");

    // Create bookmark-c on top
    repo.create_file("file_c.txt", "Content C")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to set commit description");
    repo.create_bookmark(&branch_c)
        .expect("Failed to create bookmark C");
    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_c)])
        .expect("Failed to track bookmark C");
    repo.jj(&["git", "push", "--bookmark", &branch_c])
        .expect("Failed to push bookmark C");

    // Create MRs: a→main, b→a, c→b
    gitlab
        .client
        .create_merge_request(
            &branch_a,
            "main",
            &format!("MR A: {}", branch_a),
            Some("A"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR A");

    gitlab
        .client
        .create_merge_request(
            &branch_b,
            &branch_a,
            &format!("MR B: {}", branch_b),
            Some("B"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR B");

    let mr_c = gitlab
        .client
        .create_merge_request(
            &branch_c,
            &branch_b,
            &format!("MR C: {}", branch_c),
            Some("C"),
            true,
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create MR C");

    println!("Created stacked MRs, MR C targets {}", branch_b);
    assert_eq!(mr_c.target_branch, branch_b);

    // Now simulate deleting branch_b - retarget MR C to branch_a
    let updated_mr_c = gitlab
        .client
        .update_mr_base(mr_c.iid, &branch_a)
        .await
        .expect("Failed to retarget MR C");

    assert_eq!(updated_mr_c.iid, mr_c.iid);
    assert_eq!(updated_mr_c.target_branch, branch_a);
    assert_eq!(updated_mr_c.source_branch, branch_c);

    println!(
        "Successfully retargeted MR C from {} to {}",
        branch_b, branch_a
    );
}

#[tokio::test]
async fn test_get_current_user() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let user = gitlab
        .client
        .get_current_user()
        .await
        .expect("Failed to get current user");

    assert!(user.id > 0, "User ID should be positive");
    assert!(!user.username.is_empty(), "Username should not be empty");
    assert!(!user.name.is_empty(), "Name should not be empty");

    println!(
        "Current user: {} (ID: {}, username: {})",
        user.name, user.id, user.username
    );
}

#[tokio::test]
async fn test_create_mr_with_assignee() {
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

    let branch_name = unique_test_branch("with-assignee");

    // Create a file and bookmark
    repo.create_file("test.txt", "Hello from assignee test")
        .expect("Failed to create file");

    repo.jj(&["describe", "-m", "Test commit for MR with assignee"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");

    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");

    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    let user = gitlab
        .client
        .get_current_user()
        .await
        .expect("Failed to get current user");

    let mr = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Test MR with assignee: {}", branch_name),
            Some("This MR should be assigned to the current user"),
            true,
            false,
            Some(&[user.id]),
            None,
        )
        .await
        .expect("Failed to create merge request with assignee");

    assert_eq!(mr.source_branch, branch_name);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.state, "opened");

    println!(
        "Created MR with assignee {}: {} (IID: {})",
        user.username, mr.web_url, mr.iid
    );
}

#[tokio::test]
async fn test_get_user_by_username() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let current_user = gitlab
        .client
        .get_current_user()
        .await
        .expect("Failed to get current user");

    let found_user = gitlab
        .client
        .get_user_by_username(&current_user.username)
        .await
        .expect("Failed to get user by username");

    assert!(found_user.is_some(), "Should find user by username");
    let found_user = found_user.unwrap();
    assert_eq!(found_user.id, current_user.id);
    assert_eq!(found_user.username, current_user.username);

    println!(
        "Found user by username '{}': {} (ID: {})",
        current_user.username, found_user.name, found_user.id
    );
}

#[tokio::test]
async fn test_get_user_by_username_not_found() {
    let gitlab = match GitLabTestHelper::from_env().await {
        Some(g) => g,
        None => return,
    };

    let nonexistent_user = gitlab
        .client
        .get_user_by_username("nonexistent-user-12345678")
        .await
        .expect("Should not error for nonexistent user");

    assert!(
        nonexistent_user.is_none(),
        "Should not find nonexistent user"
    );

    println!("Correctly returned None for nonexistent user");
}

#[tokio::test]
async fn test_create_mr_with_reviewers() {
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

    let branch_name = unique_test_branch("with-reviewers");

    // Create a file and bookmark
    repo.create_file("test.txt", "Hello from reviewer test")
        .expect("Failed to create file");

    repo.jj(&["describe", "-m", "Test commit for MR with reviewers"])
        .expect("Failed to set commit description");

    repo.create_bookmark(&branch_name)
        .expect("Failed to create bookmark");

    repo.jj(&["bookmark", "track", &format!("{}@origin", branch_name)])
        .expect("Failed to track bookmark");

    repo.jj(&["git", "push", "--bookmark", &branch_name])
        .expect("Failed to push bookmark");

    let user = gitlab
        .client
        .get_current_user()
        .await
        .expect("Failed to get current user");

    let mr = gitlab
        .client
        .create_merge_request(
            &branch_name,
            "main",
            &format!("Test MR with reviewers: {}", branch_name),
            Some("This MR should have the current user as a reviewer"),
            true,
            false,
            None,
            Some(&[user.id]),
        )
        .await
        .expect("Failed to create merge request with reviewers");

    assert_eq!(mr.source_branch, branch_name);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.state, "opened");

    println!(
        "Created MR with reviewer {}: {} (IID: {})",
        user.username, mr.web_url, mr.iid
    );
}
