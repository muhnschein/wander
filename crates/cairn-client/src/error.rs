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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_that_are_synonyms_of_a_cairn_code_map_through() {
        assert_eq!(code::for_status(400), code::BAD_REQUEST);
        assert_eq!(code::for_status(401), code::UNAUTHORIZED);
        assert_eq!(code::for_status(404), code::NOT_FOUND);
        assert_eq!(code::for_status(405), code::METHOD_NOT_ALLOWED);
        assert_eq!(code::for_status(408), code::REQUEST_TIMEOUT);
        assert_eq!(code::for_status(414), code::URI_TOO_LONG);
        assert_eq!(code::for_status(416), code::RANGE_NOT_SATISFIABLE);
        assert_eq!(code::for_status(429), code::TOO_MANY_REQUESTS);
        assert_eq!(code::for_status(431), code::HEADERS_TOO_LARGE);
    }

    #[test]
    fn server_side_statuses_stay_internal() {
        // Guessing `archive_unavailable` from a 503 would invent detail the
        // response never carried: an intermediary's 503 says nothing about
        // which cairn-specific failure occurred.
        for status in [500, 502, 503, 504] {
            assert_eq!(code::for_status(status), code::INTERNAL, "status {status}");
        }
        assert_ne!(code::for_status(503), code::ARCHIVE_UNAVAILABLE);
    }

    #[test]
    fn unmapped_statuses_fall_back_to_internal() {
        for status in [0, 200, 302, 418, 999] {
            assert_eq!(code::for_status(status), code::INTERNAL, "status {status}");
        }
    }

    fn api(status: u16, code: &str) -> Error {
        Error::Api {
            status,
            code: code.to_string(),
            message: "boom".to_string(),
        }
    }

    #[test]
    fn predicates_match_only_their_own_code() {
        assert!(api(404, code::NOT_FOUND).is_not_found());
        assert!(!api(404, code::NOT_FOUND).is_unauthorized());
        assert!(api(401, code::UNAUTHORIZED).is_unauthorized());
        assert!(!api(401, code::UNAUTHORIZED).is_not_found());
        assert!(!Error::Transport("refused".into()).is_not_found());
        assert!(!Error::Invalid("bad".into()).is_unauthorized());
    }

    #[test]
    fn only_response_errors_carry_a_status() {
        assert_eq!(api(503, code::INTERNAL).status(), Some(503));
        assert_eq!(Error::Transport("refused".into()).status(), None);
        assert_eq!(Error::Invalid("bad".into()).status(), None);
    }

    #[test]
    fn display_names_the_failing_layer() {
        assert_eq!(
            api(404, code::NOT_FOUND).to_string(),
            "cairn returned 404 (not_found): boom"
        );
        assert_eq!(
            Error::Transport("refused".into()).to_string(),
            "connection to cairn failed: refused"
        );
        assert_eq!(
            Error::Invalid("bad JSON".into()).to_string(),
            "invalid response from cairn: bad JSON"
        );
    }
}
