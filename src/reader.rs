use dom_query::{Document, NodeRef};
use url::Url;

use crate::error::Error;

const NAME_LIMIT: usize = 40;

const TOKENS: [&str; 3] = ["reader", "readmode", "reading-view"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderRule {
    Rel,
    Path,
    Name,
}

impl ReaderRule {
    pub fn as_str(self) -> &'static str {
        match self {
            ReaderRule::Rel => "rel",
            ReaderRule::Path => "path",
            ReaderRule::Name => "name",
        }
    }

    fn rank(self) -> u8 {
        match self {
            ReaderRule::Rel => 3,
            ReaderRule::Path => 2,
            ReaderRule::Name => 1,
        }
    }

    fn stronger(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderHit {
    pub url: Url,
    pub rule: ReaderRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderScan {
    None,
    One(ReaderHit),
    Several,
}

impl ReaderScan {
    pub fn verbose_line(&self) -> String {
        match self {
            ReaderScan::None => "reader view: kept the article page (no reader link)".to_string(),
            ReaderScan::Several => {
                "reader view: kept the article page (several reader links)".to_string()
            }
            ReaderScan::One(hit) => format!("reader view: {} ({})", hit.url, hit.rule.as_str()),
        }
    }
}

pub fn scan(doc: &Document, base: &Url) -> ReaderScan {
    let mut hits: Vec<ReaderHit> = Vec::new();
    let nodes = doc.select("a[href], link[href]").nodes().to_vec();
    for node in nodes {
        let Some(hit) = link_hit(&node, base) else {
            continue;
        };
        if let Some(existing) = hits
            .iter_mut()
            .find(|have| same_absolute(&have.url, &hit.url))
        {
            existing.rule = existing.rule.stronger(hit.rule);
            continue;
        }
        hits.push(hit);
        if hits.len() > 1 {
            return ReaderScan::Several;
        }
    }
    match hits.pop() {
        Some(hit) => ReaderScan::One(hit),
        None => ReaderScan::None,
    }
}

/// Short status for `warning: reader view failed (<url>, <status>)`.
/// `None` means the error is not a reader-view failure and must propagate.
pub fn failure_status(err: &Error) -> Option<String> {
    match err {
        Error::NoArticle { .. } => Some("no article text".to_string()),
        Error::ArticleTooLarge(_) | Error::TooLarge(_) => Some("too large".to_string()),
        Error::Fetch { reason, .. } => Some(fetch_reason_status(reason)),
        _ => None,
    }
}

fn fetch_reason_status(reason: &str) -> String {
    if let Some(rest) = reason.strip_prefix("HTTP ") {
        let code = rest.split_whitespace().next().unwrap_or("");
        if !code.is_empty() && code.bytes().all(|byte| byte.is_ascii_digit()) {
            return code.to_string();
        }
    }
    let lower = reason.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        return "timeout".to_string();
    }
    if lower.contains("too many redirects") {
        return "too many redirects".to_string();
    }
    if lower.contains("connect")
        || lower.contains("connection")
        || lower.contains("reset")
        || lower.contains("closed")
        || lower.contains("broken pipe")
    {
        return "connection failed".to_string();
    }
    "fetch failed".to_string()
}

fn link_hit(node: &NodeRef<'_>, base: &Url) -> Option<ReaderHit> {
    let href = attr(node, "href")?;
    if fragment_only(&href) {
        return None;
    }
    let mut url = base.join(href.trim()).ok()?;
    url.set_fragment(None);
    if !is_http(&url) || !same_origin(&url, base) {
        return None;
    }
    let name = accessible_name(node);
    if name_rejected(&name) {
        return None;
    }
    let rule = if rel_matches(&attr(node, "rel").unwrap_or_default()) {
        ReaderRule::Rel
    } else if path_matches(&url) {
        ReaderRule::Path
    } else if name_matches(&name) {
        ReaderRule::Name
    } else {
        return None;
    };
    Some(ReaderHit { url, rule })
}

fn fragment_only(href: &str) -> bool {
    let href = href.trim();
    href.is_empty() || href.starts_with('#')
}

fn is_http(url: &Url) -> bool {
    url.scheme() == "http" || url.scheme() == "https"
}

fn same_origin(url: &Url, base: &Url) -> bool {
    is_http(url) && is_http(base) && url.origin() == base.origin()
}

fn same_absolute(left: &Url, right: &Url) -> bool {
    url_key(left) == url_key(right)
}

fn url_key(url: &Url) -> String {
    let query = url
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    format!(
        "{}{}{query}",
        url.origin().ascii_serialization(),
        url.path()
    )
}

fn rel_matches(rel: &str) -> bool {
    rel.split_whitespace().any(is_token)
}

fn path_matches(url: &Url) -> bool {
    url.path().split('/').any(is_token)
}

fn is_token(token: &str) -> bool {
    TOKENS
        .iter()
        .any(|candidate| token.eq_ignore_ascii_case(candidate))
}

fn accessible_name(node: &NodeRef<'_>) -> String {
    if let Some(label) = attr(node, "aria-label") {
        let label = label.trim();
        if !label.is_empty() {
            return label.to_string();
        }
    }
    node.text().trim().to_string()
}

fn name_rejected(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.contains("screen reader") || folded.contains("e-reader")
}

fn name_matches(name: &str) -> bool {
    let name = name.trim();
    if name.chars().count() > NAME_LIMIT {
        return false;
    }
    if name.eq_ignore_ascii_case("leseansicht") || name.eq_ignore_ascii_case("lesemodus") {
        return true;
    }
    TOKENS.iter().any(|token| contains_word(name, token))
}

fn contains_word(name: &str, word: &str) -> bool {
    let name: Vec<char> = name.to_ascii_lowercase().chars().collect();
    let word: Vec<char> = word.to_ascii_lowercase().chars().collect();
    if word.is_empty() || name.len() < word.len() {
        return false;
    }
    let last = name.len() - word.len();
    for start in 0..=last {
        if name[start..start + word.len()] != word[..] {
            continue;
        }
        let before = start == 0 || !is_word_char(name[start - 1]);
        let end = start + word.len();
        let after = end == name.len() || !is_word_char(name[end]);
        if before && after {
            return true;
        }
    }
    false
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn attr(node: &NodeRef<'_>, name: &str) -> Option<String> {
    node.attr(name).map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_html(html: &str, base: &str) -> ReaderScan {
        let doc = Document::from(html);
        let base = Url::parse(base).unwrap();
        scan(&doc, &base)
    }

    fn one(html: &str, base: &str) -> (String, ReaderRule) {
        match scan_html(html, base) {
            ReaderScan::One(hit) => (hit.url.to_string(), hit.rule),
            other => panic!("expected one reader link, got {other:?}"),
        }
    }

    const BASE: &str = "http://example.com/news/article";

    #[test]
    fn rel_token_matches_and_amphtml_does_not() {
        assert!(matches!(
            scan_html(r#"<link rel="amphtml" href="/amp">"#, BASE),
            ReaderScan::None
        ));
        let (url, rule) = one(r#"<link rel="reader amphtml" href="/view">"#, BASE);
        assert_eq!(rule, ReaderRule::Rel);
        assert_eq!(url, "http://example.com/view");
        let (_, rule) = one(r#"<a rel="READMODE" href="/story">nope</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Rel);
        assert!(matches!(
            scan_html(r#"<a rel="readermode" href="/story">nope</a>"#, BASE),
            ReaderScan::None
        ));
    }

    #[test]
    fn path_segment_is_a_whole_token() {
        let (url, rule) = one(r#"<a href="/reading-view/article">open</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Path);
        assert_eq!(url, "http://example.com/reading-view/article");
        let (_, rule) = one(r#"<a href="/READMODE">open</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Path);
        assert!(matches!(
            scan_html(r#"<a href="/readers">archive</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="/imprint">imprint</a>"#, BASE),
            ReaderScan::None
        ));
    }

    #[test]
    fn accessible_name_is_a_whole_word_or_a_german_label() {
        let (_, rule) = one(r#"<a href="/story">Reader</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Name);
        let (_, rule) = one(r#"<a href="/story">leseMODUS</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Name);
        let (_, rule) = one(r#"<a href="/story">see the reading-view</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Name);
        assert!(matches!(
            scan_html(r#"<a href="/story">prereader tool</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="/story">Die Leseansicht</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="/story">reading-views</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="/story">Leseansicht!</a>"#, BASE),
            ReaderScan::None
        ));
    }

    #[test]
    fn name_limit_is_forty_characters() {
        let exact = format!(r#"<a href="/story">reader {}</a>"#, "x".repeat(33));
        assert_eq!(one(&exact, BASE).1, ReaderRule::Name);
        let over = format!(r#"<a href="/story">reader {}</a>"#, "x".repeat(34));
        assert!(matches!(scan_html(&over, BASE), ReaderScan::None));
    }

    #[test]
    fn aria_label_wins_and_screen_reader_names_are_rejected() {
        let (_, rule) = one(
            r#"<a href="/story" aria-label="Reader">screen reader weekly digest</a>"#,
            BASE,
        );
        assert_eq!(rule, ReaderRule::Name);
        assert!(matches!(
            scan_html(
                r#"<a rel="reader" href="/reader" aria-label="screen reader">Reader</a>"#,
                BASE
            ),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(
                r#"<a rel="reader" href="/reader" aria-label="this name mentions a screen reader and is quite long indeed">x</a>"#,
                BASE
            ),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(
                r#"<a rel="reader" href="/reader">buy an e-reader</a>"#,
                BASE
            ),
            ReaderScan::None
        ));
        let (_, rule) = one(r#"<a href="/story" aria-label="   ">Leseansicht</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Name);
    }

    #[test]
    fn fragment_only_and_non_http_are_not_candidates() {
        assert!(matches!(
            scan_html(
                r##"<a href="#top">Reader</a>"##,
                "http://example.com/reader/article"
            ),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(
                r#"<a href="">Reader</a>"#,
                "http://example.com/reader/article"
            ),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="   ">Reader</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="javascript:reader">Reader</a>"#, BASE),
            ReaderScan::None
        ));
        let (url, rule) = one(r#"<a href="/reader#top">open</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Path);
        assert_eq!(url, "http://example.com/reader");
    }

    #[test]
    fn origin_is_scheme_host_and_port() {
        assert!(matches!(
            scan_html(r#"<a href="https://other.example/reader">Reader</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(r#"<a href="https://example.com/reader">Reader</a>"#, BASE),
            ReaderScan::None
        ));
        assert!(matches!(
            scan_html(
                r#"<a href="http://example.com/reader">Reader</a>"#,
                "http://example.com:8080/a"
            ),
            ReaderScan::None
        ));
        let (url, _) = one(
            r#"<a href="http://example.com:8080/reader">open</a>"#,
            "http://example.com:8080/a",
        );
        assert_eq!(url, "http://example.com:8080/reader");
    }

    #[test]
    fn one_absolute_url_is_one_match_and_two_urls_are_several() {
        let (url, rule) = one(
            r#"<a href="/story">Reader</a><a rel="readmode" href="http://example.com:80/story">nope</a>"#,
            BASE,
        );
        assert_eq!(rule, ReaderRule::Rel);
        assert_eq!(url, "http://example.com/story");
        assert!(matches!(
            scan_html(r#"<a href="/a">Reader</a><a href="/b">Lesemodus</a>"#, BASE),
            ReaderScan::Several
        ));
        assert!(matches!(
            scan_html("<p>No links here</p>", BASE),
            ReaderScan::None
        ));
    }

    #[test]
    fn rel_beats_path_and_base_href_sets_the_origin() {
        let (_, rule) = one(r#"<a rel="reader" href="/reader">Reader</a>"#, BASE);
        assert_eq!(rule, ReaderRule::Rel);
        let html = r#"<base href="https://cdn.example/news/"><a href="reader">open</a>"#;
        let doc = Document::from(html);
        let response = Url::parse("http://typed.example/page").unwrap();
        let base = crate::meta::document_base(&doc, &response);
        match scan(&doc, &base) {
            ReaderScan::One(hit) => {
                assert_eq!(hit.rule, ReaderRule::Path);
                assert_eq!(hit.url.as_str(), "https://cdn.example/news/reader");
            }
            other => panic!("expected the base-href reader link, got {other:?}"),
        }
    }

    #[test]
    fn failure_status_uses_http_code_or_a_short_label() {
        let http = Error::Fetch {
            url: "http://example.test/reader".to_string(),
            reason: "HTTP 404 Not Found".to_string(),
        };
        assert_eq!(failure_status(&http).as_deref(), Some("404"));
        let transport = Error::Fetch {
            url: "http://example.test/reader".to_string(),
            reason: "error sending request: connection refused".to_string(),
        };
        assert_eq!(
            failure_status(&transport).as_deref(),
            Some("connection failed")
        );
        let missing = Error::NoArticle {
            url: "http://example.test/reader".to_string(),
        };
        assert_eq!(failure_status(&missing).as_deref(), Some("no article text"));
        assert_eq!(failure_status(&Error::BadSlug), None);
    }
}
