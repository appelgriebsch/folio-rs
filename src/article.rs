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
        ReadabilityError::TooManyElements(_, _) => Error::Fetch {
            url: page_url.to_string(),
            reason: err.to_string(),
        },
    }
}
