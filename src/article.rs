use dom_query::Document;
use dom_smoothie::{Config, Readability, ReadabilityError};

use crate::error::Error;

pub struct ArticleHtml {
    pub html: String,
}

pub fn extract_article(html: &str, base: &url::Url, page_url: &str) -> Result<ArticleHtml, Error> {
    let html = prepare_for_reader(html);
    let mut config = Config::default();
    // Short articles are still articles. The default threshold is sized for a full page score.
    config.char_threshold = 20;
    let mut readability = Readability::new(html.as_str(), Some(base.as_str()), Some(config))
        .map_err(|err| map_readability(err, page_url))?;
    let article = readability
        .parse()
        .map_err(|err| map_readability(err, page_url))?;
    let text = article.text_content.to_string();
    if !has_text(&text) {
        return Err(Error::NoArticle {
            url: page_url.to_string(),
        });
    }
    Ok(ArticleHtml {
        html: article.content.to_string(),
    })
}

/// Title and byline belong at the top. Reader extraction drops an `h1` that
/// matches the page title, and a short linked byline, so the body can open
/// on a kicker such as "glass-terpiece".
pub fn with_header(html: &str, title: &str, authors: &[String], published: Option<&str>) -> String {
    let title = title.trim();
    if title.is_empty() {
        return html.to_string();
    }
    let doc = Document::fragment(html);
    let want = flat(title);
    for heading in doc.select("h1, h2, h3, h4, h5, h6").nodes() {
        if flat(&heading.text()) == want {
            heading.remove_from_parent();
        }
    }
    let body = doc.tree.root().inner_html().to_string();
    let mut out = format!("<h1>{}</h1>", crate::xhtml::escape_text(title));
    if let Some(line) = byline_line(&body, authors, published) {
        out.push_str("<p>");
        out.push_str(&crate::xhtml::escape_text(&line));
        out.push_str("</p>");
    }
    out.push_str(&body);
    out
}

fn byline_line(body: &str, authors: &[String], published: Option<&str>) -> Option<String> {
    let opening = opening_text(body);
    let names: Vec<&str> = authors
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    let author_present = names.iter().any(|name| opening.contains(&flat(name)));
    let date = published
        .map(display_date)
        .filter(|value| !value.is_empty());
    let date_present = date
        .as_deref()
        .is_some_and(|value| opening.contains(&flat(value)))
        || published.is_some_and(|value| {
            let day = value.trim();
            let day = day.get(..10).unwrap_or(day);
            !day.is_empty() && opening.contains(&flat(day))
        });
    if names.is_empty() && date.is_none() {
        return None;
    }
    if author_present && (date_present || date.is_none()) {
        return None;
    }
    if !author_present && date_present && names.is_empty() {
        return None;
    }
    let mut line = names.join(", ");
    if let Some(date) = date.filter(|_| !date_present) {
        if !line.is_empty() {
            line.push_str(" — ");
        }
        line.push_str(&date);
    }
    if line.is_empty() { None } else { Some(line) }
}

fn opening_text(html: &str) -> String {
    let doc = Document::fragment(html);
    let text = flat(&doc.tree.root().text());
    text.chars().take(500).collect()
}

fn flat(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn display_date(raw: &str) -> String {
    let trimmed = raw.trim();
    let normalized = trimmed.replace('/', "-");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year = &normalized[0..4];
        let month: u32 = normalized[5..7].parse().unwrap_or(0);
        let day: u32 = normalized[8..10].parse().unwrap_or(0);
        if let Some(name) = month_name(month)
            && (1..=31).contains(&day)
            && year.chars().all(|ch| ch.is_ascii_digit())
        {
            return format!("{name} {day}, {year}");
        }
    }
    trimmed.to_string()
}

fn month_name(month: u32) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS.get(month.checked_sub(1)? as usize).copied()
}

pub fn has_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_whitespace())
}

pub fn fragment_has_text(html: &str) -> bool {
    let doc = Document::fragment(html);
    has_text(&doc.tree.root().text())
}

/// Reader scoring treats a class containing "hidden" as something to drop.
/// `overflow-hidden` only clips overflow, so captioned galleries were deleted
/// while a bare image was unwrapped and kept.
fn prepare_for_reader(html: &str) -> String {
    let doc = Document::from(html);
    neutralize_overflow_hidden(&doc);
    drop_duplicate_css_hidden_images(&doc);
    doc.html().to_string()
}

