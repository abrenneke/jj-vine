#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use assertables::{assert_contains, assert_not_contains};

    use crate::{
        error::Result,
        tests::{TestRepo, unique_branch},
    };

    #[tokio::test]
    async fn test_tracked_only_includes_pushed_bookmarks() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let a = unique_branch("tracked-a");
        repo.jj.exec(["new", "main"])?;
        repo.create_change("a.txt", "a", "Commit A")
            .create_and_push_bookmark(&a);

        let b = unique_branch("tracked-b");
        repo.jj.exec(["new"])?;
        repo.create_change("b.txt", "b", "Commit B")
            .create_bookmark(&b);

        let output = repo.run(["submit", "--tracked", "--dry-run"]).await;

        assert_contains!(output, &a);
        assert_not_contains!(output, &b);

        Ok(())
    }

    #[tokio::test]
    async fn test_tracked_excludes_default_branch() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let branch = unique_branch("tracked-feature");
        repo.jj.exec(["new", "main"])?;
        repo.create_change("feature.txt", "feature", "Feature commit")
            .create_and_push_bookmark(&branch);

        let output = repo.run(["submit", "--tracked", "--dry-run"]).await;

        assert_contains!(output, &branch);
        assert_not_contains!(output, "Would create main");

        Ok(())
    }
}
