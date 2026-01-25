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
        errors: Vec<Error>,
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

impl Error {
    pub fn backtrace(&self) -> Option<&Backtrace> {
        match self {
            Error::JjCommand { backtrace, .. } => Some(backtrace),
            Error::GitCommand { backtrace, .. } => Some(backtrace),
            Error::GitLabApi { backtrace, .. } => Some(backtrace),
            Error::GitHubApi { backtrace, .. } => Some(backtrace),
            Error::ForgejoApi { backtrace, .. } => Some(backtrace),
            Error::Clap { backtrace, .. } => Some(backtrace),
            Error::Config { backtrace, .. } => Some(backtrace),
            Error::BookmarkNotFound { backtrace, .. } => Some(backtrace),
            Error::InvalidGraph { backtrace, .. } => Some(backtrace),
            Error::Io { backtrace, .. } => Some(backtrace),
            Error::Http { backtrace, .. } => Some(backtrace),
            Error::Json { backtrace, .. } => Some(backtrace),
            Error::Utf8 { backtrace, .. } => Some(backtrace),
            Error::Parse { backtrace, .. } => Some(backtrace),
            Error::Other { backtrace, .. } => Some(backtrace),
            Error::InvalidComponent { backtrace, .. } => Some(backtrace),
            Error::Aggregate { backtrace, .. } => Some(backtrace),
            Error::AzureDevOpsApi { backtrace, .. } => Some(backtrace),
        }
    }

    pub fn location(&self) -> Option<&Location> {
        match self {
            Error::JjCommand { location, .. } => Some(location),
            Error::GitCommand { location, .. } => Some(location),
            Error::GitLabApi { location, .. } => Some(location),
            Error::GitHubApi { location, .. } => Some(location),
            Error::ForgejoApi { location, .. } => Some(location),
            Error::Clap { location, .. } => Some(location),
            Error::Config { location, .. } => Some(location),
            Error::BookmarkNotFound { location, .. } => Some(location),
            Error::InvalidGraph { location, .. } => Some(location),
            Error::Io { location, .. } => Some(location),
            Error::Http { location, .. } => Some(location),
            Error::Json { location, .. } => Some(location),
            Error::Utf8 { location, .. } => Some(location),
            Error::Parse { location, .. } => Some(location),
            Error::Other { location, .. } => Some(location),
            Error::InvalidComponent { location, .. } => Some(location),
            Error::Aggregate { location, .. } => Some(location),
            Error::AzureDevOpsApi { location, .. } => Some(location),
        }
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_trace(self, f)?;

        if let Some(backtrace) = self.backtrace() {
            writeln!(f, "\nBacktrace:\n{}", backtrace)?;
        }

        if let Some(location) = self.location() {
            writeln!(f, "\nLocation:\n{}", location)?;
        }

        Ok(())
    }
}

fn error_trace(error: &Error, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
    writeln!(f, "{}", error)?;

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
        writeln!(f, "{:3}: {}", i, source)?;
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
        }
        .build();
        assert_eq!(err.to_string(), "GitLab API error: API returned 404");
    }
}
