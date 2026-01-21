#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use crate::tests::{TestRepo, unique_branch};

    #[tokio::test]
    async fn test_submit_dry_run_shows_would_create() {
        let repo = TestRepo::with_gitlab_remote();

        let branch = unique_branch("dry-run-test");
        repo.jj.exec(["new", "main"]).unwrap();
        repo.create_change("test.txt", "content", "Test commit")
            .create_and_push_bookmark(&branch);

        let output = repo
            .submit(crate::commands::submit::SubmitCommandConfig {
                revset: Some(branch.clone()),
                dry_run: true,
                ..Default::default()
            })
            .await;

        // Output should mention the bookmark being submitted
        assert!(
            output.contains(&format!("Would create {} -> main", branch)),
            "Dry run should mention the bookmark. Output:\n{}",
            output
        );
    }

    #[tokio::test]
    async fn test_topological_ordering_in_stack() {
        let repo = TestRepo::with_gitlab_remote();

        // Create a stack: main -> A -> B -> C
        let branch_a = unique_branch("topo-a");
        let branch_b = unique_branch("topo-b");
        let branch_c = unique_branch("topo-c");

        repo.jj.exec(["new", "main"]).unwrap();
        repo.create_change("a.txt", "a", "Commit A")
            .create_and_push_bookmark(&branch_a);

        repo.jj.exec(["new"]).unwrap();
        repo.create_change("b.txt", "b", "Commit B")
            .create_and_push_bookmark(&branch_b);

        repo.jj.exec(["new"]).unwrap();
        repo.create_change("c.txt", "c", "Commit C")
            .create_and_push_bookmark(&branch_c);

        // Submit C - should process A, B, C in order
        let output = repo
            .submit(crate::commands::submit::SubmitCommandConfig {
                revset: Some(branch_c.clone()),
                dry_run: true,
                ..Default::default()
            })
            .await;

        // Check "Would create" lines appear in topological order (A before B before C)
        let create_a = format!("Would create {} -> main", branch_a);
        let create_b = format!("Would create {} -> {}", branch_b, branch_a);
        let create_c = format!("Would create {} -> {}", branch_c, branch_b);

        let pos_a = output.find(&create_a);
        let pos_b = output.find(&create_b);
        let pos_c = output.find(&create_c);

        assert!(
            pos_a.is_some(),
            "Output should have 'Would create A -> main'. Output:\n{}",
            output
        );
        assert!(
            pos_b.is_some(),
            "Output should have 'Would create B -> A'. Output:\n{}",
            output
        );
        assert!(
            pos_c.is_some(),
            "Output should have 'Would create C -> B'. Output:\n{}",
            output
        );

        // A should appear before B, B before C (topological order)
        assert!(
            pos_a < pos_b && pos_b < pos_c,
            "Bookmarks should appear in topological order A < B < C. Output:\n{}",
            output
        );
    }
}
