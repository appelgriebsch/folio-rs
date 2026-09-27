use dom_query::{Document, NodeRef};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMeta {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub language: String,
    pub identifier: String,
    pub published: Option<String>,
    /// Absolute http(s) URL of the page's share banner, when the page names one.
    pub banner: Option<String>,
    pub base: Url,
}

#[cfg(test)]
pub fn read_meta(html: &str, response_url: &Url, typed_url: &Url) -> PageMeta {
    let doc = Document::from(html);
    read_meta_doc(&doc, response_url, typed_url)
}

pub fn read_meta_doc(doc: &Document, response_url: &Url, typed_url: &Url) -> PageMeta {
    let ld = json_ld_nodes(doc);
    let metas = meta_tags(doc);
    let title = first_nonempty([
        ld.iter()
            .find(|n| n.article)
            .and_then(|n| n.headline.clone()),
        meta_content(&metas, &["og:title"]),
        meta_content(&metas, &["citation_title"]),
        meta_content(
            &metas,
            &["dc.title", "dcterms.title", "dc:title", "dcterms:title"],
        ),
        ld.iter().find_map(|n| n.headline.clone()),
        element_text(doc, "title"),
    ]);
    let mut authors = ld
        .iter()
        .find(|n| n.article && !n.authors.is_empty())
        .or_else(|| ld.iter().find(|n| !n.authors.is_empty()))
        .map(|n| n.authors.clone())
        .unwrap_or_default();
    if authors.is_empty() {
        authors = meta_contents(
            &metas,
            &[
                "author",
                "citation_author",
                "dc.creator",
                "dcterms.creator",
                "dc:creator",
                "dcterms:creator",
            ],
        );
    }
    let description = first_nonempty([
        ld.iter()
            .find(|n| n.article)
            .and_then(|n| n.description.clone()),
        meta_content(&metas, &["og:description"]),
        meta_content(&metas, &["description"]),
        meta_content(
            &metas,
            &[
                "dc.description",
                "dcterms.description",
                "dc:description",
                "dcterms:description",
            ],
        ),
        ld.iter().find_map(|n| n.description.clone()),
    ]);
    let language = first_nonempty([
        html_lang(doc).and_then(|s| normalize_lang(&s)),
        meta_http_equiv(&metas, "content-language").and_then(|s| normalize_lang(&s)),
        meta_content(&metas, &["og:locale"]).and_then(|s| normalize_lang(&s)),
        ld.iter()
            .find_map(|n| n.language.clone())
            .and_then(|s| normalize_lang(&s)),
    ])
    .unwrap_or_else(|| "und".to_string());
    let published = first_nonempty([
        ld.iter()
            .find(|n| n.article)
            .and_then(|n| n.published.clone()),
        ld.iter().find_map(|n| n.published.clone()),
        meta_content(&metas, &["article:published_time"]),
        meta_content(&metas, &["citation_publication_date"]),
        meta_content(
            &metas,
            &["dc.issued", "dcterms.issued", "dc:issued", "dcterms:issued"],
        ),
    ]);
    let base = document_base(doc, response_url);
    let banner = first_nonempty([
        meta_content(&metas, &["og:image"]).and_then(|raw| absolute_http(&base, &raw)),
        meta_content(&metas, &["twitter:image", "twitter:image:src"])
            .and_then(|raw| absolute_http(&base, &raw)),
        ld.iter()
            .find(|n| n.article)
            .and_then(|n| n.image.clone())
            .and_then(|raw| absolute_http(&base, &raw)),
        ld.iter()
            .find_map(|n| n.image.clone())
            .and_then(|raw| absolute_http(&base, &raw)),
    ]);
    PageMeta {
        title,
        authors,
        description,
        language,
        identifier: canonical_identifier(doc).unwrap_or_else(|| typed_url.as_str().to_string()),
        published,
        banner,
        base,
    }
}

fn absolute_http(base: &Url, raw: &str) -> Option<String> {
    let joined = base.join(raw.trim()).ok()?;
    if joined.scheme() == "http" || joined.scheme() == "https" {
        Some(joined.to_string())
    } else {
        None
    }
}

