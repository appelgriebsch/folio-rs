use dom_query::{Document, NodeRef};

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Ammonia emits HTML5. EPUB content documents need well-formed XML.
pub fn to_xhtml(fragment: &str, lang: &str, title: &str) -> String {
    let body = serialize_fragment(fragment);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><title>{}</title><style type=\"text/css\">img {{ max-width: 100%; height: auto; }}</style></head>\n<body>\n{body}\n</body>\n</html>\n",
        escape_text(title)
    )
}

pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_forbidden_xml(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_forbidden_xml(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_forbidden_xml(ch: char) -> bool {
    matches!(ch, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}')
}

fn serialize_fragment(fragment: &str) -> String {
    let doc = Document::fragment(fragment);
    let mut out = String::new();
    for child in doc.tree.root().children() {
        write_node(&child, &mut out);
    }
    out
}

fn write_node(node: &NodeRef<'_>, out: &mut String) {
    if node.is_text() {
        out.push_str(&escape_text(&node.immediate_text()));
        return;
    }
    if !node.is_element() {
        return;
    }
    let Some(name) = node.node_name() else {
        return;
    };
    let name = name.to_ascii_lowercase();
    if name == "data-folio-src" {
        return;
    }
    out.push('<');
    out.push_str(&name);
    for attr in node.attrs() {
        let local = attr.name.local.as_ref();
        if local.eq_ignore_ascii_case("data-folio-src") {
            continue;
        }
        // Pixel width and height pin an image larger than an ereader screen.
        if name == "img" && matches!(local, "width" | "height" | "style") {
            continue;
        }
        if !is_xml_name(local) {
            continue;
        }
        out.push(' ');
        out.push_str(local);
        out.push_str("=\"");
        out.push_str(&escape_attr(&attr.value));
        out.push('"');
    }
    if name == "img" {
        out.push_str(" style=\"max-width: 100%; height: auto;\"");
    }
    if VOID.contains(&name.as_str()) {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in node.children() {
        write_node(&child, out);
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
}

fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::to_xhtml;

    #[test]
    fn escapes_ampersand_and_closes_void_tags() {
        let xhtml = to_xhtml(
            r#"<p id="p" dir="ltr">fish & chips</p><br><img src="a.png?x=1&y=2" width="2560" height="1440" alt="x">"#,
            "en",
            "A & B",
        );
        assert!(xhtml.contains("fish &amp; chips"), "{xhtml}");
        assert!(xhtml.contains("<title>A &amp; B</title>"), "{xhtml}");
        assert!(xhtml.contains("<br/>"), "{xhtml}");
        assert!(xhtml.contains("x=1&amp;y=2"), "{xhtml}");
        assert!(xhtml.contains("<img "), "{xhtml}");
        assert!(!xhtml.contains("width="), "{xhtml}");
        assert!(!xhtml.contains("height="), "{xhtml}");
        assert!(
            xhtml.contains("style=\"max-width: 100%; height: auto;\""),
            "{xhtml}"
        );
        assert!(
            xhtml.contains("img { max-width: 100%; height: auto; }"),
            "{xhtml}"
        );
        assert!(xhtml.contains("/>"), "{xhtml}");
        assert!(!xhtml.contains("<br>"), "{xhtml}");
        assert!(
            xhtml.contains("xmlns=\"http://www.w3.org/1999/xhtml\""),
            "{xhtml}"
        );
    }
}
