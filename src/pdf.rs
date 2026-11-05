//! One article, one PDF. The HTML is a small XHTML subset so `from_html` can parse it.
//!
//! A render fails the run when `from_html` returns `Err`, the page list is empty,
//! or any `PdfWarnMsg` has severity `Error`. Info and Warning stay on stderr
//! and the file is still written. `printpdf` emits Error for a layout or parse
//! failure and Warning for a missing glyph or a skipped image.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use dom_query::{Document, NodeRef};
use printpdf::date::{Date, DateTime, Offset, OffsetDateTime, Time};
use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfParseOptions, PdfSaveOptions};

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
    let mut doc = PdfDocument::from_html(&html, &image_map, &fonts, &options, &mut warnings)
        .map_err(Error::Write)?;
    require_pages(doc.pages.len())?;
    fail_on_layout_errors(&warnings)?;
    for warning in &warnings {
        if warning.severity == printpdf::PdfParseErrorSeverity::Warning {
            eprintln!("warning: pdf: {}", warning.msg);
        }
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

fn font_map() -> BTreeMap<String, Base64OrRaw> {
    let mut fonts = BTreeMap::new();
    fonts.insert(FAMILY.to_string(), Base64OrRaw::Raw(FONT_REGULAR.to_vec()));
    fonts.insert(
        format!("{FAMILY} Italic"),
        Base64OrRaw::Raw(FONT_ITALIC.to_vec()),
    );
    fonts.insert(
        format!("{FAMILY} Bold"),
        Base64OrRaw::Raw(FONT_BOLD.to_vec()),
    );
    fonts
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

/// Date-only publish dates are midnight UTC. A full timestamp keeps its offset.
/// Anything else is not a date.
pub fn publish_date(raw: &str) -> Option<OffsetDateTime> {
    let raw = raw.trim();
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
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = raw[0..4].parse().ok()?;
    let month: u8 = raw[5..7].parse().ok()?;
    let day: u8 = raw[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(((year, month, day), &raw[10..]))
}

fn split_clock(raw: &str) -> Option<(u8, u8, u8, &str)> {
    let bytes = raw.as_bytes();
    if bytes.len() < 8 || bytes[2] != b':' || bytes[5] != b':' {
        return None;
    }
    let hour: u8 = raw[0..2].parse().ok()?;
    let minute: u8 = raw[3..5].parse().ok()?;
    let second: u8 = raw[6..8].parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut tail = &raw[8..];
    if tail.starts_with('.') {
        let digits = tail[1..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        tail = &tail[1 + digits..];
    }
    Some((hour, minute, second, tail))
}

fn parse_offset(raw: &str) -> Option<i32> {
    let (sign, rest) = match raw.as_bytes().first()? {
        b'+' => (1, &raw[1..]),
        b'-' => (-1, &raw[1..]),
        _ => return None,
    };
    let (hours, minutes) = if let Some((hours, minutes)) = rest.split_once(':') {
        (hours, minutes)
    } else if rest.len() == 4 {
        rest.split_at(2)
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
    if !BLOCKS.contains(&name.as_str()) {
        for child in node.children() {
            write_node(&child, images, map, out);
        }
        return;
    }
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
    out.push_str(&name);
    out.push('>');
    for child in node.children() {
        write_node(&child, images, map, out);
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
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

/// Pull visible text back out of a saved PDF. Tests use this instead of poppler.
pub fn pdf_text(bytes: &[u8]) -> String {
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

pub fn pdf_info(bytes: &[u8]) -> printpdf::PdfDocumentInfo {
    let mut warnings = Vec::new();
    let doc = PdfDocument::parse(
        bytes,
        &PdfParseOptions {
            fail_on_error: false,
        },
        &mut warnings,
    )
    .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
    doc.metadata.info
}

pub fn pdf_font_names(bytes: &[u8]) -> Vec<String> {
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

pub fn pdf_has_image(bytes: &[u8]) -> bool {
    let mut warnings = Vec::new();
    let doc = PdfDocument::parse(
        bytes,
        &PdfParseOptions {
            fail_on_error: false,
        },
        &mut warnings,
    )
    .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
    !doc.resources.xobjects.map.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn date_only_is_midnight_utc_and_a_timestamp_keeps_its_offset() {
        let day = publish_date("2024-05-06").unwrap();
        assert_eq!(day.date.year, 2024);
        assert_eq!(day.date.month, 5);
        assert_eq!(day.date.day, 6);
        assert_eq!(day.time.hour, 0);
        assert_eq!(day.time.minute, 0);
        assert_eq!(day.time.second, 0);
        assert_eq!(day.offset.hours, 0);
        assert_eq!(day.offset.minutes, 0);
        assert_eq!(day.unix_timestamp(), 1_714_953_600);

        let zoned = publish_date("2024-05-06T15:04:05+02:00").unwrap();
        assert_eq!(zoned.time.hour, 15);
        assert_eq!(zoned.time.minute, 4);
        assert_eq!(zoned.time.second, 5);
        assert_eq!(zoned.offset.hours, 2);
        assert_eq!(zoned.offset.minutes, 0);
        assert_eq!(zoned.unix_timestamp(), 1_715_000_645);

        let compact = publish_date("2020-01-02T03:04:05-0530").unwrap();
        assert_eq!(compact.offset.hours, -5);
        assert_eq!(compact.offset.minutes, -30);

        let file = system_time_to_pdf(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        assert_eq!(file.unix_timestamp(), 1_700_000_000);
        assert!(publish_date("not a date").is_none());
        assert!(publish_date("").is_none());
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
