use cairn_client::{CairnClient, Error};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub is_head: bool,
}

impl Response {
    fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
            is_head: false,
        }
    }

    fn entry(content_type: &str, xpath: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), content_type.to_string()),
                ("X-Cairn-Path".into(), xpath.to_string()),
            ],
            body,
            is_head: false,
        }
    }

    fn head(content_type: &str, length: u64) -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), content_type.to_string()),
                ("Content-Length".into(), length.to_string()),
                ("X-Cairn-Path".into(), "Some/Entry".into()),
            ],
            body: Vec::new(),
            is_head: true,
        }
    }

    fn error(status: u16, code: &str) -> Self {
        Self::json(
            status,
            &format!(r#"{{"error":{{"code":"{code}","message":"no such resource"}}}}"#),
        )
    }
}

struct ServerHandle {
    addr: String,
}

fn serve<F>(handler: F) -> ServerHandle
where
    F: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    let handler = Arc::new(handler);
    let _keepalive = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let handler = handler.clone();
            std::thread::spawn(move || {
                let _ = handle_conn(stream, &handler);
            });
        }
    });
    std::thread::yield_now();
    ServerHandle { addr }
}

fn handle_conn<F>(mut stream: TcpStream, handler: &Arc<F>) -> std::io::Result<()>
where
    F: Fn(&Request) -> Response,
{
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_string();
    let target = first.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let req = Request {
        method,
        target,
        headers,
    };
    let resp = handler(&req);

    let mut out = format!("HTTP/1.1 {} {}\r\n", resp.status, reason(resp.status));
    let mut has_len = false;
    for (k, v) in &resp.headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_len = true;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_len {
        out.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
    }
    out.push_str("Connection: close\r\n\r\n");
    stream.write_all(out.as_bytes())?;
    if !resp.is_head {
        stream.write_all(&resp.body)?;
    }
    stream.flush()?;
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

const UUID: &str = "b10db2a4-0aac-52db-fd17-c5f79f36ab96";

fn client(addr: &str) -> CairnClient {
    let base = format!("http://{addr}");
    CairnClient::with_base_url(&base, None)
}

fn client_with_token(addr: &str, token: &str) -> CairnClient {
    let base = format!("http://{addr}");
    CairnClient::with_base_url(&base, Some(token))
}

const STATUS_JSON: &str = r#"{
  "version": "2026.08",
  "uptime_seconds": 3600,
  "listener": "tcp:127.0.0.1:8080",
  "archives": 2,
  "auth": null,
  "sandbox": {
    "required": true,
    "layers": [
      {"name": "no_new_privs", "state": "applied", "detail": null},
      {"name": "landlock", "state": "applied", "detail": "abi 5"},
      {"name": "seccomp", "state": "applied", "detail": "49 syscalls"}
    ]
  }
}"#;

const ARCHIVES_JSON: &str = r#"{"archives": [
  {"uuid": "b10db2a4-0aac-52db-fd17-c5f79f36ab96", "title": "Climate Change",
   "entry_count": 20317, "cluster_count": 389, "main_page": "index.html",
   "format_version": "6.3", "content_namespace": "C", "suggest": true}
]}"#;

const ARCHIVE_DETAIL_JSON: &str = r#"{
  "uuid": "b10db2a4-0aac-52db-fd17-c5f79f36ab96",
  "title": "Climate Change",
  "entry_count": 20317,
  "main_page": "index.html",
  "suggest": true,
  "metadata": {"Creator": "Wikipedia", "Language": "eng"},
  "binary_metadata": ["Illustration_48x48"]
}"#;

#[test]
fn status_parses_and_hits_the_right_path() {
    let server = serve(|req| {
        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/v1/status");
        assert!(req.header("host").is_some());
        Response::json(200, STATUS_JSON)
    });
    let status = client(&server.addr).status().expect("status");
    assert_eq!(status.version, "2026.08");
    assert_eq!(status.uptime_seconds, 3600);
    assert_eq!(status.sandbox.layers.len(), 3);
    assert_eq!(status.sandbox.layers[1].name, "landlock");
    assert_eq!(status.sandbox.layers[2].state, "applied");
}

#[test]
fn bearer_token_sent_only_when_configured() {
    let server = serve(|req| match req.header("authorization") {
        Some("Bearer sekrit") => Response::json(200, STATUS_JSON),
        _ => Response::error(401, "unauthorized"),
    });
    assert!(client(&server.addr).status().is_err());
    let status = client_with_token(&server.addr, "sekrit")
        .status()
        .expect("authed status");
    assert_eq!(status.archives, 2);
}

#[test]
fn archives_list_unwrapped() {
    let server = serve(|req| {
        assert_eq!(req.target, "/v1/archives");
        Response::json(200, ARCHIVES_JSON)
    });
    let list = client(&server.addr).archives().expect("archives");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].uuid, UUID);
    assert_eq!(list[0].title, "Climate Change");
    assert_eq!(list[0].entry_count, 20317);
    assert!(list[0].suggest);
}