fn neutralize_overflow_hidden(doc: &Document) {
    let nodes: Vec<_> = doc.select("[class]").nodes().to_vec();
    for node in nodes {
        let Some(class) = node.attr("class") else {
            continue;
        };
        let next = rewrite_overflow_hidden(&class);
        if next != class.as_ref() {
            node.set_attr("class", &next);
        }
    }
}

fn rewrite_overflow_hidden(class: &str) -> String {
    class
        .split_whitespace()
        .map(rewrite_overflow_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn rewrite_overflow_token(token: &str) -> String {
    let Some((prefix, utility)) = token.rsplit_once(':') else {
        return match rewrite_overflow_utility(token) {
            Some(utility) => utility.to_string(),
            None => token.to_string(),
        };
    };
    match rewrite_overflow_utility(utility) {
        Some(utility) => format!("{prefix}:{utility}"),
        None => token.to_string(),
    }
}

fn rewrite_overflow_utility(utility: &str) -> Option<&'static str> {
    let utility = utility.strip_prefix('!').unwrap_or(utility);
    let utility = utility.strip_suffix('!').unwrap_or(utility);
    match utility {
        "overflow-hidden" => Some("overflow-clip"),
        "overflow-x-hidden" => Some("overflow-x-clip"),
        "overflow-y-hidden" => Some("overflow-y-clip"),
        _ => None,
    }
}

/// A `hidden` image next to a visible sibling is the undisplayed copy.
fn drop_duplicate_css_hidden_images(doc: &Document) {
    let imgs: Vec<_> = doc.select("img").nodes().to_vec();
    for img in imgs {
        if !img
            .attr("class")
            .is_some_and(|class| has_unconditional_hidden_token(&class))
        {
            continue;
        }
        let Some(parent) = img.parent() else {
            continue;
        };
        let visible_sibling = parent.element_children().iter().any(|child| {
            child
                .node_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("img"))
                && child.id != img.id
                && !child
                    .attr("class")
                    .is_some_and(|class| has_unconditional_hidden_token(&class))
        });
        if visible_sibling {
            img.remove_from_parent();
        }
    }
}

fn has_unconditional_hidden_token(class: &str) -> bool {
    class.split_whitespace().any(|token| {
        let token = token.strip_prefix('!').unwrap_or(token);
        let token = token.strip_suffix('!').unwrap_or(token);
        token == "hidden"
    })
}

