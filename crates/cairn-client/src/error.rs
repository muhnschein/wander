use std::fmt;

pub mod code {
    pub const BAD_REQUEST: &str = "bad_request";
    pub const BAD_UUID: &str = "bad_uuid";
    pub const BAD_PATH: &str = "bad_path";
    pub const BAD_QUERY: &str = "bad_query";
    pub const BODY_NOT_ALLOWED: &str = "body_not_allowed";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const NOT_FOUND: &str = "not_found";
    pub const METHOD_NOT_ALLOWED: &str = "method_not_allowed";
    pub const REQUEST_TIMEOUT: &str = "request_timeout";
    pub const URI_TOO_LONG: &str = "uri_too_long";
    pub const RANGE_NOT_SATISFIABLE: &str = "range_not_satisfiable";
    pub const TOO_MANY_REQUESTS: &str = "too_many_requests";
    pub const HEADERS_TOO_LARGE: &str = "headers_too_large";
    pub const INTERNAL: &str = "internal";
    pub const ARCHIVE_UNAVAILABLE: &str = "archive_unavailable";
    pub const VERSION_NOT_SUPPORTED: &str = "version_not_supported";
}

#[derive(Debug, Clone)]
pub enum Error {
    Api {
        status: u16,
        code: String,
        message: String,
    },
    Transport(String),
    Invalid(String),
}

impl Error {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Api { code, .. } if code == code::NOT_FOUND)
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Api { code, .. } if code == code::UNAUTHORIZED)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api {
                status,
                code,
                message,
            } => {
                write!(f, "cairn returned {status} ({code}): {message}")
            }
            Error::Transport(msg) => write!(f, "connection to cairn failed: {msg}"),
            Error::Invalid(msg) => write!(f, "invalid response from cairn: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
