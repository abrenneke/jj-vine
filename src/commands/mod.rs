use snafu::ensure;

use crate::{
    error::{CLISnafu, Result},
    jj::{Bookmark, Jujutsu},
};

pub mod init;
pub mod status;
pub mod submit;

pub struct GetBookmarksOptions {
    /// Use a manual revset
    revset: Option<String>,

    /// Include only `(mine() & tracked_remote_bookmarks()) ~ trunk()`
    tracked: bool,

    /// Include only `(mine() & bookmarks()) ~ trunk()`
    mine: bool,
}

/// Get bookmarks from the repository using CLI flags
pub fn get_bookmarks(options: &GetBookmarksOptions, jj: &Jujutsu) -> Result<Vec<Bookmark>> {
    let mut desired_revsets = vec![options.revset.as_deref()];

    if options.tracked {
        desired_revsets.push(Some("(mine() & tracked_remote_bookmarks()) ~ trunk()"));
    }

    if options.mine {
        desired_revsets.push(Some("(mine() & bookmarks()) ~ trunk()"));
    }

    let desired_revsets: Vec<_> = desired_revsets.iter().flatten().collect();

    ensure!(
        !desired_revsets.is_empty(),
        CLISnafu {
            message: "Must specify either a revset or use --tracked flag".to_string(),
        }
    );

    ensure!(
        desired_revsets.len() <= 1,
        CLISnafu {
            message:
                "Cannot specify both a revset and --tracked flag. Please use one or the other."
                    .to_string(),
        }
    );

    jj.get_bookmarks_with_revset(desired_revsets.first().unwrap())
}
