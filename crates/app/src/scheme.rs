use cairn_client::CairnClient;
use percent_encoding::percent_decode_str;
use std::sync::Arc;

pub const SCHEME: &str = "cairn";

pub fn install(
    context: &webkit::WebContext,
    client: Arc<CairnClient>,
    uuid: String,
    main_page: Option<String>,
) {
    context.register_uri_scheme(SCHEME, move |request: &webkit::URISchemeRequest| {
        let request = request.clone();
        let client = client.clone();
        let uuid = uuid.clone();
        let main_page = main_page.clone();
        glib::spawn_future_local(async move {
            let Some(uri) = request.uri() else {
                fail(&request, "scheme request has no URI");
                return;
            };
            match resolve(client, uuid, main_page, uri.as_str()).await {
                Ok(entry) => finish(request, entry),
                Err(message) => fail(&request, &message),
            }
        });
    });
}

struct Target {
    path: String,
}

fn parse_target(uri: &str) -> Option<Target> {
    let rest = uri.strip_prefix("cairn://")?;
    let raw_path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    if raw_path.contains('#') || raw_path.contains('?') {
        return None;
    }
    let decoded = percent_decode_str(raw_path).decode_utf8().ok()?;
    Some(Target {
        path: decoded.into_owned(),
    })
}

async fn resolve(
    client: Arc<CairnClient>,
    uuid: String,
    main_page: Option<String>,
    uri: &str,
) -> Result<cairn_client::Entry, String> {
    let target = parse_target(uri).ok_or_else(|| format!("malformed {SCHEME} URI: {uri}"))?;
    let fetched = gio::spawn_blocking(move || {
        if target.path.is_empty() {
            let entry_path = match main_page {
                Some(main) => main,
                None => client
                    .archive(&uuid)
                    .ok()
                    .and_then(|detail| detail.summary.main_page)
                    .unwrap_or_default(),
            };
            client.entry(&uuid, &entry_path)
        } else {
            client.entry(&uuid, &target.path)
        }
        .map_err(|e| e.to_string())
    })
    .await;
    match fetched {
        Ok(result) => result,
        Err(_) => Err("background task failed".to_string()),
    }
}

fn finish(request: webkit::URISchemeRequest, entry: cairn_client::Entry) {
    let mime = entry
        .content_type
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .trim()
        .to_string();
    let bytes = glib::Bytes::from_owned(entry.bytes);
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    request.finish(&stream, bytes.len() as i64, Some(&mime));
}

fn fail(request: &webkit::URISchemeRequest, message: &str) {
    let mut error = glib::Error::new(glib::FileError::Failed, message);
    request.finish_error(&mut error);
}
