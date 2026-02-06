use crate::{error::Result, tests::TestRepo};

#[tokio::test]
async fn test_pr_title_generates() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    repo.set_config(
        "jj-vine.title.singleRevision",
        "\"[{stack_index}/{stack_count}] {head.description_first_line}\"",
    );

    let a = repo.bookmark_name("feature-a");
    let b = repo.bookmark_name("feature-b");

    // main -> A -> B
    repo.submit_stack([&a, &b]).await;

    let pr_a = repo.get_mr_with_base(&a, "main").await;
    let pr_b = repo.get_mr_with_base(&b, &a).await;

    assert_eq!(
        pr_a.pull_request.title,
        format!("[1/2] Commit for {} bookmark", a)
    );
    assert_eq!(
        pr_b.pull_request.title,
        format!("[2/2] Commit for {} bookmark", b)
    );

    Ok(())
}

#[tokio::test]
async fn test_pr_title_syncs_on_submit() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    repo.set_config("jj-vine.title.sync", "true");

    repo.set_config(
        "jj-vine.title.singleRevision",
        "\"[{stack_index}/{stack_count}] {head.description_first_line}\"",
    );

    let a = repo.bookmark_name("feature-a");
    let b = repo.bookmark_name("feature-b");
    let c = repo.bookmark_name("feature-c");

    // main -> A -> B
    repo.submit_stack([&a, &b]).await;

    repo.create_change_and_tracked_bookmark(&c);
    repo.new_on("@");

    // main -> A -> B -> C
    repo.run(["submit", &c]).await;

    let pr_a = repo.get_mr_with_base(&a, "main").await;
    let pr_b = repo.get_mr_with_base(&b, &a).await;
    let pr_c = repo.get_mr_with_base(&c, &b).await;

    assert_eq!(
        pr_a.pull_request.title,
        format!("[1/3] Commit for {} bookmark", a)
    );
    assert_eq!(
        pr_b.pull_request.title,
        format!("[2/3] Commit for {} bookmark", b)
    );
    assert_eq!(
        pr_c.pull_request.title,
        format!("[3/3] Commit for {} bookmark", c)
    );

    Ok(())
}

#[tokio::test]
async fn test_pr_title_does_not_sync_on_submit() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    repo.set_config("jj-vine.title.sync", "false");

    repo.set_config(
        "jj-vine.title.singleRevision",
        "\"[{stack_index}/{stack_count}] {head.description_first_line}\"",
    );

    let a = repo.bookmark_name("feature-a");
    let b = repo.bookmark_name("feature-b");
    let c = repo.bookmark_name("feature-c");

    // main -> A -> B
    repo.submit_stack([&a, &b]).await;

    repo.create_change_and_tracked_bookmark(&c);
    repo.new_on("@");

    // main -> A -> B -> C
    repo.run(["submit", &c]).await;

    let pr_a = repo.get_mr_with_base(&a, "main").await;
    let pr_b = repo.get_mr_with_base(&b, &a).await;
    let pr_c = repo.get_mr_with_base(&c, &b).await;

    assert_eq!(
        pr_a.pull_request.title,
        format!("[1/2] Commit for {} bookmark", a)
    );
    assert_eq!(
        pr_b.pull_request.title,
        format!("[2/2] Commit for {} bookmark", b)
    );
    assert_eq!(
        pr_c.pull_request.title,
        format!("[3/3] Commit for {} bookmark", c)
    );

    Ok(())
}