fn map_readability(err: ReadabilityError, page_url: &str) -> Error {
    match err {
        ReadabilityError::GrabFailed | ReadabilityError::BadDocumentURL => Error::NoArticle {
            url: page_url.to_string(),
        },
        ReadabilityError::TooManyElements(_, _) => Error::ArticleTooLarge(page_url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_elements_is_not_a_fetch_failure() {
        let html = "<html><body><article><p>One sentence of article text.</p><p>More.</p></article></body></html>";
        let mut config = Config::default();
        config.char_threshold = 20;
        config.max_elements_to_parse = 1;
        let base = url::Url::parse("http://example.test/a").unwrap();
        let mut readability = Readability::new(html, Some(base.as_str()), Some(config)).unwrap();
        let err = readability.parse().unwrap_err();
        let mapped = map_readability(err, base.as_str());
        assert!(matches!(mapped, Error::ArticleTooLarge(_)));
        assert_eq!(
            mapped.to_string(),
            "article is too large to read at http://example.test/a"
        );
    }

    fn page(body: &str) -> String {
        format!(
            "<!DOCTYPE html><html><head><title>Review</title></head><body><article>{body}</article></body></html>"
        )
    }

    fn prose(n: &str) -> String {
        format!(
            "<p>Paragraph {n}, with commas, stays in the article so the gallery is not the whole page.</p>"
        )
    }

    fn extracted(body: &str) -> String {
        let html = page(body);
        let base = url::Url::parse("https://news.example/review").unwrap();
        extract_article(&html, &base, base.as_str()).unwrap().html
    }

    #[test]
    fn captioned_overflow_hidden_gallery_keeps_its_images() {
        let body = format!(
            "{}<div class=\"ars-lightbox align-fullwidth\"><div class=\"ars-lightbox-item relative overflow-hidden rounded-sm\"><a href=\"https://cdn.example/island.jpg\"><img src=\"https://cdn.example/island.jpg\" alt=\"Island\"></a><div class=\"caption\">The smaller island on the phone.</div></div><div class=\"md:overflow-x-hidden\"><a href=\"https://cdn.example/bench.jpg\"><img src=\"https://cdn.example/bench.jpg\" alt=\"Chart\"></a><div class=\"caption\">Benchmark chart for the chip.</div></div></div>{}",
            prose("one"),
            prose("two"),
        );
        let html = extracted(&body);
        assert!(html.contains("https://cdn.example/island.jpg"), "{html}");
        assert!(html.contains("https://cdn.example/bench.jpg"), "{html}");
        assert!(html.contains("The smaller island"), "{html}");
        assert!(html.contains("Benchmark chart"), "{html}");
    }

    #[test]
    fn display_none_block_stays_out_and_hidden_duplicate_is_dropped() {
        let body = format!(
            "{}<div class=\"hidden\"><img src=\"https://cdn.example/secret.jpg\" alt=\"secret\"><p>Sponsored aside, with a comma, that the page does not show.</p></div><a href=\"https://cdn.example/hero.jpg\"><img class=\"object-cover hidden\" src=\"https://cdn.example/hero-small.jpg\" alt=\"phone\"><img class=\"intro-image\" src=\"https://cdn.example/hero.jpg\" alt=\"phone\"></a><p>Apple's phone in burgundy.</p>{}",
            prose("one"),
            prose("two"),
        );
        let html = extracted(&body);
        assert!(html.contains("https://cdn.example/hero.jpg"), "{html}");
        assert!(!html.contains("hero-small.jpg"), "{html}");
        assert!(!html.contains("secret.jpg"), "{html}");
    }

    #[test]
    fn overflow_tokens_lose_the_hidden_substring() {
        assert_eq!(
            rewrite_overflow_hidden(
                "relative overflow-hidden md:overflow-x-hidden !overflow-y-hidden"
            ),
            "relative overflow-clip md:overflow-x-clip overflow-y-clip"
        );
        assert_eq!(
            rewrite_overflow_hidden("hidden md:block"),
            "hidden md:block"
        );
    }

    #[test]
    fn header_title_and_byline_precede_the_kicker() {
        let html = r#"<!DOCTYPE html><html><head>
            <title>macOS 26 Tahoe: The Ars Technica review - Ars Technica</title>
            <meta property="og:title" content="macOS 26 Tahoe: The Ars Technica review">
            <script type="application/ld+json">
              {"@type":"NewsArticle","headline":"macOS 26 Tahoe: The Ars Technica review",
               "datePublished":"2025-09-15T13:00:27-04:00",
               "author":{"@type":"Person","name":"Andrew Cunningham"}}
            </script>
            </head><body><article><header>
            <p>glass-terpiece</p>
            <h1>macOS 26 Tahoe: The Ars Technica review</h1>
            <p>Liquid Glass brings translucent sheen to the typical batch of iterative changes.</p>
            <div><a href="https://news.example/author/andrew">Andrew Cunningham</a> – <time datetime="2025-09-15">Sep 15, 2025</time></div>
            </header>
            <p>The last time Apple gave macOS a fresh design was in 2020, and this paragraph has commas, and enough words to be the article.</p>
            <p>Second paragraph continues the review, with commas, so the extractor keeps the story and the header around it.</p>
            <p>Third paragraph names the release and keeps the score high enough for a short page.</p>
            </article></body></html>"#;
        let base = url::Url::parse("https://news.example/tahoe").unwrap();
        let article = extract_article(html, &base, base.as_str()).unwrap();
        let headed = with_header(
            &article.html,
            "macOS 26 Tahoe: The Ars Technica review",
            &["Andrew Cunningham".to_string()],
            Some("2025-09-15T13:00:27-04:00"),
        );
        let title_at = headed.find("<h1>").expect(&headed);
        let kicker_at = headed.find("glass-terpiece").expect(&headed);
        assert!(title_at < kicker_at, "{headed}");
        assert!(headed.contains("Andrew Cunningham"), "{headed}");
        assert!(headed.contains("Sep 15, 2025"), "{headed}");
        assert_eq!(headed.matches("<h1>").count(), 1, "{headed}");
        let again = with_header(
            &headed,
            "macOS 26 Tahoe: The Ars Technica review",
            &["Andrew Cunningham".to_string()],
            Some("2025-09-15T13:00:27-04:00"),
        );
        assert_eq!(again.matches("<h1>").count(), 1, "{again}");
        assert_eq!(again.matches("Andrew Cunningham").count(), 1, "{again}");
    }

    #[test]
    fn display_date_uses_the_calendar_day() {
        assert_eq!(display_date("2025-09-15T17:00:27+00:00"), "Sep 15, 2025");
        assert_eq!(display_date("2024-05-06"), "May 6, 2024");
        assert_eq!(display_date("not-a-date"), "not-a-date");
    }
}
