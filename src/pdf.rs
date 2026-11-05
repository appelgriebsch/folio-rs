//! One article, one PDF. The HTML is a small XHTML subset so printpdf can parse it.
//!
//! A render fails the run when layout returns `Err`, the page list is empty,
//! or any `PdfWarnMsg` has severity `Error`. Info and Warning stay on stderr
//! and the file is still written. `printpdf` emits Error for a layout or parse
//! failure and Warning for a skipped image. Fonts are the bundled IBM Plex
//! files only. A character none of them can draw produces one `warning:` line.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use ab_glyph::Font;
use dom_query::{Document, NodeRef};
use printpdf::date::{Date, DateTime, Offset, OffsetDateTime, Time};
use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions};

use crate::error::Error;
use crate::images::EmbeddedImage;
use crate::meta::PageMeta;
use crate::xhtml::{escape_attr, escape_text};

const FAMILY: &str = "IBM Plex Serif";

const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/IBMPlexSerif-Regular.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../assets/fonts/IBMPlexSerif-Italic.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/IBMPlexSerif-Bold.ttf");

const BLOCKS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "blockquote",
    "br",
    "img",
    "strong",
    "em",
    "b",
    "i",
];

/// Block elements printpdf will not lay out. Emitted as `<p>` so their text stays.
const AS_P: &[&str] = &[
    "div",
    "section",
    "article",
    "figure",
    "figcaption",
    "dl",
    "dt",
    "dd",
    "pre",
    "caption",
    "header",
    "footer",
    "aside",
    "address",
];

pub struct PdfMeta<'a> {
    pub title: &'a str,
    pub meta: &'a PageMeta,
    pub file_time: SystemTime,
}

/// XHTML for `printpdf`, plus the image bytes it looks up by `src`.
/// SVG images are omitted. Their text-free absence is the caller's warning.
pub fn article_xhtml(
    fragment: &str,
    images: &[EmbeddedImage],
) -> (String, BTreeMap<String, Vec<u8>>) {
    let doc = Document::fragment(fragment);
    let mut out = String::new();
    let mut map = BTreeMap::new();
    for child in doc.tree.root().children() {
        write_node(&child, images, &mut map, &mut out);
    }
    (out, map)
}

pub fn render(
    xhtml_body: &str,
    images: &BTreeMap<String, Vec<u8>>,
    info: &PdfMeta<'_>,
) -> Result<Vec<u8>, Error> {
    let html = wrap(xhtml_body, info.title);
    let image_map = images
        .iter()
        .map(|(key, bytes)| (key.clone(), Base64OrRaw::Raw(bytes.clone())))
        .collect();
    let fonts = font_map();
    let pool = bundled_font_pool();
    let options = GeneratePdfOptions {
        font_embedding: Some(true),
        page_width: Some(210.0),
        page_height: Some(297.0),
        margin_top: Some(20.0),
        margin_right: Some(20.0),
        margin_bottom: Some(20.0),
        margin_left: Some(20.0),
        ..GeneratePdfOptions::default()
    };
    let mut warnings = Vec::new();
    let mut doc = PdfDocument::from_html_with_cache(
        &html,
        &image_map,
        &fonts,
        &options,
        &mut warnings,
        Some(pool),
    )
    .map_err(Error::Write)?;
    require_pages(doc.pages.len())?;
    fail_on_layout_errors(&warnings)?;
    for warning in &warnings {
        if warning.severity == printpdf::PdfParseErrorSeverity::Warning {
            eprintln!("warning: pdf: {}", warning.msg);
        }
    }
    if plex_lacks(&html) {
        eprintln!("warning: pdf: IBM Plex Serif has no glyph for a character in this article");
    }
    apply_info(&mut doc, info);
    let bytes = doc.save(&PdfSaveOptions::default(), &mut warnings);
    fail_on_layout_errors(&warnings)?;
    Ok(bytes)
}

/// Empty layout is a failed PDF: exit 1 and leave no file.
pub(crate) fn require_pages(count: usize) -> Result<(), Error> {
    if count == 0 {
        Err(Error::Write("pdf layout produced no pages".to_string()))
    } else {
        Ok(())
    }
}