pub fn document_base(doc: &Document, response_url: &Url) -> Url {
    for base in selected(doc, "base") {
        let Some(href) = attr(&base, "href") else {
            continue;
        };
        let Ok(joined) = response_url.join(href.trim()) else {
            continue;
        };
        if joined.scheme() == "http" || joined.scheme() == "https" {
            return joined;
        }
    }
    response_url.clone()
}

fn canonical_identifier(doc: &Document) -> Option<String> {
    for link in selected(doc, "link") {
        let rel = attr(&link, "rel").unwrap_or_default();
        if !rel
            .split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("canonical"))
        {
            continue;
        }
        let href = attr(&link, "href")?;
        let Ok(url) = Url::parse(href.trim()) else {
            continue;
        };
        if url.scheme() == "http" || url.scheme() == "https" {
            return Some(url.to_string());
        }
    }
    None
}

#[derive(Default)]
struct LdNode {
    article: bool,
    headline: Option<String>,
    published: Option<String>,
    authors: Vec<String>,
    description: Option<String>,
    language: Option<String>,
    image: Option<String>,
}

fn json_ld_nodes(doc: &Document) -> Vec<LdNode> {
    let mut nodes = Vec::new();
    for script in selected(doc, "script") {
        let kind = attr(&script, "type").unwrap_or_default();
        let kind = kind.split(';').next().unwrap_or("").trim();
        if !kind.eq_ignore_ascii_case("application/ld+json") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&script.text()) else {
            continue;
        };
        collect_ld(&value, &mut nodes);
    }
    nodes
}

fn collect_ld(value: &Value, out: &mut Vec<LdNode>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_ld(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(graph) = map.get("@graph") {
                collect_ld(graph, out);
            }
            let mut node = LdNode {
                article: is_article_value(map.get("@type")),
                headline: map.get("headline").and_then(json_text),
                published: map.get("datePublished").and_then(json_text),
                authors: Vec::new(),
                description: map.get("description").and_then(json_text),
                language: map.get("inLanguage").and_then(json_text),
                image: json_image(map.get("image")),
            };
            if let Some(author) = map.get("author") {
                push_authors(author, &mut node.authors);
            }
            if let Some(creator) = map.get("creator") {
                push_authors(creator, &mut node.authors);
            }
            if node.headline.is_some()
                || node.published.is_some()
                || node.description.is_some()
                || node.language.is_some()
                || node.image.is_some()
                || !node.authors.is_empty()
                || node.article
            {
                out.push(node);
            }
            for (key, child) in map {
                if key == "@graph" || key == "author" || key == "creator" {
                    continue;
                }
                if child.is_object() || child.is_array() {
                    collect_ld(child, out);
                }
            }
        }
        _ => {}
    }
}

fn is_article_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => is_article_type(text),
        Some(Value::Array(items)) => items
            .iter()
            .any(|item| item.as_str().is_some_and(is_article_type)),
        _ => false,
    }
}

fn is_article_type(raw: &str) -> bool {
    let name = raw.rsplit(['/', '#']).next().unwrap_or(raw);
    matches!(
        name,
        "Article"
            | "NewsArticle"
            | "BlogPosting"
            | "LiveBlogPosting"
            | "DiscussionForumPosting"
            | "SocialMediaPosting"
            | "ScholarlyArticle"
            | "MedicalScholarlyArticle"
            | "TechArticle"
            | "APIReference"
            | "Report"
            | "SatiricalArticle"
            | "AdvertiserContentArticle"
            | "AnalysisNewsArticle"
            | "AskPublicNewsArticle"
            | "BackgroundNewsArticle"
            | "OpinionNewsArticle"
            | "ReportageNewsArticle"
            | "ReviewNewsArticle"
    ) || name.ends_with("Article")
        || name.ends_with("Posting")
}

