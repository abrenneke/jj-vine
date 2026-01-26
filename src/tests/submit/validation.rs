#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use assertables::assert_contains;

    use crate::{error::Result, tests::TestRepo};

    #[tokio::test]
    async fn test_bookmark_and_tracked_mutually_exclusive() -> Result<()> {
        let repo = TestRepo::new();

        let result = repo.try_run(["submit", "some-bookmark", "--tracked"]).await;

        assert_contains!(result.unwrap_err().to_string(), "Usage:");

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_requires_bookmark_or_tracked() -> Result<()> {
        let repo = TestRepo::new();

        let result = repo.try_run(["submit"]).await;

        assert_contains!(result.unwrap_err().to_string(), "Usage:");

        Ok(())
    }

    #[tokio::test]
    async fn test_tracked_with_no_pushed_bookmarks() -> Result<()> {
        let repo = TestRepo::new();

        repo.jj.exec(["new"])?;
        repo.create_change("test.txt", "content", "Test commit")
            .create_bookmark("unpushed-branch");

        let result = repo.try_run(["submit", "--tracked"]).await;

        assert_contains!(
            result.unwrap_err().to_string(),
            "No bookmarks in revset (mine() & tracked_remote_bookmarks()) ~ trunk()"
        );

        Ok(())
    }

    #[tokio::test]
    #[cfg(not(feature = "no-e2e-tests"))]
    async fn test_push_failure_skips_mr_creation() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();
        let upstream = TestRepo::new();

        upstream
            .create_change("init.txt", "initial", "Initial commit")
            .create_bookmark("main");

        repo.jj.exec([
            "git",
            "remote",
            "set-url",
            "origin",
            &upstream.path.to_string_lossy(),
        ])?;

        repo.jj.exec(["git", "fetch"])?;
        repo.jj.exec(["bookmark", "track", "main"])?;
        repo.jj.exec(["new", "main"])?;
        repo.create_change("test.txt", "content", "Feature commit")
            .create_tracked_bookmark("feature-push-fail");

        std::fs::remove_dir_all(&upstream.path)?;

        let result = repo.try_run(["submit", "feature-push-fail"]).await;
        assert_contains!(result.unwrap_err().to_string(), "push");

        Ok(())
    }
}
