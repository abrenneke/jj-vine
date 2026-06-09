use reqwest::{Method, StatusCode};
use snafu::{Backtrace, ChainCompat, ErrorCompat as _, Location, Snafu};

pub type Result<T, E = Error> = core::result::Result<T, E>;

macro_rules! make_whatever {
    ($fmt:literal$(, $($arg:expr),* $(,)?)?) => {
        snafu::FromString::without_source(
            snafu::__format!($fmt$(, $($arg),*)*),
        )
    };
}

pub(crate) use make_whatever;

#[expect(unused, reason = "will use")]
macro_rules! err_whatever {
($fmt:literal$(, $($arg:expr),* $(,)?)?) => {
        core::result::Result::Err(make_whatever!($fmt$(, $($arg),*)*))
    };
}

#[expect(unused, reason = "will use")]
pub(crate) use err_whatever;

#[derive(Snafu)]
#[snafu(visibility(pub))]
#[expect(clippy::error_impl_error, reason = "will think about it")]
pub enum Error {
    #[snafu(display("{} error(s) occurred: {:?}", errors.len(), errors))]
    Aggregate {
        errors: Vec<Box<dyn core::error::Error>>,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("jj command failed: {message}"))]
    JjCommand {
        message: String,
        output: Option<std::process::Output>,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("git command failed: {message}"))]
    GitCommand {
        message: String,
        output: Option<std::process::Output>,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("GitLab API error: {message}"))]
    GitLabApi {
        message: String,
        backtrace: Box<Backtrace>,

        method: Method,
        url: String,
        status: StatusCode,
        response_body: String,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("GitHub API error: {message}"))]
    GitHubApi {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Forgejo API error: {message}"))]
    ForgejoApi {
        message: String,
        status: reqwest::StatusCode,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Azure DevOps API error: {message}"))]
    AzureDevOpsApi {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("CLI error: {source}\nArguments: {arguments:?}\n{source}"))]
    Clap {
        arguments: Vec<String>,
        source: clap::Error,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Configuration error: {message}"))]
    Config {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Bookmark not found: {name}"))]
    BookmarkNotFound {
        name: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Invalid bookmark graph: {message}"))]
    InvalidGraph {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("IO error: {message}"))]
    Io {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("HTTP request failed: {message}"))]
    Http {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("JSON error: {source}\n\nJSON: {json}"))]
    Json {
        source: serde_json::Error,
        json: String,

        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("UTF-8 decoding error: {source}"))]
    Utf8 {
        source: std::string::FromUtf8Error,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("Parse error: {message}"))]
    Parse {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },

    #[snafu(display("{message}"), whatever)]
    Other {
        message: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,

        #[snafu(source(from(Box<dyn core::error::Error>, Some)))]
        source: Option<Box<dyn core::error::Error>>,
    },

    #[snafu(display("Invalid component: {component}"))]
    InvalidComponent {
        component: String,
        backtrace: Box<Backtrace>,

        #[snafu(implicit)]
        location: Box<Location>,
    },
}

#[derive(Debug, Clone)]
#[expect(clippy::module_name_repetitions, reason = "this one is fine")]
pub struct ClonableError {
    pub message: String,
    pub backtrace: Option<String>,
    pub location: Option<Location>,
}

impl snafu::Error for ClonableError {}

