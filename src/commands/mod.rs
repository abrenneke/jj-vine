use unicode_segmentation::UnicodeSegmentation;

use crate::{
    error::{Error, Result},
    jj::{Change, Jujutsu},
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
pub fn get_changes_from_cli_args(
    options: &GetBookmarksOptions,
    jj: &Jujutsu,
) -> Result<Vec<Change>> {
    let mut desired_revsets = vec![options.revset.as_deref()];

    if options.tracked {
        desired_revsets.push(Some("(mine() & tracked_remote_bookmarks()) ~ trunk()"));
    }

    if options.mine {
        desired_revsets.push(Some("(mine() & bookmarks()) ~ trunk()"));
    }

    let desired_revsets: Vec<_> = desired_revsets.iter().flatten().collect();

    match desired_revsets[..] {
        [] => Err(Error::CLI {
            message: "Must specify either a revset or use --tracked flag".to_string(),
        }),
        [revset] => jj.log(revset),
        _ => Err(Error::CLI {
            message:
                "Cannot specify both a revset and --tracked flag. Please use one or the other."
                    .to_string(),
        }),
    }
}

trait StrVisualWidth {
    fn visual_width(&self) -> usize;
}

impl<T> StrVisualWidth for T
where
    T: AsRef<str>,
{
    fn visual_width(&self) -> usize {
        strip_ansi_escapes::strip_str(self).graphemes(true).count()
    }
}
