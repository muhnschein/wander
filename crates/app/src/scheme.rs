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

/// Extract the archive-relative entry path from a `cairn://{uuid}/{path}` URI.
///
/// The authority is ignored on purpose: every reader page registers its own
/// handler bound to a single archive, so a URI naming another archive can only
/// ever be resolved against this one. That keeps a page from reaching into a
/// sibling archive by editing a link.
///
/// A query or fragment is stripped rather than rejected. Archived markup is
/// full of `?printable=yes` and `#section` links, and the stored entry is the
/// same however they are decorated; failing the load instead would leave the
/// reader on a blank page. Splitting before percent-decoding means a literal
/// `%3F` inside a stored path survives as part of the path.
fn parse_target(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("cairn://")?;
    let raw_path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let raw_path = raw_path.split(['#', '?']).next().unwrap_or("");
    let decoded = percent_decode_str(raw_path).decode_utf8().ok()?;
    Some(decoded.into_owned())
}

async fn resolve(
    client: Arc<CairnClient>,
    uuid: String,
    main_page: Option<String>,
    uri: &str,
) -> Result<cairn_client::Entry, String> {
    let target = parse_target(uri).ok_or_else(|| format!("malformed {SCHEME} URI: {uri}"))?;
    let fetched = gio::spawn_blocking(move || {
        if target.is_empty() {
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
            client.entry(&uuid, &target)
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

#[cfg(test)]
mod tests {
    use super::parse_target;

    const UUID: &str = "b10db2a4-0aac-52db-fd17-c5f79f36ab96";

    #[test]
    fn root_uri_has_an_empty_path() {
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/")).as_deref(),
            Some("")
        );
        assert_eq!(
            parse_target(&format!("cairn://{UUID}")).as_deref(),
            Some("")
        );
    }

    #[test]
    fn nested_paths_keep_their_separators() {
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/A/Vienna/Ring.html")).as_deref(),
            Some("A/Vienna/Ring.html")
        );
    }

    #[test]
    fn percent_escapes_are_decoded() {
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/A/Wien%20%C3%84.html")).as_deref(),
            Some("A/Wien Ä.html")
        );
    }

    #[test]
    fn query_and_fragment_are_stripped_not_rejected() {
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/A/Ring.html?printable=yes")).as_deref(),
            Some("A/Ring.html")
        );
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/A/Ring.html#History")).as_deref(),
            Some("A/Ring.html")
        );
    }

    #[test]
    fn an_escaped_question_mark_stays_in_the_path() {
        assert_eq!(
            parse_target(&format!("cairn://{UUID}/A/What%3F.html")).as_deref(),
            Some("A/What?.html")
        );
    }

    #[test]
    fn foreign_schemes_are_refused() {
        assert!(parse_target("https://example.org/A/Ring.html").is_none());
        assert!(parse_target("file:///etc/passwd").is_none());
    }
}