impl core::fmt::Display for ClonableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(backtrace) = &self.backtrace {
            writeln!(f, "\nBacktrace:\n{backtrace}")?;
        }
        if let Some(location) = &self.location {
            writeln!(f, "\nLocation:\n{location}")?;
        }
        Ok(())
    }
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        make_whatever!("{}", message.into())
    }

    #[must_use]
    pub fn location(&self) -> Option<&Location> {
        match self {
            Error::JjCommand { location, .. }
            | Error::GitCommand { location, .. }
            | Error::GitLabApi { location, .. }
            | Error::GitHubApi { location, .. }
            | Error::ForgejoApi { location, .. }
            | Error::Clap { location, .. }
            | Error::Config { location, .. }
            | Error::BookmarkNotFound { location, .. }
            | Error::InvalidGraph { location, .. }
            | Error::Io { location, .. }
            | Error::Http { location, .. }
            | Error::Json { location, .. }
            | Error::Utf8 { location, .. }
            | Error::Parse { location, .. }
            | Error::Other { location, .. }
            | Error::InvalidComponent { location, .. }
            | Error::Aggregate { location, .. }
            | Error::AzureDevOpsApi { location, .. } => Some(location),
        }
    }

    #[must_use]
    pub fn to_clonable_error(&self) -> ClonableError {
        ClonableError {
            message: self.to_string(),
            backtrace: self.backtrace().map(ToString::to_string),
            location: self.location().copied(),
        }
    }
}

impl core::fmt::Debug for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "{self}")?;

        let sources = ChainCompat::new(self).skip(1);
        let plurality = sources.clone().take(2).count();

        match plurality {
            0 => {}
            1 => writeln!(f, "\nCaused by this error:")?,
            _ => writeln!(f, "\nCaused by these errors (recent errors listed first):")?,
        }

        for (i, source) in sources.enumerate() {
            // Let's use 1-based indexing for presentation
            let i = i.saturating_add(1);
            writeln!(f, "{i:3}: {source}")?;
        }

        if let Some(backtrace) = self.backtrace() {
            writeln!(f, "\nBacktrace:\n{backtrace}")?;
        }

        if let Some(location) = self.location() {
            writeln!(f, "\nLocation:\n{location}")?;
        }

        Ok(())
    }
}

// Implement From for common error types
impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        IoSnafu {
            message: source.to_string(),
        }
        .build()
    }
}

impl From<reqwest::Error> for Error {
    fn from(source: reqwest::Error) -> Self {
        HttpSnafu {
            message: source.to_string(),
        }
        .build()
    }
}

impl From<dialoguer::Error> for Error {
    fn from(source: dialoguer::Error) -> Self {
        IoSnafu {
            message: source.to_string(),
        }
        .build()
    }
}

impl From<core::num::ParseIntError> for Error {
    fn from(source: core::num::ParseIntError) -> Self {
        ParseSnafu {
            message: source.to_string(),
        }
        .build()
    }
}

#[cfg(test)]
mod tests {
    use snafu::ResultExt as _;

    use super::*;

    #[test]
    fn error_new() {
        let err = Error::new("test error");
        assert_eq!(err.to_string(), "test error");
    }

    #[test]
    fn config_error() {
        let err = ConfigSnafu {
            message: "missing config".to_owned(),
        }
        .build();
        assert_eq!(err.to_string(), "Configuration error: missing config");
    }

    #[test]
    fn bookmark_not_found() {
        let err = BookmarkNotFoundSnafu {
            name: "feature".to_owned(),
        }
        .build();
        assert_eq!(err.to_string(), "Bookmark not found: feature");
    }

    #[test]
    #[expect(clippy::std_instead_of_core, reason = "gated feature")]
    fn io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn utf8_error_conversion() {
        let bytes = vec![0, 159, 146, 150]; // Invalid UTF-8
        let err = String::from_utf8(bytes).context(Utf8Snafu).unwrap_err();
        assert!(err.to_string().contains("UTF-8 decoding error"));
    }

    #[test]
    fn jj_command_error() {
        let err = JjCommandSnafu {
            message: "command failed".to_owned(),
            output: None,
        }
        .build();
        assert_eq!(err.to_string(), "jj command failed: command failed");
    }

    #[test]
    fn gitlab_api_error() {
        let err = GitLabApiSnafu {
            message: "API returned 404".to_owned(),
            method: Method::POST,
            url: "https://example.com/api/foo",
            status: StatusCode::NOT_FOUND,
            response_body: "404 Not Found",
        }
        .build();
        assert_eq!(err.to_string(), "GitLab API error: API returned 404");
    }
}
