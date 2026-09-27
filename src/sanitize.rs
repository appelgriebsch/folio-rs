use std::collections::HashSet;

use ammonia::{Builder, UrlRelative};

/// Ammonia policy for article HTML that will be stored as XHTML.
pub fn clean(html: &str) -> String {
    let mut generic = HashSet::new();
    generic.insert("id");
    generic.insert("dir");
    let mut builder = Builder::default();
    builder
        .generic_attributes(generic)
        .add_clean_content_tags(&[
            "iframe", "video", "audio", "embed", "object", "noscript", "canvas", "svg", "math",
            "form",
        ])
        .add_tag_attributes("img", &["data-folio-src"])
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None);
    builder.clean(html).to_string()
}
