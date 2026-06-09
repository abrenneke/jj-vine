use reqwest::{Method, StatusCode};
use snafu::{Backtrace, ChainCompat, Location, Snafu};

pub type Result<T, E = Error> = std::result::Result<T, E>;

macro_rules! make_whatever {
    ($fmt:literal$(, $($arg:expr),* $(,)?)?) => {
        snafu::FromString::without_source(
            snafu::__format!($fmt$(, $($arg),*)*),
        )
    };
}

pub(crate) use make_whatever;

#[allow(unused)]
macro_rules! err_whatever {
($fmt:literal$(, $($arg:expr),* $(,)?)?) => {
        core::result::Result::Err(make_whatever!($fmt$(, $($arg),*)*))
    };
}

#[allow(unused)]
pub(crate) use err_whatever;

#[derive(Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("{} error(s) occurred: {:?}", errors.len(), errors))]
    Aggregate {
        errors: Vec<Box<dyn std::error::Error>>,
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

        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
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
pub struct ClonableError {
    pub message: String,
    backtrace: Option<String>,
    location: Option<Location>,
}

impl snafu::Error for ClonableError {}

impl std::fmt::Display for ClonableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
    #[must_use]
    pub fn backtrace(&self) -> Option<&Backtrace> {
        match self {
            Error::JjCommand { backtrace, .. }
            | Error::GitCommand { backtrace, .. }
            | Error::GitLabApi { backtrace, .. }
            | Error::GitHubApi { backtrace, .. }
            | Error::ForgejoApi { backtrace, .. }
            | Error::Clap { backtrace, .. }
            | Error::Config { backtrace, .. }
            | Error::BookmarkNotFound { backtrace, .. }
            | Error::InvalidGraph { backtrace, .. }
            | Error::Io { backtrace, .. }
            | Error::Http { backtrace, .. }
            | Error::Json { backtrace, .. }
            | Error::Utf8 { backtrace, .. }
            | Error::Parse { backtrace, .. }
            | Error::Other { backtrace, .. }
            | Error::InvalidComponent { backtrace, .. }
            | Error::Aggregate { backtrace, .. }
            | Error::AzureDevOpsApi { backtrace, .. } => Some(backtrace),
        }
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

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_trace(self, f)?;

        if let Some(backtrace) = self.backtrace() {
            writeln!(f, "\nBacktrace:\n{backtrace}")?;
        }

        if let Some(location) = self.location() {
            writeln!(f, "\nLocation:\n{location}")?;
        }

        Ok(())
    }
}

fn error_trace(error: &Error, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
    writeln!(f, "{error}")?;

    let sources = ChainCompat::new(error).skip(1);
    let plurality = sources.clone().take(2).count();

    match plurality {
        0 => {}
        1 => writeln!(f, "\nCaused by this error:")?,
        _ => writeln!(f, "\nCaused by these errors (recent errors listed first):")?,
    }

    for (i, source) in sources.enumerate() {
        // Let's use 1-based indexing for presentation
        let i = i + 1;
        writeln!(f, "{i:3}: {source}")?;
    }

    Ok(())
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        make_whatever!("{}", message.into())
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

impl From<std::num::ParseIntError> for Error {
    fn from(source: std::num::ParseIntError) -> Self {
        ParseSnafu {
            message: source.to_string(),
        }
        .build()
    }
}

#[cfg(test)]
mod tests {
    use snafu::ResultExt;

    use super::*;

    #[test]
    fn test_error_new() {
        let err = Error::new("test error");
        assert_eq!(err.to_string(), "test error");
    }

    #[test]
    fn test_config_error() {
        let err = ConfigSnafu {
            message: "missing config".to_string(),
        }
        .build();
        assert_eq!(err.to_string(), "Configuration error: missing config");
    }

    #[test]
    fn test_bookmark_not_found() {
        let err = BookmarkNotFoundSnafu {
            name: "feature".to_string(),
        }
        .build();
        assert_eq!(err.to_string(), "Bookmark not found: feature");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_utf8_error_conversion() {
        let bytes = vec![0, 159, 146, 150]; // Invalid UTF-8
        let err = String::from_utf8(bytes).context(Utf8Snafu).unwrap_err();
        assert!(err.to_string().contains("UTF-8 decoding error"));
    }

    #[test]
    fn test_jj_command_error() {
        let err = JjCommandSnafu {
            message: "command failed".to_string(),
            output: None,
        }
        .build();
        assert_eq!(err.to_string(), "jj command failed: command failed");
    }

    #[test]
    fn test_gitlab_api_error() {
        let err = GitLabApiSnafu {
            message: "API returned 404".to_string(),
            method: Method::POST,
            url: "https://example.com/api/foo",
            status: StatusCode::NOT_FOUND,
            response_body: "404 Not Found",
        }
        .build();
        assert_eq!(err.to_string(), "GitLab API error: API returned 404");
    }
}