fn fail_on_layout_errors(warnings: &[printpdf::PdfWarnMsg]) -> Result<(), Error> {
    let errors: Vec<_> = warnings
        .iter()
        .filter(|msg| msg.severity == printpdf::PdfParseErrorSeverity::Error)
        .map(|msg| msg.msg.as_str())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Write(errors.join("; ")))
    }
}

fn wrap(body: &str, title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>{}</title>
<style type="text/css">
body {{ font-family: "{FAMILY}"; font-size: 11pt; font-weight: 400; font-style: normal; }}
h1, h2, h3, h4, h5, h6, strong, b {{ font-family: "{FAMILY}"; font-weight: 700; font-style: normal; }}
em, i {{ font-family: "{FAMILY}"; font-weight: 400; font-style: italic; }}
blockquote {{ margin-left: 8mm; }}
img {{ max-width: 100%; }}
</style>
</head>
<body>
{body}
</body>
</html>
"#,
        escape_text(title)
    )
}

fn bundled_font_bytes() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        (FAMILY.to_string(), FONT_REGULAR.to_vec()),
        (format!("{FAMILY} Italic"), FONT_ITALIC.to_vec()),
        (format!("{FAMILY} Bold"), FONT_BOLD.to_vec()),
    ])
}

fn bundled_font_pool() -> printpdf::SharedFontPool {
    // ponytail: empty family list skips system fonts. A host with no fonts
    // cannot tell this from a full scan; the PDF then embeds Plex either way.
    printpdf::build_font_pool(&bundled_font_bytes(), Some(&[]))
}

fn font_map() -> BTreeMap<String, Base64OrRaw> {
    bundled_font_bytes()
        .into_iter()
        .map(|(name, bytes)| (name, Base64OrRaw::Raw(bytes)))
        .collect()
}

fn plex_lacks(text: &str) -> bool {
    if text.is_ascii() {
        return false;
    }
    let Ok(fonts) = [FONT_REGULAR, FONT_ITALIC, FONT_BOLD]
        .into_iter()
        .map(ab_glyph::FontRef::try_from_slice)
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    text.chars()
        .any(|ch| !ch.is_ascii() && fonts.iter().all(|font| font.glyph_id(ch).0 == 0))
}

fn apply_info(doc: &mut PdfDocument, info: &PdfMeta<'_>) {
    let file_time = system_time_to_pdf(info.file_time);
    let created = info
        .meta
        .published
        .as_deref()
        .and_then(publish_date)
        .unwrap_or(file_time);
    let dict = &mut doc.metadata.info;
    dict.document_title = info.title.to_string();
    dict.author = info.meta.authors.join(", ");
    dict.subject = info.meta.description.clone().unwrap_or_default();
    dict.creator = "folio".to_string();
    dict.producer = "folio".to_string();
    dict.creation_date = created;
    dict.modification_date = file_time;
    dict.metadata_date = file_time;
}

/// Date-only publish dates are midnight UTC. A timestamp keeps its offset.
/// `/` is a date separator, seconds may be omitted, and a fractional second is ignored.
/// A non-ASCII string, an impossible day, or an impossible clock is not a date.
pub fn publish_date(raw: &str) -> Option<OffsetDateTime> {
    let raw = raw.trim();
    // Byte indexes below are only safe on ASCII. Non-ASCII page dates must not panic.
    if !raw.is_ascii() {
        return None;
    }
    let (date, rest) = split_date(raw)?;
    if rest.is_empty() {
        return Some(at(date, 0, 0, 0, 0));
    }
    let rest = rest
        .strip_prefix('T')
        .or_else(|| rest.strip_prefix('t'))
        .or_else(|| rest.strip_prefix(' '))?;
    let (hour, minute, second, tail) = split_clock(rest)?;
    let offset = if tail.is_empty() || tail == "Z" || tail == "z" {
        0
    } else {
        parse_offset(tail)?
    };
    Some(at(date, hour, minute, second, offset))
}

fn at(date: (i32, u8, u8), hour: u8, minute: u8, second: u8, offset_minutes: i32) -> DateTime {
    let sign = if offset_minutes < 0 { -1 } else { 1 };
    let abs = offset_minutes.unsigned_abs();
    DateTime {
        date: Date {
            year: date.0,
            month: date.1,
            day: date.2,
        },
        time: Time {
            hour,
            minute,
            second,
            millisecond: 0,
        },
        offset: Offset {
            hours: sign as i8 * (abs / 60) as i8,
            minutes: sign as i8 * (abs % 60) as i8,
            seconds: 0,
            milliseconds: 0,
        },
    }
}