fn json_image(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => nonempty(text),
        Value::Array(items) => items.iter().find_map(|item| json_image(Some(item))),
        Value::Object(map) => map
            .get("url")
            .and_then(json_text)
            .or_else(|| map.get("contentUrl").and_then(json_text)),
        _ => None,
    }
}

fn push_authors(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => push_clean(out, text),
        Value::Array(items) => {
            for item in items {
                push_authors(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(name) = map.get("name").and_then(json_text) {
                push_clean(out, &name);
            }
        }
        _ => {}
    }
}

fn json_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => nonempty(text),
        Value::Object(map) => map
            .get("@value")
            .or_else(|| map.get("name"))
            .or_else(|| map.get("alternateName"))
            .and_then(json_text),
        Value::Array(items) => items.iter().find_map(json_text),
        _ => None,
    }
}

struct MetaTag {
    name: Option<String>,
    property: Option<String>,
    http_equiv: Option<String>,
    content: Option<String>,
}

fn meta_tags(doc: &Document) -> Vec<MetaTag> {
    selected(doc, "meta")
        .into_iter()
        .map(|node| MetaTag {
            name: attr(&node, "name").map(|s| s.to_ascii_lowercase()),
            property: attr(&node, "property").map(|s| s.to_ascii_lowercase()),
            http_equiv: attr(&node, "http-equiv").map(|s| s.to_ascii_lowercase()),
            content: attr(&node, "content").and_then(|s| nonempty(s.trim())),
        })
        .collect()
}

fn meta_content(tags: &[MetaTag], keys: &[&str]) -> Option<String> {
    tags.iter().find_map(|tag| {
        let name = tag.name.as_deref().unwrap_or("");
        let property = tag.property.as_deref().unwrap_or("");
        if keys.iter().any(|key| name == *key || property == *key) {
            tag.content.clone()
        } else {
            None
        }
    })
}

fn meta_contents(tags: &[MetaTag], keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for tag in tags {
        let name = tag.name.as_deref().unwrap_or("");
        let property = tag.property.as_deref().unwrap_or("");
        if keys.iter().any(|key| name == *key || property == *key) {
            if let Some(content) = &tag.content {
                push_clean(&mut out, content);
            }
        }
    }
    out
}

fn meta_http_equiv(tags: &[MetaTag], key: &str) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.http_equiv.as_deref() == Some(key) {
            tag.content.clone()
        } else {
            None
        }
    })
}

fn html_lang(doc: &Document) -> Option<String> {
    let html = selected(doc, "html");
    let node = html.first()?;
    attr(node, "lang").or_else(|| attr(node, "xml:lang"))
}

fn element_text(doc: &Document, selector: &str) -> Option<String> {
    let nodes = selected(doc, selector);
    let node = nodes.first()?;
    nonempty(node.text().trim())
}

pub fn normalize_lang(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .split([',', ' ', ';'])
        .next()
        .unwrap_or("")
        .trim()
        .replace('_', "-");
    let clean: String = token
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect();
    let clean = clean.trim_matches('-').to_string();
    if clean.is_empty() { None } else { Some(clean) }
}

fn first_nonempty<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}

fn nonempty(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn push_clean(out: &mut Vec<String>, text: &str) {
    if let Some(text) = nonempty(text) {
        if !out.iter().any(|have| have == &text) {
            out.push(text);
        }
    }
}

fn selected<'a>(doc: &'a Document, selector: &str) -> Vec<NodeRef<'a>> {
    doc.select(selector).nodes().to_vec()
}

