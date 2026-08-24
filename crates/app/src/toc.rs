//! Heading extraction for the reader's table of contents.
//!
//! JavaScript is disabled in the reader, so the page cannot be asked what its
//! headings are. They are read out of the stored HTML instead, on the same
//! bytes the scheme handler has already fetched.
//!
//! This is a deliberately small scanner, not an HTML parser. It looks for
//! `h1`–`h6` and nothing else, and the worst a mistake can do is put a wrong
//! row in a sidebar: the text it extracts is rendered as a plain GTK label,
//! never as markup, and nothing here decides what gets loaded.

/// One entry in a page's table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 for `<h1>` through 6 for `<h6>`.
    pub level: u8,
    pub text: String,
    /// Fragment to scroll to, when the heading or something inside it has an id.
    pub anchor: Option<String>,
}

/// Elements whose text is chrome rather than content. MediaWiki puts an
/// `[edit]` link inside the heading itself, which would otherwise land in the
/// middle of every title.
const CHROME_CLASSES: [&str; 4] = [
    "mw-editsection",
    "mw-jump-link",
    "noprint",
    "mw-headline-anchor",
];

/// Extract the headings of an HTML document, in document order.
pub fn headings(html: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if starts_with_ci(&html[i..], "<!--") {
            i = match html[i + 4..].find("-->") {
                Some(end) => i + 4 + end + 3,
                None => break,
            };
            continue;
        }
        // A heading inside <script> or <style> is source text, not content.
        if let Some(name) = opening_name(&html[i..], &["script", "style"]) {
            let Some(open_end) = tag_end(html, i) else {
                break;
            };
            i = skip_element(html, open_end, name);
            continue;
        }
        if let Some(level) = heading_level(&html[i..]) {
            let Some(open_end) = tag_end(html, i) else {
                break;
            };
            let attrs = &html[i + 3..open_end.saturating_sub(1)];
            let close = format!("</h{level}");
            let Some(rel) = find_ci(&html[open_end..], &close) else {
                i = open_end;
                continue;
            };
            let inner = &html[open_end..open_end + rel];
            let text = text_of(inner);
            if !text.is_empty() {
                out.push(Heading {
                    level,
                    text,
                    // The heading's own id wins; MediaWiki instead puts it on an
                    // inner `<span class="mw-headline">`, so fall back to that.
                    anchor: attribute(attrs, "id").or_else(|| first_id(inner)),
                });
            }
            i = open_end + rel + close.len();
            continue;
        }
        i += 1;
    }
    out
}

/// `Some(level)` when `s` starts with an `h1`–`h6` opening tag.
fn heading_level(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() < 4 || b[0] != b'<' || !b[1].eq_ignore_ascii_case(&b'h') {
        return None;
    }
    let level = (b[2] as char).to_digit(10)? as u8;
    if !(1..=6).contains(&level) {
        return None;
    }
    // Reject `<h2x>`: the name has to end here.
    matches!(b[3], b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r').then_some(level)
}

/// The matched name when `s` starts with an opening tag from `names`.
fn opening_name<'a>(s: &str, names: &[&'a str]) -> Option<&'a str> {
    let rest = s.strip_prefix('<')?;
    names.iter().copied().find(|name| {
        starts_with_ci(rest, name)
            && rest
                .as_bytes()
                .get(name.len())
                .is_some_and(|c| matches!(c, b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r'))
    })
}

/// Byte index just past the `>` closing the tag that starts at `start`.
///
/// Quoted attribute values are honoured so `<h2 title="a>b">` is not cut short.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let b = html.as_bytes();
    let mut quote: Option<u8> = None;
    for (offset, &c) in b.iter().enumerate().skip(start) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return Some(offset + 1),
            None => {}
        }
    }
    None
}

/// Byte index just past the close tag matching an element named `name` whose
/// content begins at `from`. Nested elements of the same name are counted.
fn skip_element(html: &str, from: usize, name: &str) -> usize {
    let open = format!("<{name}");
    let close = format!("</{name}");
    let mut depth = 1usize;
    let mut i = from;
    while i < html.len() {
        let next_open = find_ci(&html[i..], &open).map(|r| i + r);
        let next_close = find_ci(&html[i..], &close).map(|r| i + r);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i = o + open.len();
            }
            (_, Some(c)) => {
                depth -= 1;
                i = tag_end(html, c).unwrap_or(c + close.len());
                if depth == 0 {
                    return i;
                }
            }
            _ => break,
        }
    }
    html.len()
}

/// Visible text of a fragment: tags removed, chrome elements dropped whole,
/// entities decoded and whitespace collapsed.
fn text_of(inner: &str) -> String {
    let mut raw = String::new();
    let bytes = inner.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            raw.push_str(&inner[start..i]);
            continue;
        }
        if starts_with_ci(&inner[i..], "<!--") {
            i = match inner[i + 4..].find("-->") {
                Some(end) => i + 4 + end + 3,
                None => bytes.len(),
            };
            continue;
        }
        let Some(open_end) = tag_end(inner, i) else {
            break;
        };
        let tag = &inner[i..open_end];
        if let Some(name) = tag_name(tag) {
            let self_closing = tag.trim_end().ends_with("/>");
            if !tag.starts_with("</") && !self_closing && is_chrome(tag) {
                i = skip_element(inner, open_end, name);
                continue;
            }
        }
        i = open_end;
    }
    collapse_whitespace(&decode_entities(&raw))
}

