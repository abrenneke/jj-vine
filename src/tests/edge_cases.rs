use std::collections::HashSet;

use assertables::{
    assert_any,
    assert_contains,
    assert_is_empty,
    assert_none,
    assert_not_contains,
    assert_some,
};

use crate::{
    bookmark::{Bookmark, BookmarkGraph, BookmarkOrPending, BookmarkRef},
    error::Result,
    submit::find_changes_to_submit,
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
    let bookmarks: Vec<_> = BookmarkOrPending::from_changes(&changes)
        .into_iter()
        .collect();

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
    let repo = TestRepo::with_local_remote();

    repo.create_change("f1.txt", "feature1", "Feature 1")
        .create_bookmark("feature-1");

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("f2.txt", "feature2", "Feature 2")
        .create_bookmark("feature-2");

    let changes = repo.jj.log("::feature-2")?;
    let bookmarks: Vec<_> = BookmarkOrPending::from_changes(&changes)
        .into_iter()
        .collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;
    let stack = graph.component_containing("feature-2").unwrap();

    assert_contains!(stack, "feature-1");
    assert_contains!(stack, "feature-2");
    assert_not_contains!(stack, "main");

    Ok(())
}

#[test]
fn test_submit_base_branch_errors() -> Result<()> {
    let repo = TestRepo::with_local_remote();

    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    let changes = repo.jj.log("main")?;
    let bookmarks: Vec<_> = BookmarkOrPending::from_changes(&changes)
        .into_iter()
        .collect();
    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), true)?;

    assert_is_empty!(graph.components());

    Ok(())
}

#[test]
fn test_graph_skips_default_branch_history() -> Result<()> {
    let repo = TestRepo::with_local_remote();

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
    let bookmarks: Vec<_> = BookmarkOrPending::from_changes(&changes)
        .into_iter()
        .collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;

    assert_none!(graph.component_containing("main"));
    assert_some!(graph.component_containing("feature-1"));

    Ok(())
}

#[test]
fn test_find_changes_to_submit_with_advanced_main() -> Result<()> {
    let repo = TestRepo::with_local_remote();
    repo.create_change("f1.txt", "f1", "Feature 1")
        .create_bookmark("feature");

    repo.jj.exec(["new", "main"])?;
    repo.create_change("m1.txt", "m1", "Main advance");
    repo.jj.exec(["bookmark", "set", "main", "--to", "@"])?;
    repo.jj.exec(["git", "push"])?;

    repo.jj.exec(["new"])?;
    repo.create_change("f2.txt", "f2", "Feature 2")
        .create_bookmark("feature-2");

    repo.jj.exec(["new"])?;
    repo.create_change("f3.txt", "f3", "Feature 3")
        .create_bookmark("feature-3");

    // old-main --> main --> feature-2 --> feature-3
    //   /-->feature

    // From feature-3, we expect the whole downstack back to (but not including)
    // trunk — i.e. feature-2 and feature-3.
    let changes = find_changes_to_submit(&repo.jj, ["feature-3"], &HashSet::new())?;
    let mut names: Vec<_> = Bookmark::from_changes(&changes)
        .into_iter()
        .map(|b| b.name().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["feature-2".to_string(), "feature-3".to_string()]
    );

    // From feature, we expect just feature: it branched off old main but is
    // not in the ancestry of the new trunk, so it should not be filtered out.
    let changes = find_changes_to_submit(&repo.jj, ["feature"], &HashSet::new())?;
    let names: Vec<_> = Bookmark::from_changes(&changes)
        .into_iter()
        .map(|b| b.name().to_string())
        .collect();
    assert_eq!(names, vec!["feature".to_string()]);

    Ok(())
}

#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use assertables::assert_contains;

    use crate::{error::Result, tests::TestRepo};

    #[tokio::test]
    async fn test_multiple_independent_stacks_dont_incorrectly_retarget() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        // main -> a -> b
        // main -> c

        repo.jj.exec(["new", "main"])?;
        let a = repo.bookmark_name("a");
        repo.create_change("file1.txt", "a content", "A")
            .create_and_push_bookmark(&a);

        repo.jj.exec(["new"])?;
        let b = repo.bookmark_name("b");
        repo.create_change("file2.txt", "b content", "B")
            .create_and_push_bookmark(&b);

        repo.jj.exec(["new", "main"])?;
        let c = repo.bookmark_name("c");
        repo.create_change("file3.txt", "c content", "C")
            .create_and_push_bookmark(&c);

        let output = repo.run(["submit", "--tracked", "--dry-run"]).await;

        assert_contains!(output, &format!("Would create {} -> main", a));
        assert_contains!(output, &format!("Would create {} -> {}", b, a));
        assert_contains!(output, &format!("Would create {} -> main", c));

        Ok(())
    }

    #[tokio::test]
    async fn test_complex_bookmark_name() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let name = repo.bookmark_name("complex-bookmark--parent/complex--name");

        repo.create_change_and_bookmark(&name);

        let output = repo
            .run(["submit", &format!("\"{name}\""), "--dry-run"])
            .await;

        assert_contains!(output, &format!("Would create {name} -> main"));

        Ok(())
    }

    #[tokio::test]
    async fn test_complex_bookmark_name_tracked() -> Result<()> {
        let repo = TestRepo::with_forgejo_remote();

        let name = repo.bookmark_name("complex-bookmark--parent/complex--name");

        repo.create_change_and_tracked_bookmark(&name)
            .push_bookmark(&name);

        let output = repo.run(["submit", "--tracked", "--dry-run"]).await;

        assert_contains!(output, &format!("Would create {name} -> main"));

        Ok(())
    }
}