fn split_date(raw: &str) -> Option<((i32, u8, u8), &str)> {
    let bytes = raw.as_bytes();
    let sep = *bytes.get(4)?;
    if (sep != b'-' && sep != b'/') || bytes.get(7) != Some(&sep) || bytes.len() < 10 {
        return None;
    }
    let year: i32 = raw.get(0..4)?.parse().ok()?;
    let month: u8 = raw.get(5..7)?.parse().ok()?;
    let day: u8 = raw.get(8..10)?.parse().ok()?;
    if !valid_day(year, month, day) {
        return None;
    }
    Some(((year, month, day), raw.get(10..)?))
}

fn valid_day(year: i32, month: u8, day: u8) -> bool {
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max).contains(&day)
}

fn split_clock(raw: &str) -> Option<(u8, u8, u8, &str)> {
    let bytes = raw.as_bytes();
    if bytes.get(2) != Some(&b':') {
        return None;
    }
    let hour: u8 = raw.get(0..2)?.parse().ok()?;
    let minute: u8 = raw.get(3..5)?.parse().ok()?;
    let (second, tail) = if bytes.get(5) == Some(&b':') {
        (raw.get(6..8)?.parse().ok()?, raw.get(8..)?)
    } else {
        (0, raw.get(5..)?)
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, skip_fraction(tail)?))
}

fn skip_fraction(tail: &str) -> Option<&str> {
    if !tail.starts_with('.') {
        return Some(tail);
    }
    let digits = tail
        .get(1..)?
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    tail.get(1 + digits..)
}

