use assertables::{
    assert_any,
    assert_contains,
    assert_is_empty,
    assert_none,
    assert_not_contains,
    assert_some,
};

use crate::{
    bookmark::{Bookmark, BookmarkGraph, BookmarkRef},
    error::Result,
    tests::TestRepo,
};

#[test]
fn test_deleted_middle_bookmark() -> Result<()> {
    let repo = TestRepo::new();

    // a -> b -> c
    repo.commit_with_bookmark("file1.txt", "content1", "Commit A", "bookmark-a")
        .commit_with_bookmark("file2.txt", "content2", "Commit B", "bookmark-b")
        .commit_with_bookmark("file3.txt", "content3", "Commit C", "bookmark-c");

    repo.jj.exec(["bookmark", "delete", "bookmark-b"]).unwrap();

    let changes = repo.jj.log("mine() & bookmarks()")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;

    let bookmark_a = graph.find_bookmark_in_components("bookmark-a").unwrap();

    assert_none!(graph.find_bookmark_in_components("bookmark-b"));

    let bookmark_c = graph.find_bookmark_in_components("bookmark-c").unwrap();

    assert_any!(bookmark_c.parents.iter(), |p| p
        == &BookmarkRef::Bookmark(bookmark_a.clone()));

    Ok(())
}

#[test]
fn test_base_branch_not_included_in_submission() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();
    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.jj
        .exec(["bookmark", "track", "main", "--remote", "origin"])?;

    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("f1.txt", "feature1", "Feature 1")
        .create_bookmark("feature-1");

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("f2.txt", "feature2", "Feature 2")
        .create_bookmark("feature-2");

    let changes = repo.jj.log("::feature-2")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;
    let stack = graph.component_containing("feature-2").unwrap();

    assert_contains!(stack, "feature-1");
    assert_contains!(stack, "feature-2");
    assert_not_contains!(stack, "main");

    Ok(())
}

#[test]
fn test_submit_base_branch_errors() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();

    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    repo.jj
        .exec(["bookmark", "track", "main", "--remote", "origin"])?;

    let changes = repo.jj.log("main")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();
    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), true)?;

    assert_is_empty!(graph.components());

    Ok(())
}

#[test]
fn test_graph_skips_default_branch_history() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();
    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.jj.exec(["bookmark", "track", "main"])?;

    repo.jj.exec(["new", "main"])?;

    for i in 1..=50 {
        repo.create_change(
            &format!("file{}.txt", i),
            &format!("content {}", i),
            &format!("Commit {}", i),
        );
        repo.jj.exec(["commit", "-m", &format!("Commit {}", i)])?;
    }

    repo.jj.exec(["bookmark", "set", "main", "--to", "@-"])?;
    repo.jj.exec(["git", "push"])?;

    repo.jj.exec(["new", "main"])?;
    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    let changes = repo.jj.log("mine() & bookmarks()")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;

    assert_none!(graph.component_containing("main"));
    assert_some!(graph.component_containing("feature-1"));

    Ok(())
}

#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use assertables::assert_contains;

    use crate::{
        error::Result,
        tests::{TestRepo, unique_branch},
    };

    #[tokio::test]
    async fn test_multiple_independent_stacks_dont_incorrectly_retarget() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        // main -> a -> b
        // main -> c

        repo.jj.exec(["new", "main"])?;
        let a = unique_branch("a");
        repo.create_change("file1.txt", "a content", "A")
            .create_and_push_bookmark(&a);

        repo.jj.exec(["new"])?;
        let b = unique_branch("b");
        repo.create_change("file2.txt", "b content", "B")
            .create_and_push_bookmark(&b);

        repo.jj.exec(["new", "main"])?;
        let c = unique_branch("c");
        repo.create_change("file3.txt", "c content", "C")
            .create_and_push_bookmark(&c);

        let output = repo.run(["submit", "--tracked", "--dry-run"]).await;

        // Verify that c targets main, not b
        assert_contains!(output, &format!("Would create {} -> main", a));
        assert_contains!(output, &format!("Would create {} -> {}", b, a));
        assert_contains!(output, &format!("Would create {} -> main", c));

        Ok(())
    }
}
