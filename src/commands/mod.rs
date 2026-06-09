use unicode_segmentation::UnicodeSegmentation;

pub mod init;
pub mod status;
pub mod submit;

pub enum GetBookmarksOptions {
    /// Use a manual revset
    Revset(String),

    /// Include only `(mine() & tracked_remote_bookmarks()) ~ trunk()`
    Tracked,

    /// Include only `(mine() & bookmarks()) ~ trunk()`
    Mine,
}

impl GetBookmarksOptions {
    #[must_use]
    pub fn to_revset(&self) -> String {
        match self {
            GetBookmarksOptions::Revset(revset) => revset.clone(),
            GetBookmarksOptions::Tracked => {
                "(mine() & tracked_remote_bookmarks()) ~ ::trunk()".to_string()
            }
            GetBookmarksOptions::Mine => "(mine() & bookmarks()) ~ ::trunk()".to_string(),
        }
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
        strip_ansi::strip_str(self.as_ref()).graphemes(true).count()
    }
}