fn parse_offset(raw: &str) -> Option<i32> {
    let (sign, rest) = match raw.as_bytes().first()? {
        b'+' => (1, raw.get(1..)?),
        b'-' => (-1, raw.get(1..)?),
        _ => return None,
    };
    let (hours, minutes) = if let Some((hours, minutes)) = rest.split_once(':') {
        (hours, minutes)
    } else if rest.len() == 4 {
        (rest.get(..2)?, rest.get(2..)?)
    } else if rest.len() == 2 {
        (rest, "00")
    } else {
        return None;
    };
    let hours: i32 = hours.parse().ok()?;
    let minutes: i32 = minutes.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

fn system_time_to_pdf(time: SystemTime) -> DateTime {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    DateTime::from_unix_timestamp(seconds).unwrap_or_else(|_| DateTime::epoch())
}

fn write_node(
    node: &NodeRef<'_>,
    images: &[EmbeddedImage],
    map: &mut BTreeMap<String, Vec<u8>>,
    out: &mut String,
) {
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
    if name == "a" {
        for child in node.children() {
            write_node(&child, images, map, out);
        }
        return;
    }
    if name == "sup" || name == "sub" {
        out.push(if name == "sup" { '^' } else { '_' });
        for child in node.children() {
            write_node(&child, images, map, out);
        }
        return;
    }
    if name == "hr" {
        return;
    }
    if name == "table" {
        write_table(node, images, map, out);
        return;
    }
    let tag = if AS_P.contains(&name.as_str()) {
        "p"
    } else if BLOCKS.contains(&name.as_str()) {
        name.as_str()
    } else {
        for child in node.children() {
            write_node(&child, images, map, out);
        }
        return;
    };
    if name == "br" {
        out.push_str("<br/>");
        return;
    }
    if name == "img" {
        write_img(node, images, map, out);
        return;
    }
    if name == "li" {
        let children = node.children();
        if children.iter().any(is_nested_list) {
            // ponytail: printpdf 0.12.8 duplicates an li's text when that li also
            // contains a ul or ol. Hoist each nested list to a sibling after the li.
            // Text that sat after the nested list is kept, but moves ahead of it.
            // Put the list back inside the li when printpdf stops doubling the text.
            out.push_str("<li>");
            for child in &children {
                if !is_nested_list(child) {
                    write_node(child, images, map, out);
                }
            }
            out.push_str("</li>");
            for child in &children {
                if is_nested_list(child) {
                    write_node(child, images, map, out);
                }
            }
            return;
        }
    }
    out.push('<');
    out.push_str(tag);
    out.push('>');
    for child in node.children() {
        write_node(&child, images, map, out);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn write_table(
    node: &NodeRef<'_>,
    images: &[EmbeddedImage],
    map: &mut BTreeMap<String, Vec<u8>>,
    out: &mut String,
) {
    for child in node.children() {
        if is_tag(&child, "caption") {
            out.push_str("<p>");
            for grand in child.children() {
                write_node(&grand, images, map, out);
            }
            out.push_str("</p>");
        }
    }
    out.push_str("<table>");
    for child in node.children() {
        if !is_tag(&child, "caption") {
            write_node(&child, images, map, out);
        }
    }
    out.push_str("</table>");
}

fn is_tag(node: &NodeRef<'_>, want: &str) -> bool {
    node.node_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(want))
}

fn is_nested_list(node: &NodeRef<'_>) -> bool {
    let Some(name) = node.node_name() else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name == "ul" || name == "ol"
}

fn write_img(
    node: &NodeRef<'_>,
    images: &[EmbeddedImage],
    map: &mut BTreeMap<String, Vec<u8>>,
    out: &mut String,
) {
    let Some(src) = node.attr("src") else {
        return;
    };
    let Some(image) = images.iter().find(|image| image.href == src.as_ref()) else {
        return;
    };
    if image.media_type == "image/svg+xml" {
        eprintln!("warning: omitted svg image from the pdf {}", image.href);
        return;
    }
    let key = image.href.clone();
    map.entry(key.clone())
        .or_insert_with(|| image.bytes.clone());
    let alt = node.attr("alt").unwrap_or_default();
    out.push_str("<img src=\"");
    out.push_str(&escape_attr(&key));
    out.push_str("\" alt=\"");
    out.push_str(&escape_attr(&alt));
    out.push_str("\"/>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use printpdf::PdfParseOptions;

    /// Pull visible text back out of a saved PDF. Tests use this instead of poppler.
    fn pdf_text(bytes: &[u8]) -> String {
        let mut warnings = Vec::new();
        let doc = PdfDocument::parse(
            bytes,
            &PdfParseOptions {
                fail_on_error: false,
            },
            &mut warnings,
        )
        .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
        doc.extract_text()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn pdf_font_names(bytes: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        for key in [b"/BaseFont" as &[u8], b"/FontName"] {
            let mut rest = bytes;
            while let Some(at) = rest.windows(key.len()).position(|window| window == key) {
                let after = &rest[at + key.len()..];
                let start = after
                    .iter()
                    .position(|byte| !byte.is_ascii_whitespace() && *byte != b'/')
                    .unwrap_or(after.len());
                let after = &after[start..];
                let end = after
                    .iter()
                    .position(|byte| {
                        byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>' | b']' | b')')
                    })
                    .unwrap_or(after.len());
                if let Ok(name) = std::str::from_utf8(&after[..end])
                    && !name.is_empty()
                {
                    names.push(name.to_string());
                }
                rest = &rest[at + key.len()..];
            }
        }
        names.sort();
        names.dedup();
        names
    }

    fn image(href: &str, media: &str) -> EmbeddedImage {
        EmbeddedImage {
            href: href.to_string(),
            bytes: b"img-bytes".to_vec(),
            media_type: media.to_string(),
        }
    }

    #[test]
    fn serializer_escapes_keeps_text_and_maps_images() {
        let html = r#"<p class="x">a &amp; b &lt; c &gt; &quot;q&quot;</p><div>kept <span>inside</span></div><ul><li>one<ol><li>nested</li></ol></li></ul><table><thead><tr><th>H</th></tr></thead><tbody><tr><td>C</td></tr></tbody></table><img src="images/img-1.png" alt="a&quot;b"><img src="images/img-2.svg" alt="svg"><script>SECRET</script>"#;
        let images = vec![
            image("images/img-1.png", "image/png"),
            image("images/img-2.svg", "image/svg+xml"),
        ];
        let (body, map) = article_xhtml(html, &images);
        assert!(body.contains("a &amp; b &lt; c &gt; \"q\""), "{body}");
        assert!(!body.contains("<div"), "{body}");
        assert!(!body.contains("<span"), "{body}");
        assert!(!body.contains("<script"), "{body}");
        assert!(body.contains("kept inside"), "{body}");
        assert!(body.contains("SECRET"), "{body}");
        assert!(
            body.contains("<ul><li>one</li><ol><li>nested</li></ol></ul>"),
            "{body}"
        );
        assert!(
            body.contains("<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>C</td></tr></tbody></table>"),
            "{body}"
        );
        assert!(body.contains("src=\"images/img-1.png\""), "{body}");
        assert!(body.contains("alt=\"a&quot;b\""), "{body}");
        assert!(!body.contains("img-2.svg"), "{body}");
        assert_eq!(map.len(), 1);
        assert_eq!(map["images/img-1.png"], b"img-bytes");
        let xml = wrap(&body, r#"A & B "t""#);
        assert!(xml.contains("<title>A &amp; B \"t\"</title>"), "{xml}");
        for token in xmlparser::Tokenizer::from(xml.as_str()) {
            token.expect("well-formed xml");
        }
    }

    #[test]
    fn unlisted_blocks_become_paragraphs() {
        let html = "<div>Alpha</div><section>Sec</section><article>Art</article><figure>Fig</figure><figcaption>CapFig</figcaption><dl><dt>TermWord</dt><dd>DefWord</dd></dl><pre>PreWord</pre><header>Head</header><footer>Foot</footer><aside>Side</aside><address>Addr</address><hr><p>10<sup>6</sup> and H<sub>2</sub>O</p><table><caption>CapWord</caption><tbody><tr><td>Cell</td></tr></tbody></table>";
        let (body, _) = article_xhtml(html, &[]);
        assert_eq!(
            body,
            "<p>Alpha</p><p>Sec</p><p>Art</p><p>Fig</p><p>CapFig</p><p><p>TermWord</p><p>DefWord</p></p><p>PreWord</p><p>Head</p><p>Foot</p><p>Side</p><p>Addr</p><p>10^6 and H_2O</p><p>CapWord</p><table><tbody><tr><td>Cell</td></tr></tbody></table>"
        );
    }

    #[test]
    fn flattened_blocks_stay_separate_in_the_pdf() {
        let html = "<dl><dt>TermWord</dt><dd>DefWord</dd></dl><table><caption>CapWord</caption><tr><td>Cell</td></tr></table><p>10<sup>6</sup></p><div>Alpha</div><div>Beta</div>";
        let text = pdf_text(&render_html(html));
        assert!(text.contains("TermWord"), "{text}");
        assert!(text.contains("DefWord"), "{text}");
        assert!(!text.contains("TermWordDefWord"), "{text}");
        assert!(text.contains("CapWord"), "{text}");
        assert!(text.contains("10^6"), "{text}");
        assert!(text.contains("Alpha"), "{text}");
        assert!(text.contains("Beta"), "{text}");
        assert!(!text.contains("AlphaBeta"), "{text}");
    }

    #[test]
    fn nested_lists_are_hoisted_beside_the_li() {
        let html =
            r#"<ul><li>outer one<ol><li>nested alpha</li></ol> tail</li><li>outer two</li></ul>"#;
        let (body, _) = article_xhtml(html, &[]);
        assert_eq!(
            body,
            "<ul><li>outer one tail</li><ol><li>nested alpha</li></ol><li>outer two</li></ul>"
        );

        let deep = r#"<ul><li>a<ol><li>b<ul><li>c</li></ul> after</li></ol></li></ul>"#;
        let (body, _) = article_xhtml(deep, &[]);
        assert_eq!(
            body,
            "<ul><li>a</li><ol><li>b after</li><ul><li>c</li></ul></ol></ul>"
        );
    }

    #[test]
    fn nested_list_text_appears_once_in_order() {
        let html = r#"<ul><li>outer one<ol><li>nested alpha</li></ol></li><li>outer two</li></ul>"#;
        let text = pdf_text(&render_html(html));
        assert_eq!(text.matches("outer one").count(), 1, "{text}");
        let one = text.find("outer one").unwrap();
        let alpha = text.find("nested alpha").unwrap();
        let two = text.find("outer two").unwrap();
        assert!(one < alpha && alpha < two, "{text}");
    }

    #[test]
    fn a4_page_keeps_twenty_millimetre_margins() {
        let mut html = String::from("<h1>TitleWord</h1><p>");
        for _ in 0..24 {
            html.push_str("WideWord ");
        }
        html.push_str("</p>");
        for i in 0..70 {
            html.push_str(&format!(
                "<p>Fill{i} stays as its own paragraph on the page.</p>"
            ));
        }
        let bytes = render_html(&html);
        let mut warnings = Vec::new();
        let doc = PdfDocument::parse(
            &bytes,
            &PdfParseOptions {
                fail_on_error: false,
            },
            &mut warnings,
        )
        .unwrap();
        let page = &doc.pages[0];
        // 210 mm × 297 mm. 1 mm = 72/25.4 pt.
        assert!(
            (page.media_box.width.0 - 595.28).abs() < 0.5,
            "{}",
            page.media_box.width.0
        );
        assert!(
            (page.media_box.height.0 - 841.89).abs() < 0.5,
            "{}",
            page.media_box.height.0
        );
        let lines = &doc.extract_text_boxes()[0].lines;
        let word = &lines[0].words[0];
        assert_eq!(word.text, "TitleWord");
        // 20 mm is 56.69 pt. h1 inset puts the first glyph at 62.69, 71.47.
        assert!((word.bbox[0] - 62.69).abs() < 0.5, "x {}", word.bbox[0]);
        assert!((word.bbox[1] - 71.47).abs() < 0.5, "y {}", word.bbox[1]);
        let wide = lines
            .iter()
            .find(|line| line.words.iter().any(|item| item.text == "WideWord"))
            .unwrap();
        assert_eq!(wide.words.len(), 8, "right edge {}", wide.bbox[2]);
        assert!(
            (wide.bbox[2] - 512.18).abs() < 0.5,
            "right {}",
            wide.bbox[2]
        );
        // The last line sits just above the bottom margin. One more line does not fit.
        assert_eq!(lines.len(), 28, "bottom");
        let last_y = lines.last().unwrap().bbox[3];
        assert!((last_y - 765.52).abs() < 0.5, "bottom {last_y}");
    }

    #[test]
    fn blockquote_css_indents_the_quote() {
        let xml = wrap("<blockquote>QuoteStart</blockquote>", "t");
        assert!(xml.contains("blockquote { margin-left: 8mm; }"), "{xml}");
        let bytes = render_html("<p>ParaStart stays</p><blockquote>QuoteStart inset</blockquote>");
        let para = word_x(&bytes, "ParaStart");
        let quote = word_x(&bytes, "QuoteStart");
        let delta = quote - para;
        // 8mm is 22.68pt. printpdf applies this margin; without the rule the delta is 0.
        assert!(
            (delta - 22.68).abs() < 1.0,
            "para {para} quote {quote} delta {delta}"
        );
    }

    fn render_html(html: &str) -> Vec<u8> {
        let meta = PageMeta {
            title: None,
            authors: Vec::new(),
            description: None,
            language: "en".to_string(),
            identifier: "https://example.com/a".to_string(),
            published: None,
            banner: None,
            banners: Vec::new(),
            base: url::Url::parse("https://example.com/a").unwrap(),
        };
        let (body, images) = article_xhtml(html, &[]);
        render(
            &body,
            &images,
            &PdfMeta {
                title: "t",
                meta: &meta,
                file_time: UNIX_EPOCH,
            },
        )
        .unwrap()
    }

    fn word_x(bytes: &[u8], word: &str) -> f32 {
        let mut warnings = Vec::new();
        let doc = PdfDocument::parse(
            bytes,
            &PdfParseOptions {
                fail_on_error: false,
            },
            &mut warnings,
        )
        .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
        for page in doc.extract_text_boxes() {
            for line in page.lines {
                for item in line.words {
                    if item.text == word {
                        return item.bbox[0];
                    }
                }
            }
        }
        panic!("missing {word}");
    }

    #[test]
    fn bundled_fonts_are_the_only_pool_and_the_only_embedded_faces() {
        let pool = bundled_font_pool();
        let listed = pool.fc_cache.list();
        assert!(!listed.is_empty());
        for (pattern, id) in &listed {
            assert!(pool.fc_cache.is_memory_font(id), "{pattern:?}");
            let label = pattern
                .family
                .as_deref()
                .or(pattern.name.as_deref())
                .unwrap_or("");
            assert!(label.to_ascii_lowercase().contains("plex"), "{pattern:?}");
        }
        assert!(plex_lacks("東京"));
        assert!(plex_lacks("😀"));
        assert!(!plex_lacks("café Latin"));
        let bytes = render_html("<h1>東京</h1><p>😀 LatinWord</p>");
        let names = pdf_font_names(&bytes);
        assert!(!names.is_empty(), "{names:?}");
        for name in &names {
            assert!(
                name.to_ascii_lowercase().contains("plex"),
                "{name} in {names:?}"
            );
        }
    }

    #[test]
    fn publish_date_accepts_page_dates_and_rejects_impossible_ones() {
        let cases = [
            ("2024-05-06", Some(1_714_953_600_i64)),
            ("2024/05/06", Some(1_714_953_600)),
            ("2024-05-06T10:00:00Z", Some(1_714_989_600)),
            ("2024-05-06T10:00:00.123Z", Some(1_714_989_600)),
            ("2024-05-06T10:00:00.123+00:00", Some(1_714_989_600)),
            ("2024-05-06T10:00:00z", Some(1_714_989_600)),
            ("2024-05-06T10:00:00", Some(1_714_989_600)),
            ("2024-05-06t10:00:00Z", Some(1_714_989_600)),
            ("2024-05-06 10:00:00Z", Some(1_714_989_600)),
            ("2024-05-06T15:04+02:00", Some(1_715_000_640)),
            ("2024-05-06T15:04:05+02:00", Some(1_715_000_645)),
            ("2020-01-02T03:04:05-0530", Some(1_577_954_045)),
            ("2024-05-06T15:04:05+0530", Some(1_714_988_045)),
            ("2024-05-06T12:00:00+05", Some(1_714_978_800)),
            ("2024-05-06T00:00:00+23:59", Some(1_714_867_260)),
            ("2024-05-06T00:00:00+00:59", Some(1_714_950_060)),
            ("2024-02-29", Some(1_709_164_800)),
            ("2000-02-29", Some(951_782_400)),
            ("2023-02-28", Some(1_677_542_400)),
            ("2024-01-01", Some(1_704_067_200)),
            ("2024-12-31", Some(1_735_603_200)),
            ("2024-04-30", Some(1_714_435_200)),
            ("2024-05-06T23:59:59Z", Some(1_715_039_999)),
            ("2024-02-31", None),
            ("2023-02-29", None),
            ("1900-02-29", None),
            ("2024-04-31", None),
            ("2024-13-01", None),
            ("2024-00-10", None),
            ("2024-01-32", None),
            ("2024-05-00", None),
            ("2024-05x06", None),
            ("2024-05/06", None),
            ("2024/05-06", None),
            ("2024-05-06T24:00:00Z", None),
            ("2024-05-06T23:60:00Z", None),
            ("2024-05-06T23:59:60Z", None),
            ("2024-05-06T10:00:00.Z", None),
            ("2024-05-06T15:04:05+24:00", None),
            ("2024-05-06T15:04:05+00:60", None),
            ("2024-05-06T12:34:5é", None),
            ("2024-05-06T12:34:56+1é1", None),
            ("not a date", None),
            ("", None),
        ];
        for (raw, expect) in cases {
            let got = publish_date(raw).map(|date| date.unix_timestamp());
            assert_eq!(got, expect, "{raw}");
        }
        let compact = publish_date("2020-01-02T03:04:05-0530").unwrap();
        assert_eq!(compact.offset.hours, -5);
        assert_eq!(compact.offset.minutes, -30);
        let file = system_time_to_pdf(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        assert_eq!(file.unix_timestamp(), 1_700_000_000);
    }

    #[test]
    fn an_error_severity_warning_fails_and_an_empty_page_list_fails() {
        let failed = fail_on_layout_errors(&[printpdf::PdfWarnMsg {
            page: 0,
            op_id: 0,
            severity: printpdf::PdfParseErrorSeverity::Error,
            msg: "layout failed".to_string(),
        }]);
        let err = failed.unwrap_err();
        assert_eq!(err.to_string(), "failed to write the file: layout failed");
        assert_eq!(err.code(), 1);
        assert!(
            fail_on_layout_errors(&[printpdf::PdfWarnMsg::warning(
                0,
                0,
                "missing glyph".to_string()
            )])
            .is_ok()
        );
        let empty = require_pages(0).unwrap_err();
        assert_eq!(empty.code(), 1);
        assert_eq!(
            empty.to_string(),
            "failed to write the file: pdf layout produced no pages"
        );
        assert!(require_pages(1).is_ok());
    }
}