fn attr(node: &NodeRef<'_>, name: &str) -> Option<String> {
    node.attr(name).map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::read_meta;

    fn urls() -> (Url, Url) {
        let response = Url::parse("http://response.example/final").unwrap();
        let typed = Url::parse("http://typed.example/original").unwrap();
        (response, typed)
    }

    #[test]
    fn publish_date_prefers_article_json_ld() {
        let html = r#"
            <html lang="de_DE"><head>
            <title>Document title</title>
            <meta property="og:title" content="OG Title">
            <script type="application/ld+json">
              {"@type":"WebPage","headline":"Web headline","datePublished":"2010-01-01"}
            </script>
            <script type="application/ld+json">
              {"@type":"NewsArticle","headline":"Article Headline","datePublished":"2024-05-06",
               "author":{"@type":"Person","name":"Ada Lovelace"},"description":"From json"}
            </script>
            <meta property="article:published_time" content="1999-01-01T00:00:00Z">
            <meta name="citation_publication_date" content="1998/1/1">
            <meta name="DC.issued" content="1997-01-01">
            <link rel="canonical" href="https://example.com/canonical-article">
            </head><body></body></html>
        "#;
        let (response, typed) = urls();
        let meta = read_meta(html, &response, &typed);
        assert_eq!(meta.title.as_deref(), Some("Article Headline"));
        assert_eq!(meta.published.as_deref(), Some("2024-05-06"));
        assert_eq!(meta.authors, vec!["Ada Lovelace".to_string()]);
        assert_eq!(meta.description.as_deref(), Some("From json"));
        assert_eq!(meta.language, "de-DE");
        assert_eq!(meta.identifier, "https://example.com/canonical-article");
        assert_eq!(meta.banner, None);
    }

    #[test]
    fn banner_prefers_og_image_then_twitter_then_json_ld() {
        let (response, typed) = urls();
        let html = r#"
            <html><head>
            <base href="http://cdn.example/articles/">
            <meta property="og:image" content="banner.jpg">
            <meta name="twitter:image" content="https://other.example/card.jpg">
            <script type="application/ld+json">
              {"@type":"NewsArticle","image":{"url":"https://cdn.example/hero.jpg"}}
            </script>
            </head></html>
        "#;
        let meta = read_meta(html, &response, &typed);
        assert_eq!(
            meta.banner.as_deref(),
            Some("http://cdn.example/articles/banner.jpg")
        );

        let twitter = r#"
            <html><head>
            <meta name="twitter:image" content="https://other.example/card.jpg">
            <script type="application/ld+json">
              {"@type":"NewsArticle","image":"https://cdn.example/hero.jpg"}
            </script>
            </head></html>
        "#;
        let meta = read_meta(twitter, &response, &typed);
        assert_eq!(
            meta.banner.as_deref(),
            Some("https://other.example/card.jpg")
        );

        let json = r#"
            <html><head>
            <meta property="og:image" content="javascript:alert(1)">
            <script type="application/ld+json">
              {"@type":"NewsArticle","image":["https://cdn.example/hero.jpg"]}
            </script>
            </head></html>
        "#;
        let meta = read_meta(json, &response, &typed);
        assert_eq!(meta.banner.as_deref(), Some("https://cdn.example/hero.jpg"));
    }

    #[test]
    fn date_falls_through_and_missing_date_is_none() {
        let html = r#"
            <html><head>
            <meta property="og:title" content="OG Title">
            <meta property="article:published_time" content=" ">
            <meta name="citation_publication_date" content="1998/1/1">
            <meta name="DC.issued" content="1997-01-01">
            <link rel="canonical" href="/not-absolute">
            </head></html>
        "#;
        let (response, typed) = urls();
        let meta = read_meta(html, &response, &typed);
        assert_eq!(meta.published.as_deref(), Some("1998/1/1"));
        assert_eq!(meta.identifier, typed.as_str());
        assert_eq!(meta.language, "und");

        let bare = "<html><head><title>Only</title></head></html>";
        let bare = read_meta(bare, &response, &typed);
        assert_eq!(bare.title.as_deref(), Some("Only"));
        assert_eq!(bare.published, None);
        assert_eq!(bare.language, "und");
    }

    #[test]
    fn base_href_overrides_response_url_not_canonical() {
        let html = r#"
            <html><head>
            <base href="http://cdn.example/articles/">
            <link rel="canonical" href="https://canonical.example/a">
            </head></html>
        "#;
        let (response, typed) = urls();
        let meta = read_meta(html, &response, &typed);
        assert_eq!(meta.base.as_str(), "http://cdn.example/articles/");
        assert_eq!(meta.identifier, "https://canonical.example/a");
    }
}
