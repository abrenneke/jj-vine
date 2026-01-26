#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use assertables::{assert_contains, assert_lt};

    use crate::{
        error::Result,
        tests::{TestRepo, unique_branch},
    };

    #[tokio::test]
    async fn test_submit_dry_run_shows_would_create() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let branch = unique_branch("dry-run-test");
        repo.jj.exec(["new", "main"])?;
        repo.create_change("test.txt", "content", "Test commit")
            .create_and_push_bookmark(&branch);

        let output = repo.run(["submit", "--dry-run", &branch]).await;

        assert_contains!(output, &format!("Would create {} -> main", branch));

        Ok(())
    }

    #[tokio::test]
    async fn test_topological_ordering_in_stack() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let a = unique_branch("topo-a");
        let b = unique_branch("topo-b");
        let c = unique_branch("topo-c");

        // main -> A -> B -> C
        repo.jj.exec(["new", "main"])?;
        repo.create_change("a.txt", "a", "Commit A")
            .create_and_push_bookmark(&a);

        repo.jj.exec(["new"])?;
        repo.create_change("b.txt", "b", "Commit B")
            .create_and_push_bookmark(&b);

        repo.jj.exec(["new"])?;
        repo.create_change("c.txt", "c", "Commit C")
            .create_and_push_bookmark(&c);

        let output = repo.run(["submit", "--dry-run", &c]).await;

        let a_msg = format!("Would create {} -> main", a);
        let b_msg = format!("Would create {} -> {}", b, a);
        let c_msg = format!("Would create {} -> {}", c, b);

        assert_contains!(output, &a_msg);
        assert_contains!(output, &b_msg);
        assert_contains!(output, &c_msg);

        assert_lt!(output.find(&a_msg).unwrap(), output.find(&b_msg).unwrap());
        assert_lt!(output.find(&b_msg).unwrap(), output.find(&c_msg).unwrap());

        Ok(())
    }
}
