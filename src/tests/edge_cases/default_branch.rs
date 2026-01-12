use crate::{
    bookmark::BookmarkGraph,
    config::{Config, GitLabConfig},
    jj::Jujutsu,
    submit::analyze,
    tests::TestRepo,
};

/// Test that BookmarkGraph respects different default branch names
#[tokio::test]
async fn test_default_branch_configuration() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_change("file1.txt", "content1", "First commit");
    repo.jj(["new"]);
    repo.create_bookmark("feature-a");

    repo.create_change("file2.txt", "content2", "Second commit");
    repo.jj(["new"]);
    repo.create_bookmark("feature-b");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");

    // Test with "main" as default
    let graph_main = BookmarkGraph::build(&jj, "main", bookmarks.clone())
        .await
        .expect("Failed to build graph");
    let stack = graph_main
        .find_stack_for_bookmark("feature-b")
        .expect("Should find stack");
    assert_eq!(stack.base, "main");

    // Test with "develop" as default
    let graph_develop = BookmarkGraph::build(&jj, "develop", bookmarks.clone())
        .await
        .expect("Failed to build graph");
    let stack = graph_develop
        .find_stack_for_bookmark("feature-b")
        .expect("Should find stack");
    assert_eq!(stack.base, "develop");

    // Test with "master" as default
    let graph_master = BookmarkGraph::build(&jj, "master", bookmarks)
        .await
        .expect("Failed to build graph");
    let stack = graph_master
        .find_stack_for_bookmark("feature-b")
        .expect("Should find stack");
    assert_eq!(stack.base, "master");
}

/// Test that base branch is not included in bookmarks_to_submit
#[tokio::test]
async fn test_base_branch_not_included_in_submission() {
    let repo = TestRepo::new();

    // Create main bookmark
    repo.create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    // Create feature stack
    repo.jj(["new"]);
    repo.create_change("f1.txt", "feature1", "Feature 1")
        .create_bookmark("feature-1");

    repo.jj(["new"]);
    repo.create_change("f2.txt", "feature2", "Feature 2")
        .create_bookmark("feature-2");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu");
    let config = Config {
        forge: crate::config::ForgeType::GitLab,
        gitlab: GitLabConfig {
            host: "https://gitlab.example.com".to_string(),
            project: "test/project".to_string(),
            token: "fake-token".to_string(),
        },
        forgejo: Default::default(),
        github: Default::default(),
        default_branch: "main".to_string(),
        remote_name: "origin".to_string(),
        ca_bundle: None,
        tls_accept_non_compliant_certs: false,
        enable_stack_visualization: true,
        stack_format: crate::config::StackFormat::Linear,
        delete_source_branch: true,
        squash_commits: false,
        assign_to_self: false,
        default_reviewers: vec![],
    };

    let analysis = analyze::analyze(&jj, &config, &["feature-2".to_string()])
        .await
        .expect("Failed to analyze");

    // Main should NOT be in bookmarks_to_submit
    assert!(
        !analysis.bookmarks_to_submit.contains(&"main".to_string()),
        "Base branch should not be in submission list: {:?}",
        analysis.bookmarks_to_submit
    );

    // Feature bookmarks SHOULD be included
    assert!(
        analysis
            .bookmarks_to_submit
            .contains(&"feature-1".to_string())
    );
    assert!(
        analysis
            .bookmarks_to_submit
            .contains(&"feature-2".to_string())
    );
}

/// Test that attempting to submit the base branch errors
#[tokio::test]
async fn test_submit_base_branch_errors() {
    let repo = TestRepo::new();

    repo.create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu");
    let config = Config {
        forge: crate::config::ForgeType::GitLab,
        gitlab: GitLabConfig {
            host: "https://gitlab.example.com".to_string(),
            project: "test/project".to_string(),
            token: "fake-token".to_string(),
        },
        forgejo: Default::default(),
        github: Default::default(),
        default_branch: "main".to_string(),
        remote_name: "origin".to_string(),
        ca_bundle: None,
        tls_accept_non_compliant_certs: false,
        enable_stack_visualization: true,
        stack_format: crate::config::StackFormat::Linear,
        delete_source_branch: true,
        squash_commits: false,
        assign_to_self: false,
        default_reviewers: vec![],
    };

    let result = analyze::analyze(&jj, &config, &["main".to_string()]).await;

    assert!(result.is_err(), "Should error when submitting base branch");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("base branch"),
        "Error should mention base branch: {}",
        err
    );
}

/// Test that graph building doesn't traverse entire default branch history
#[tokio::test]
async fn test_graph_skips_default_branch_history() {
    let repo = TestRepo::new();

    // Create initial bookmark
    repo.create_change("initial.txt", "initial", "Initial commit")
        .create_bookmark("initial");

    // Create 50 commits on master (simulating long history)
    for i in 1..=50 {
        repo.jj(["new"]);
        repo.create_change(
            &format!("file{}.txt", i),
            &format!("content {}", i),
            &format!("Commit {}", i),
        );
    }
    repo.create_bookmark("master");

    // Create feature off master
    repo.jj(["new", "master"]);
    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    // Build graph - should complete quickly, not traverse all 50+ commits
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");

    let graph = BookmarkGraph::build(&jj, "master", bookmarks)
        .await
        .expect("Failed to build graph");

    // Master has no parent (it's the default branch)
    assert_eq!(graph.get_parent("master"), None);

    // Feature has master as parent
    assert_eq!(graph.get_parent("feature-1"), Some(&"master".to_string()));
}