#[test]
fn archive_detail_includes_metadata() {
    let server = serve(move |req| {
        assert_eq!(req.target, format!("/v1/archives/{UUID}"));
        Response::json(200, ARCHIVE_DETAIL_JSON)
    });
    let detail = client(&server.addr).archive(UUID).expect("detail");
    assert_eq!(detail.summary.title, "Climate Change");
    assert_eq!(
        detail.metadata.get("Language").map(String::as_str),
        Some("eng")
    );
    assert_eq!(detail.binary_metadata, vec!["Illustration_48x48"]);
}

#[test]
fn entry_bytes_and_headers() {
    let server = serve(|req| {
        assert_eq!(
            req.target,
            "/v1/archives/b10db2a4-0aac-52db-fd17-c5f79f36ab96/entry/Some_Entry"
        );
        Response::entry("text/html", "Some_Entry", b"<h1>hello</h1>".to_vec())
    });
    let entry = client(&server.addr)
        .entry(UUID, "Some_Entry")
        .expect("entry");
    assert_eq!(entry.bytes, b"<h1>hello</h1>");
    assert_eq!(entry.content_type, "text/html");
    assert_eq!(entry.path, "Some_Entry");
}

#[test]
fn entry_paths_are_percent_encoded_once() {
    let server = serve(|req| {
        assert_eq!(
            req.target,
            "/v1/archives/b10db2a4-0aac-52db-fd17-c5f79f36ab96/entry/Wien%2F%C3%84%20Stra%C3%9Fe~1.html"
        );
        Response::entry("text/html", "x", b"<p>ok</p>".to_vec())
    });
    let entry = client(&server.addr)
        .entry(UUID, "Wien/Ä Straße~1.html")
        .expect("entry");
    assert_eq!(entry.bytes, b"<p>ok</p>");
}

#[test]
fn head_entry_meta_without_body() {
    let server = serve(|req| {
        assert_eq!(req.method, "HEAD");
        Response::head("image/png", 123456)
    });
    let meta = client(&server.addr)
        .entry_meta(UUID, "img.png")
        .expect("meta");
    assert_eq!(meta.length, Some(123456));
    assert_eq!(meta.content_type, "image/png");
    assert_eq!(meta.path, "Some/Entry");
}

#[test]
fn suggest_encodes_query_and_parses() {
    let server = serve(|req| {
        assert!(
            req.target
                .starts_with(&format!("/v1/archives/{UUID}/suggest?"))
        );
        assert!(req.target.contains("q=Klimawandel%20in"));
        assert!(req.target.contains("limit=7"));
        Response::json(
            200,
            r#"{"archive":"b10db2a4-0aac-52db-fd17-c5f79f36ab96","suggestions":[
              {"title":"Klimawandel in Deutschland","path":"Klimawandel_in_Deutschland"}]}"#,
        )
    });
    let sug = client(&server.addr)
        .suggest(UUID, "Klimawandel in", 7)
        .expect("suggest");
    assert_eq!(sug.len(), 1);
    assert_eq!(sug[0].title, "Klimawandel in Deutschland");
    assert_eq!(sug[0].path, "Klimawandel_in_Deutschland");
}

#[test]
fn empty_suggest_query_is_local_noop() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c2 = counter.clone();
    let server = serve(move |_req| {
        c2.fetch_add(1, Ordering::SeqCst);
        panic!("server must not be contacted for an empty query")
    });
    let sug = client(&server.addr).suggest(UUID, "", 5).expect("noop");
    assert!(sug.is_empty());
    drop(server);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn random_returns_path() {
    let server = serve(|req| {
        assert_eq!(req.target, format!("/v1/archives/{UUID}/random"));
        Response::json(
            200,
            r#"{"archive":"b10db2a4-0aac-52db-fd17-c5f79f36ab96","path":"Rhinoceros"}"#,
        )
    });
    let path = client(&server.addr).random(UUID).expect("random");
    assert_eq!(path, "Rhinoceros");
}

#[test]
fn error_shape_maps_to_api_error() {
    let server = serve(|_| Response::error(404, "not_found"));
    let err = client(&server.addr)
        .entry(UUID, "Missing")
        .expect_err("must fail");
    match err {
        Error::Api {
            status,
            code,
            message,
        } => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
            assert_eq!(message, "no such resource");
            assert!(
                Error::Api {
                    status,
                    code,
                    message
                }
                .is_not_found()
            );
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[test]
fn unauthorized_is_recognizable() {
    let server = serve(|_| Response::error(401, "unauthorized"));
    let err = client(&server.addr).archives().expect_err("must fail");
    assert!(err.is_unauthorized(), "got {err}");
}

#[test]
fn non_canonical_uuid_rejected_before_any_request() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c2 = counter.clone();
    let server = serve(move |_| {
        c2.fetch_add(1, Ordering::SeqCst);
        Response::json(200, "{}")
    });
    let upper = UUID.to_uppercase();
    assert!(matches!(
        client(&server.addr).archive(&upper),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        client(&server.addr).archive("not-a-uuid"),
        Err(Error::Invalid(_))
    ));
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn malformed_error_body_still_yields_api_error() {
    let server = serve(|_| Response::json(503, "<html>gateway timeout</html>"));
    let err = client(&server.addr).archives().expect_err("must fail");
    match err {
        Error::Api { status, code, .. } => {
            assert_eq!(status, 503);
            assert_eq!(code, "internal");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}
