use cairn_client::CairnClient;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use std::sync::Arc;

pub const SCHEME: &str = "cairn";

/// Percent-encoding for the path portion of a `cairn://` URI.
///
/// Unlike the encoding the HTTP client applies, `/` is left alone. cairn takes
/// an entry path as a single opaque segment, but a `cairn://` URI is a real URI
/// that WebKit resolves relative links against: keeping the separators means a
/// link to `Graben.html` inside `A/Vienna/Ring.html` resolves to
/// `A/Vienna/Graben.html`, as the archive author intended. Encoding it to `%2F`
/// would flatten every entry into the archive root and send each relative link
/// to a path that does not exist.
const URI_PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// Build the URI that addresses `path` within `uuid`.
///
/// Kept beside [`parse_target`] on purpose: the two are one format, and a
/// change to either that the other does not match is exactly the defect the
/// round-trip tests below exist to catch.
pub fn entry_uri(uuid: &str, path: &str) -> String {
    format!(
        "{SCHEME}://{uuid}/{}",
        utf8_percent_encode(path, URI_PATH_SET)
    )
}

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

    /// The property the reader actually depends on: a relative link inside an
    /// entry must resolve to a sibling of that entry.
    ///
    /// An encode-then-decode round trip cannot see this. Escaping `/` to `%2F`
    /// survives that trip perfectly intact while still destroying relative
    /// resolution, because a resolver then treats the whole path as one
    /// segment. Resolution is checked against glib's RFC 3986 implementation
    /// rather than a hand-rolled model of what WebKit is assumed to do.
    #[test]
    fn relative_links_resolve_to_siblings_of_the_entry() {
        let base = super::entry_uri(UUID, "A/Vienna/Ring.html");
        for (link, expected) in [
            ("Graben.html", "A/Vienna/Graben.html"),
            ("./Graben.html", "A/Vienna/Graben.html"),
            ("../Salzburg/Dom.html", "A/Salzburg/Dom.html"),
            ("/A/Wien.html", "A/Wien.html"),
            ("Ring.html#History", "A/Vienna/Ring.html"),
        ] {
            let resolved = glib::Uri::resolve_relative(Some(&base), link, glib::UriFlags::NONE)
                .expect("glib resolves the reference");
            assert_eq!(
                parse_target(&resolved).as_deref(),
                Some(expected),
                "{link:?} against {base:?} resolved to {resolved:?}"
            );
        }
    }

    /// Guards encode/decode symmetry only. Deliberately weaker than the test
    /// above, and on its own it would not notice a change that breaks relative
    /// links — both directions would simply agree on the wrong thing.
    #[test]
    fn built_uris_parse_back_to_the_same_path() {
        for path in [
            "index.html",
            "A/Vienna/Ring.html",
            "A/Wien Ä.html",
            "A/Arts & Crafts.html",
            "A/What?.html",
            "A/100% Cotton.html",
            "A/a#b.html",
            "A/Ünïcödé/Straße~1.html",
            "A/plus+sign.html",
            "",
        ] {
            let uri = super::entry_uri(UUID, path);
            assert_eq!(
                parse_target(&uri).as_deref(),
                Some(path),
                "round trip failed for {path:?} (uri was {uri:?})"
            );
        }
    }

    #[test]
    fn separators_survive_encoding_so_relative_links_resolve() {
        // The `/` must stay literal: WebKit resolves a relative link against
        // the last path segment, so `%2F` would flatten the entry into the
        // archive root and break every relative link on the page.
        let uri = super::entry_uri(UUID, "A/Vienna/Ring.html");
        assert_eq!(uri, format!("cairn://{UUID}/A/Vienna/Ring.html"));
        assert!(!uri.contains("%2F"));
    }

    #[test]
    fn characters_that_would_change_the_uri_shape_are_escaped() {
        // Left literal these would truncate the path at the parser.
        assert!(super::entry_uri(UUID, "A/What?.html").contains("%3F"));
        assert!(super::entry_uri(UUID, "A/a#b.html").contains("%23"));
    }

    #[test]
    fn foreign_schemes_are_refused() {
        assert!(parse_target("https://example.org/A/Ring.html").is_none());
        assert!(parse_target("file:///etc/passwd").is_none());
    }
}
