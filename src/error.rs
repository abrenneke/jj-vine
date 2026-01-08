use snafu::Snafu;
use std::process::Output;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("jj command failed: {message}"))]
    JjCommand {
        message: String,
        output: Option<Output>,
    },

    #[snafu(display("git command failed: {message}"))]
    GitCommand {
        message: String,
        output: Option<Output>,
    },

    #[snafu(display("GitLab API error: {message}"))]
    GitLabApi { message: String },

    #[snafu(display("Configuration error: {message}"))]
    Config { message: String },

    #[snafu(display("Bookmark not found: {name}"))]
    BookmarkNotFound { name: String },

    #[snafu(display("Invalid bookmark graph: {message}"))]
    InvalidGraph { message: String },

    #[snafu(display("IO error: {source}"))]
    Io { source: std::io::Error },

    #[snafu(display("HTTP request failed: {source}"))]
    Http { source: reqwest::Error },

    #[snafu(display("JSON error: {source}"))]
    Json { source: serde_json::Error },

    #[snafu(display("UTF-8 decoding error: {source}"))]
    Utf8 { source: std::string::FromUtf8Error },

    #[snafu(display("Parse error: {message}"))]
    Parse { message: String },

    #[snafu(display("{message}"))]
    Other { message: String },
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error::Other {
            message: message.into(),
        }
    }
}

// Implement From for common error types
impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::Io { source }
    }
}

impl From<reqwest::Error> for Error {
    fn from(source: reqwest::Error) -> Self {
        Error::Http { source }
    }
}

impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Error::Json { source }
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(source: std::string::FromUtf8Error) -> Self {
        Error::Utf8 { source }
    }
}

impl From<dialoguer::Error> for Error {
    fn from(source: dialoguer::Error) -> Self {
        Error::Io {
            source: std::io::Error::other(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_new() {
        let err = Error::new("test error");
        assert_eq!(err.to_string(), "test error");
    }

    #[test]
    fn test_config_error() {
        let err = Error::Config {
            message: "missing config".to_string(),
        };
        assert_eq!(err.to_string(), "Configuration error: missing config");
    }

    #[test]
    fn test_bookmark_not_found() {
        let err = Error::BookmarkNotFound {
            name: "feature".to_string(),
        };
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
        let utf8_err = String::from_utf8(bytes).unwrap_err();
        let err: Error = utf8_err.into();
        assert!(err.to_string().contains("UTF-8 decoding error"));
    }

    #[test]
    fn test_jj_command_error() {
        let err = Error::JjCommand {
            message: "command failed".to_string(),
            output: None,
        };
        assert_eq!(err.to_string(), "jj command failed: command failed");
    }

    #[test]
    fn test_gitlab_api_error() {
        let err = Error::GitLabApi {
            message: "API returned 404".to_string(),
        };
        assert_eq!(err.to_string(), "GitLab API error: API returned 404");
    }
}
