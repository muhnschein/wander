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

    /// The code a well-behaved cairn would have sent for `status`, for use when
    /// the response body is missing or is not a cairn error envelope — a
    /// reverse proxy answering instead of cairn, say.
    ///
    /// Only statuses whose cairn code is a plain synonym of the HTTP status are
    /// mapped. 5xx deliberately falls through to [`INTERNAL`]: a 500 or 503
    /// from an intermediary says nothing about which cairn-specific failure
    /// occurred, and guessing [`ARCHIVE_UNAVAILABLE`] there would invent detail
    /// the response never carried.
    pub fn for_status(status: u16) -> &'static str {
        match status {
            400 => BAD_REQUEST,
            401 => UNAUTHORIZED,
            404 => NOT_FOUND,
            405 => METHOD_NOT_ALLOWED,
            408 => REQUEST_TIMEOUT,
            414 => URI_TOO_LONG,
            416 => RANGE_NOT_SATISFIABLE,
            429 => TOO_MANY_REQUESTS,
            431 => HEADERS_TOO_LARGE,
            _ => INTERNAL,
        }
    }
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

    /// The HTTP status cairn answered with, if this came from a response at all.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
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
