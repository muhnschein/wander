use crate::error::{Error, code};
use crate::model::{
    ArchiveDetail, ArchiveSummary, ArchivesResponse, ErrorBody, RandomResponse, Status, Suggestion,
    SuggestionsResponse,
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::de::DeserializeOwned;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use ureq::Agent;

const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
// Every request occupies a thread from the GTK blocking pool for its whole
// life, so a generous global timeout is not free: a stalled daemon would pin
// those threads and freeze the reader. Two minutes is long enough for a large
// entry over a slow link and short enough that a dead server surfaces as an
// error rather than a hang.
const DEFAULT_GLOBAL_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct CairnClient {
    base_url: String,
    token: Option<Arc<str>>,
    agent: Agent,
}

#[derive(Debug)]
pub struct Entry {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub archive: String,
    pub path: String,
}

#[derive(Debug)]
pub struct EntryMeta {
    pub content_type: String,
    pub length: Option<u64>,
    pub archive: String,
    pub path: String,
}

impl CairnClient {
    pub fn new(host: &str, port: u16, token: Option<&str>) -> Result<Self, Error> {
        let authority = host_authority(host.trim())?;
        Ok(Self::with_base_url(
            &format!("http://{authority}:{port}"),
            token,
        ))
    }

    pub fn with_base_url(base_url: &str, token: Option<&str>) -> Self {
        let agent: Agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(DEFAULT_GLOBAL_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build()
            .new_agent();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            agent,
            token: token
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(Arc::from),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn status(&self) -> Result<Status, Error> {
        self.get_json(&format!("{}/v1/status", self.base_url))
    }

    pub fn archives(&self) -> Result<Vec<ArchiveSummary>, Error> {
        let resp: ArchivesResponse = self.get_json(&format!("{}/v1/archives", self.base_url))?;
        Ok(resp.archives)
    }

    pub fn archive(&self, uuid: &str) -> Result<ArchiveDetail, Error> {
        let uuid = canonical_uuid(uuid)?;
        self.get_json(&format!("{}/v1/archives/{uuid}", self.base_url))
    }

    pub fn entry(&self, uuid: &str, path: &str) -> Result<Entry, Error> {
        let uuid = canonical_uuid(uuid)?;
        let url = format!(
            "{}/v1/archives/{uuid}/entry/{}",
            self.base_url,
            utf8_percent_encode(path, PATH_SET)
        );
        let resp = self.request("GET", &url)?;
        let status = resp.status().as_u16();
        let content_type = header_str(&resp, "Content-Type")
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let path_header = header_str(&resp, "X-Cairn-Path").unwrap_or_else(|| path.to_string());
        if !resp.status().is_success() {
            return Err(self.api_error(status, resp));
        }
        let bytes = resp
            .into_body()
            .with_config()
            .limit(MAX_ENTRY_BYTES)
            .read_to_vec()
            .map_err(|e| Error::Transport(format!("reading entry body failed: {e}")))?;
        Ok(Entry {
            bytes,
            content_type,
            archive: uuid.to_string(),
            path: path_header,
        })
    }

    pub fn entry_meta(&self, uuid: &str, path: &str) -> Result<EntryMeta, Error> {
        let uuid = canonical_uuid(uuid)?;
        let url = format!(
            "{}/v1/archives/{uuid}/entry/{}",
            self.base_url,
            utf8_percent_encode(path, PATH_SET)
        );
        let resp = self.request("HEAD", &url)?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            return Err(self.api_error(status, resp));
        }
        Ok(EntryMeta {
            content_type: header_str(&resp, "Content-Type")
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            length: header_str(&resp, "Content-Length").and_then(|v| v.parse().ok()),
            archive: uuid.to_string(),
            path: header_str(&resp, "X-Cairn-Path").unwrap_or_else(|| path.to_string()),
        })
    }

    pub fn random(&self, uuid: &str) -> Result<String, Error> {
        let uuid = canonical_uuid(uuid)?;
        let resp: RandomResponse =
            self.get_json(&format!("{}/v1/archives/{uuid}/random", self.base_url))?;
        Ok(resp.path)
    }

    pub fn suggest(&self, uuid: &str, query: &str, limit: u32) -> Result<Vec<Suggestion>, Error> {
        let uuid = canonical_uuid(uuid)?;
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!(
            "{}/v1/archives/{uuid}/suggest?q={}&limit={}",
            self.base_url,
            utf8_percent_encode(query, PATH_SET),
            limit.clamp(1, 32)
        );
        let resp: SuggestionsResponse = self.get_json(&url)?;
        Ok(resp.suggestions)
    }

    fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, Error> {
        let resp = self.request("GET", url)?;
        let status = resp.status().as_u16();
        let text = resp
            .into_body()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_string()
            .map_err(|e| Error::Transport(format!("reading body failed: {e}")))?;
        if !(200..300).contains(&status) {
            return Err(api_error_from_body(status, &text));
        }
        serde_json::from_str(&text).map_err(|e| Error::Invalid(format!("bad JSON: {e}")))
    }

    fn request(&self, method: &str, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let builder = match method {
            "GET" => self.agent.get(url),
            "HEAD" => self.agent.head(url),
            _ => return Err(Error::Invalid(format!("unsupported method {method}"))),
        };
        let builder = match &self.token {
            Some(token) => builder.header("Authorization", &format!("Bearer {token}")),
            None => builder,
        };
        builder.call().map_err(|e| Error::Transport(e.to_string()))
    }

    fn api_error(&self, status: u16, resp: ureq::http::Response<ureq::Body>) -> Error {
        let text = resp
            .into_body()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_string()
            .unwrap_or_default();
        api_error_from_body(status, &text)
    }
}

fn api_error_from_body(status: u16, text: &str) -> Error {
    // A body that parses but carries no code is as useless as one that does not
    // parse at all, so both fall back to the status. Without this an nginx 404
    // in front of cairn would surface as `internal` and `is_not_found` would
    // answer false for a plain missing entry.
    match serde_json::from_str::<ErrorEnvelope>(text) {
        Ok(env) if !env.error.code.is_empty() => Error::Api {
            status,
            code: env.error.code,
            message: env.error.message,
        },
        _ => Error::Api {
            status,
            code: code::for_status(status).to_string(),
            message: "malformed error response".to_string(),
        },
    }
}

/// Render `host` as the authority component of a URL, bracketing IPv6 literals.
///
/// Rejects anything that would let the host smuggle further URL syntax into the
/// base address: userinfo (`@`) silently moves the real host to the far side of
/// the separator, and a path, query or fragment would reroute every request the
/// client builds on top of it.
fn host_authority(host: &str) -> Result<String, Error> {
    let host = match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some(inner) => inner,
        None => host,
    };
    if host.is_empty() {
        return Err(Error::Invalid("host must not be empty".into()));
    }
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Error::Invalid("host must not contain whitespace".into()));
    }
    if host.contains(['/', '\\', '@', '?', '#', '[', ']']) {
        return Err(Error::Invalid("host must be an IP address or name".into()));
    }
    // A colon is only legitimate in a bare IPv6 literal; anything else with one
    // is a host:port pair that belongs in the separate `port` argument.
    if host.contains(':') {
        return match host.parse::<Ipv6Addr>() {
            Ok(addr) => Ok(format!("[{addr}]")),
            Err(_) => Err(Error::Invalid(
                "host must not include a port; pass the port separately".into(),
            )),
        };
    }
    Ok(host.to_string())
}

#[derive(serde::Deserialize)]
struct ErrorEnvelope {
    #[serde(default = "missing_error")]
    error: ErrorBody,
}

fn missing_error() -> ErrorBody {
    ErrorBody {
        code: String::new(),
        message: String::new(),
    }
}

fn header_str(resp: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn canonical_uuid(uuid: &str) -> Result<&str, Error> {
    let b = uuid.as_bytes();
    let dashes = [8usize, 13, 18, 23];
    let hex = |c: u8| matches!(c, b'0'..=b'9' | b'a'..=b'f');
    let valid = b.len() == 36
        && dashes.iter().all(|&i| b[i] == b'-')
        && b.iter()
            .enumerate()
            .all(|(i, &c)| dashes.contains(&i) || hex(c));
    if valid {
        Ok(uuid)
    } else {
        Err(Error::Invalid(
            "uuid must be the canonical lowercase hyphenated form".into(),
        ))
    }
}
