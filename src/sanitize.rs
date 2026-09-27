use ammonia::{Builder, UrlRelative};

/// Ammonia policy for article HTML that will be stored as XHTML.
pub fn clean(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_generic_attributes(&["id", "dir"])
        .add_clean_content_tags(&[
            "iframe", "video", "audio", "embed", "object", "noscript", "canvas", "svg", "math",
            "form",
        ])
        .add_tag_attributes("img", &["data-folio-src"])
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None);
    builder.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn keeps_default_lang_and_title_plus_id_and_dir() {
        let out = clean(r#"<p id="a" lang="en" title="t" dir="rtl">Hi</p>"#);
        assert!(out.contains("id="), "{out}");
        assert!(out.contains("lang="), "{out}");
        assert!(out.contains("title="), "{out}");
        assert!(out.contains("dir="), "{out}");
    }
}