/// Element name of a tag, opening or closing.
fn tag_name(tag: &str) -> Option<&str> {
    let rest = tag
        .strip_prefix("</")
        .or_else(|| tag.strip_prefix('<'))?
        .trim_start();
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

fn is_chrome(tag: &str) -> bool {
    let Some(close) = tag_name(tag).map(str::len) else {
        return false;
    };
    let attrs = &tag[1 + close..tag.len().saturating_sub(1)];
    attribute(attrs, "class").is_some_and(|class| {
        class
            .split_whitespace()
            .any(|c| CHROME_CLASSES.contains(&c))
    })
}

/// First `id` on any non-chrome element inside a fragment.
fn first_id(inner: &str) -> Option<String> {
    let mut i = 0;
    while let Some(rel) = inner[i..].find('<') {
        let start = i + rel;
        let end = tag_end(inner, start)?;
        let tag = &inner[start..end];
        if !tag.starts_with("</") && !is_chrome(tag) {
            let name_len = tag_name(tag).map(str::len).unwrap_or(0);
            let attrs = &tag[1 + name_len..tag.len().saturating_sub(1)];
            if let Some(id) = attribute(attrs, "id").filter(|id| !id.is_empty()) {
                return Some(id);
            }
        }
        i = end;
    }
    None
}

/// Value of `name` within a tag's attribute text.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < b.len() && !matches!(b[i], b'=' | b'/' | b'>') && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == key_start {
            // No progress on a stray `=` or `/`; step over it.
            i += 1;
            continue;
        }
        let key = &attrs[key_start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue; // valueless attribute
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
            let quote = b[i];
            i += 1;
            let start = i;
            while i < b.len() && b[i] != quote {
                i += 1;
            }
            let value = &attrs[start..i];
            i = (i + 1).min(b.len());
            value
        } else {
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                i += 1;
            }
            &attrs[start..i]
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(decode_entities(value));
        }
    }
    None
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() || c == '\u{a0}' {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

fn starts_with_ci(hay: &str, needle: &str) -> bool {
    hay.len() >= needle.len()
        && hay.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

/// Byte offset of the first case-insensitive match of an ASCII `needle`.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::{Heading, headings};

    fn h(level: u8, text: &str, anchor: Option<&str>) -> Heading {
        Heading {
            level,
            text: text.to_string(),
            anchor: anchor.map(str::to_string),
        }
    }

    #[test]
    fn reads_headings_in_document_order() {
        let html = "<h1>One</h1><p>x</p><h2>Two</h2><h3>Three</h3>";
        assert_eq!(
            headings(html),
            vec![h(1, "One", None), h(2, "Two", None), h(3, "Three", None)]
        );
    }

    #[test]
    fn every_level_is_recognised_and_nothing_else_is() {
        for level in 1..=6u8 {
            let html = format!("<h{level}>T</h{level}>");
            assert_eq!(headings(&html), vec![h(level, "T", None)]);
        }
        // h0/h7 are not headings, and neither is a tag that merely starts h2.
        assert!(headings("<h0>T</h0>").is_empty());
        assert!(headings("<h7>T</h7>").is_empty());
        assert!(headings("<h2x>T</h2x>").is_empty());
        assert!(headings("<header>T</header>").is_empty());
    }

    #[test]
    fn tag_case_does_not_matter() {
        assert_eq!(
            headings("<H2 ID=\"a\">Text</H2>"),
            vec![h(2, "Text", Some("a"))]
        );
    }

    #[test]
    fn an_id_on_the_heading_is_the_anchor() {
        assert_eq!(
            headings(r#"<h2 id="History">History</h2>"#),
            vec![h(2, "History", Some("History"))]
        );
    }

    #[test]
    fn a_mediawiki_headline_span_supplies_the_anchor() {
        // The older MediaWiki shape: the id lives on an inner span, not the h2.
        let html = r#"<h2><span class="mw-headline" id="Early_life">Early life</span></h2>"#;
        assert_eq!(headings(html), vec![h(2, "Early life", Some("Early_life"))]);
    }

    #[test]
    fn edit_links_are_not_part_of_the_title() {
        // Without dropping the chrome this reads "[edit] History".
        let html = r#"<h2><span class="mw-editsection">[<a href="/edit">edit</a>]</span><span class="mw-headline" id="History">History</span></h2>"#;
        assert_eq!(headings(html), vec![h(2, "History", Some("History"))]);
    }

    #[test]
    fn nested_markup_is_reduced_to_its_text() {
        let html = "<h2>A <i>very</i> <b>bold</b> claim</h2>";
        assert_eq!(headings(html), vec![h(2, "A very bold claim", None)]);
    }

    #[test]
    fn entities_are_decoded() {
        let html = "<h2>Arts &amp; Crafts &#8212; &lt;i&gt; &#x41;&nbsp;B</h2>";
        assert_eq!(headings(html), vec![h(2, "Arts & Crafts — <i> A B", None)]);
    }

    #[test]
    fn an_unknown_entity_is_left_alone() {
        assert_eq!(
            headings("<h2>a &notanentity; b</h2>")[0].text,
            "a &notanentity; b"
        );
        assert_eq!(headings("<h2>Q &amp A</h2>")[0].text, "Q &amp A");
    }

    #[test]
    fn a_quoted_attribute_may_contain_a_close_bracket() {
        // Truncating the tag at the first `>` would leak `b">Real` into the text.
        let html = r#"<h2 title="a>b" id="x">Real</h2>"#;
        assert_eq!(headings(html), vec![h(2, "Real", Some("x"))]);
    }

    #[test]
    fn unquoted_and_single_quoted_attributes_are_read() {
        assert_eq!(
            headings("<h2 id=plain>T</h2>"),
            vec![h(2, "T", Some("plain"))]
        );
        assert_eq!(headings("<h2 id='sq'>T</h2>"), vec![h(2, "T", Some("sq"))]);
    }

    #[test]
    fn valueless_attributes_do_not_derail_the_scan() {
        assert_eq!(
            headings("<h2 hidden id=\"x\">T</h2>"),
            vec![h(2, "T", Some("x"))]
        );
    }

    #[test]
    fn headings_inside_script_or_style_are_not_content() {
        let html = r#"<script>var s = "<h1>Not a heading</h1>";</script><h1>Real</h1>"#;
        assert_eq!(headings(html), vec![h(1, "Real", None)]);
        let html = "<style>/* <h3>nope</h3> */</style><h1>Real</h1>";
        assert_eq!(headings(html), vec![h(1, "Real", None)]);
    }

    #[test]
    fn commented_out_headings_are_ignored() {
        assert_eq!(
            headings("<!-- <h1>Draft</h1> --><h1>Real</h1>"),
            vec![h(1, "Real", None)]
        );
    }

    #[test]
    fn empty_headings_are_dropped() {
        assert!(headings("<h2></h2><h2>   </h2><h2><span></span></h2>").is_empty());
    }

    #[test]
    fn an_unclosed_heading_does_not_hang_or_panic() {
        assert!(headings("<h2>Dangling").is_empty());
        assert!(headings("<h2").is_empty());
        assert!(headings("<!-- unterminated").is_empty());
        assert!(headings("<script>unterminated").is_empty());
    }

    #[test]
    fn multibyte_text_survives_intact() {
        let html = "<h2 id=\"Wien\">Wien — Straße Ä 日本語</h2>";
        assert_eq!(
            headings(html),
            vec![h(2, "Wien — Straße Ä 日本語", Some("Wien"))]
        );
    }

    #[test]
    fn whitespace_in_titles_is_collapsed() {
        assert_eq!(
            headings("<h2>\n  A   \t B \n</h2>"),
            vec![h(2, "A B", None)]
        );
    }

    /// This scanner runs on untrusted archive markup and does its own index
    /// arithmetic, so the only acceptable behaviour on garbage is to return.
    #[test]
    fn arbitrary_input_neither_panics_nor_hangs() {
        // Deterministic pseudo-random fragments assembled from the characters
        // most likely to confuse a tag scanner.
        let alphabet: Vec<char> = "<>hH123456/ \t\n\"'=&;#!-abc\u{a0}Ä日".chars().collect();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let mut input = String::new();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = (seed % 120) as usize;
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                input.push(alphabet[(seed % alphabet.len() as u64) as usize]);
            }
            // Any outcome is fine; not returning is not.
            let _ = headings(&input);
        }
    }

    #[test]
    fn deeply_nested_chrome_terminates() {
        let html = format!(
            "<h2>{}Real{}</h2>",
            "<span class=\"noprint\">".repeat(200),
            "</span>".repeat(200)
        );
        // The chrome swallows its own content; what matters is that it returns.
        let _ = headings(&html);
    }

    #[test]
    fn a_realistic_article_yields_its_outline() {
        let html = r##"
            <html><head><title>Vienna</title></head><body>
            <h1 id="firstHeading">Vienna</h1>
            <p>Vienna is the capital of Austria.</p>
            <h2><span class="mw-editsection">[<a href="#">edit</a>]</span>
                <span class="mw-headline" id="History">History</span></h2>
            <p>...</p>
            <h3 id="Roman_era">Roman era</h3>
            <h2 id="Geography">Geography &amp; climate</h2>
            </body></html>"##;
        assert_eq!(
            headings(html),
            vec![
                h(1, "Vienna", Some("firstHeading")),
                h(2, "History", Some("History")),
                h(3, "Roman era", Some("Roman_era")),
                h(2, "Geography & climate", Some("Geography")),
            ]
        );
    }
}
