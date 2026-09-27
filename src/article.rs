use dom_query::Document;
use dom_smoothie::{Config, Readability, ReadabilityError};

use crate::error::Error;

pub struct ArticleHtml {
    pub html: String,
}

pub fn extract_article(html: &str, base: &url::Url, page_url: &str) -> Result<ArticleHtml, Error> {
    let mut config = Config::default();
    // Short articles are still articles. The default threshold is sized for a full page score.
    config.char_threshold = 20;
    let mut readability = Readability::new(html, Some(base.as_str()), Some(config))
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

pub fn has_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_whitespace())
}

pub fn fragment_has_text(html: &str) -> bool {
    let doc = Document::fragment(html);
    has_text(&doc.tree.root().text())
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
}
