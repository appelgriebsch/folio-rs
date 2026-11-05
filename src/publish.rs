use std::io::{Cursor, Read, Write};

use dom_query::Document;
use rbook::Epub;
use rbook::ebook::toc::TocEntryKind;
use rbook::epub::EpubChapter;
use rbook::epub::manifest::DetachedEpubManifestEntry;
use rbook::epub::metadata::EpubVersion;
use rbook::epub::toc::DetachedEpubTocEntry;

use crate::cover::{self, Cover};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::article::{extract_article, fragment_has_text};
use crate::cli::{Format, Options};
use crate::error::Error;
use crate::fetch::{self, Page, client};
use crate::images::{self, ImageWarnings};
use crate::meta::{self, read_meta_doc};
use crate::output::{self, TempFile};
use crate::pdf::{self, PdfMeta};
use crate::reader::{self, ReaderScan};
use crate::sanitize;
use crate::toc::{self, Section};
use crate::xhtml;

struct Composed {
    title: String,
    meta: meta::PageMeta,
    xhtml: String,
    /// Sanitized HTML with image hrefs, before the EPUB XHTML wrapper.
    anchored: String,
    images: Vec<images::EmbeddedImage>,
    outline: Vec<Section>,
    /// Absent for PDF. That format has no cover and does not fetch share images.
    cover: Option<Cover>,
}

const MAX_COVER_CANDIDATES: usize = 3;

/// This ticket fetches one page. `max_pages` is validated by the CLI and kept for later tickets.
fn pages_fetched(max_pages: u8) -> u8 {
    let _ = max_pages;
    1
}

pub fn publish(opts: Options) -> Result<std::path::PathBuf, Error> {
    let _single_page = pages_fetched(opts.max_pages);
    let typed = crate::cli::require_http_url(&opts.url)?;
    let explicit = output::prepare_explicit_output(&opts)?;
    let format = output::agree_format(opts.format, explicit.as_deref())?;
    let http = client()?;
    let page = fetch::fetch_html(&http, &typed, opts.verbose)?;
    let composed = choose_source(&http, &page, &typed, format, opts.verbose)?;
    let output = match explicit {
        Some(path) => path,
        None => output::path_from_title(&composed.title, &opts)?,
    };
    let temp = TempFile::new(&output)?;
    match format {
        Format::Epub => write_epub(
            &temp,
            &composed.title,
            &composed.meta,
            &composed.xhtml,
            &composed.images,
            &composed.outline,
            composed
                .cover
                .as_ref()
                .expect("epub compose builds a cover"),
        )?,
        Format::Pdf => write_pdf(&temp, &composed)?,
    }
    temp.persist(&output)?;
    Ok(output)
}

fn write_pdf(temp: &TempFile, composed: &Composed) -> Result<(), Error> {
    let (body, images) = pdf::article_xhtml(&composed.anchored, &composed.images);
    let bytes = pdf::render(
        &body,
        &images,
        &PdfMeta {
            title: &composed.title,
            meta: &composed.meta,
            file_time: std::time::SystemTime::now(),
        },
    )?;
    std::fs::write(temp.path(), bytes).map_err(|err| Error::Write(err.to_string()))
}

fn choose_source(
    http: &reqwest::blocking::Client,
    page: &Page,
    typed: &url::Url,
    format: Format,
    verbose: bool,
) -> Result<Composed, Error> {
    let (scan, print, site_banners) = {
        let doc = Document::from(page.html.as_str());
        let meta = read_meta_doc(&doc, &page.response_url, typed);
        (
            reader::scan(&doc, &meta.base),
            reader::find_print_link(&doc, &meta.base),
            meta.banners,
        )
    };
    if verbose {
        eprintln!("{}", scan.verbose_line());
        if format == Format::Pdf {
            eprintln!("{}", reader::print_verbose_line(&print));
        }
    }
    // ponytail: issue #3 appends rel=next on the document this function returns
    if format == Format::Pdf
        && let reader::PrintScan::One(print_url) = print
    {
        match fetch::fetch_html(http, &print_url, verbose) {
            Ok(print_page) => match compose(http, &print_page, typed, &site_banners, format) {
                Ok(composed) => return Ok(composed),
                Err(err) => warn_print(&print_url, &err),
            },
            Err(err) => warn_print(&print_url, &err),
        }
    }
    choose_article(http, page, typed, &scan, &site_banners, verbose, format)
}

fn warn_print(print_url: &url::Url, err: &Error) {
    // The error's Display names the typed URL. The warning must name the print URL.
    let status = reader::failure_status(err).unwrap_or_else(|| match err {
        Error::MissingTitle(_) => "missing title".to_string(),
        _ => "failed".to_string(),
    });
    eprintln!("warning: print view failed ({print_url}, {status}); used the article page");
}

/// Issue #3 walks continuation pages on this document. It is not merged, so the
/// chosen page is the whole article.
fn choose_article(
    http: &reqwest::blocking::Client,
    page: &Page,
    typed: &url::Url,
    scan: &ReaderScan,
    site_banners: &[String],
    verbose: bool,
    format: Format,
) -> Result<Composed, Error> {
    match scan {
        ReaderScan::One(hit) => match fetch::fetch_html(http, &hit.url, verbose) {
            Ok(reader_page) => match compose(http, &reader_page, typed, site_banners, format) {
                Ok(composed) => Ok(composed),
                Err(err) => fallback(http, page, typed, &hit.url, err, format),
            },
            Err(err) => fallback(http, page, typed, &hit.url, err, format),
        },
        ReaderScan::None | ReaderScan::Several => compose(http, page, typed, &[], format),
    }
}

fn fallback(
    http: &reqwest::blocking::Client,
    page: &Page,
    typed: &url::Url,
    reader_url: &url::Url,
    err: Error,
    format: Format,
) -> Result<Composed, Error> {
    match reader::failure_status(&err) {
        Some(status) => {
            eprintln!(
                "warning: reader view failed ({reader_url}, {status}); used the article page"
            );
            compose(http, page, typed, &[], format)
        }
        None => Err(err),
    }
}

fn compose(
    http: &reqwest::blocking::Client,
    page: &Page,
    typed: &url::Url,
    site_banners: &[String],
    format: Format,
) -> Result<Composed, Error> {
    let doc = Document::from(page.html.as_str());
    let meta = read_meta_doc(&doc, &page.response_url, typed);
    images::recover_specials(&doc);
    let html = doc
        .tree
        .root()
        .try_html()
        .map(|value| value.to_string())
        .unwrap_or_else(|| page.html.clone());
    let article = extract_article(&html, &meta.base, typed.as_str())?;
    let title = meta
        .title
        .clone()
        .ok_or_else(|| Error::MissingTitle(typed.to_string()))?;
    let headed = crate::article::with_header(
        &article.html,
        &title,
        &meta.authors,
        meta.published.as_deref(),
    );
    let flattened = images::flatten_images(&headed, &meta.base);
    let cleaned = sanitize::clean(&flattened);
    if !fragment_has_text(&cleaned) {
        return Err(Error::NoArticle {
            url: typed.to_string(),
        });
    }
    let mut warnings = ImageWarnings::default();
    // PDF has no cover page, so share images are not fetched and not warned about.
    let cover = (format == Format::Epub).then(|| {
        let mut image = None;
        for url in cover_urls(site_banners, &meta.banners) {
            match images::download_cover(http, &url) {
                Some(decoded) => {
                    image = Some(decoded);
                    break;
                }
                None => warnings.push(&url),
            }
        }
        let host = typed.host_str().unwrap_or("");
        cover::make_cover(image, &title, host)
    });
    let (with_images, embedded) = images::download_images(&cleaned, http, &mut warnings);
    warnings.emit();
    let (anchored, outline) = toc::anchor_outline(&with_images);
    let xhtml = xhtml::to_xhtml(&anchored, &meta.language, &title);
    Ok(Composed {
        title,
        meta,
        xhtml,
        anchored,
        images: embedded,
        outline,
        cover,
    })
}

fn write_epub(
    temp: &TempFile,
    title: &str,
    meta: &crate::meta::PageMeta,
    xhtml_doc: &str,
    images: &[images::EmbeddedImage],
    outline: &[Section],
    cover: &Cover,
) -> Result<(), Error> {
    let mut editor = Epub::builder()
        .identifier(meta.identifier.as_str())
        .title(title)
        .language(meta.language.as_str())
        .modified_now();
    for author in &meta.authors {
        editor = editor.author(author.as_str());
    }
    if let Some(description) = &meta.description {
        editor = editor.description(description.as_str());
    }
    if let Some(published) = &meta.published {
        editor = editor.published_date(published.as_str());
    }
    for (index, image) in images.iter().enumerate() {
        editor = editor.resource(
            DetachedEpubManifestEntry::new(format!("img{}", index + 1))
                .href(image.href.as_str())
                .media_type(image.media_type.as_str())
                .content(image.bytes.clone()),
        );
    }
    editor = editor.cover_image(
        DetachedEpubManifestEntry::new("cover")
            .href("images/cover.jpg")
            .media_type("image/jpeg")
            .content(cover.bytes.clone()),
    );
    let cover_page = EpubChapter::unlisted("cover.xhtml")
        .with_manifest_entry(
            DetachedEpubManifestEntry::new("cover-page")
                .href("cover.xhtml")
                .property("svg"),
        )
        .xhtml(cover_document(cover.width, cover.height).into_bytes());
    let chapter = EpubChapter::new(title)
        .href("chapter.xhtml")
        .with_toc_entry(contents_entry(title, outline))
        .xhtml(xhtml_doc.as_bytes().to_vec());
    editor = editor.chapter([cover_page, chapter]);
    let mut epub = editor.build();
    epub.toc_mut().insert_root(
        TocEntryKind::Landmarks,
        EpubVersion::EPUB3,
        DetachedEpubTocEntry::new("Landmarks").children(
            DetachedEpubTocEntry::new("Cover")
                .kind(TocEntryKind::Cover)
                .href("cover.xhtml"),
        ),
    );
    let mut bytes = epub
        .write()
        .to_vec()
        .map_err(|err| Error::Write(err.to_string()))?;
    // rbook invents dc:date when the book has none. A missing article date stays absent.
    if meta.published.is_none() {
        bytes = strip_generated_dc_date(bytes)?;
    }
    std::fs::write(temp.path(), bytes).map_err(|err| Error::Write(err.to_string()))?;
    Ok(())
}

fn cover_urls(preferred: &[String], page: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for url in preferred.iter().chain(page) {
        if out.iter().any(|have| have == url) {
            continue;
        }
        out.push(url.clone());
        if out.len() == MAX_COVER_CANDIDATES {
            break;
        }
    }
    out
}

fn cover_document(width: u32, height: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Cover</title></head>
<body>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 {width} {height}" width="100%" height="100%" preserveAspectRatio="xMidYMid meet"><image width="{width}" height="{height}" xlink:href="images/cover.jpg"/></svg>
</body>
</html>
"#
    )
}

fn contents_entry(title: &str, outline: &[Section]) -> DetachedEpubTocEntry {
    let mut entry = DetachedEpubTocEntry::new(title).href("chapter.xhtml");
    for section in outline {
        entry = entry.children(section_entry(section));
    }
    entry
}

fn section_entry(section: &Section) -> DetachedEpubTocEntry {
    let href = format!("chapter.xhtml#{}", fragment(&section.id));
    let mut entry = DetachedEpubTocEntry::new(section.label.as_str()).href(href);
    for child in &section.children {
        entry = entry.children(section_entry(child));
    }
    entry
}

fn fragment(id: &str) -> String {
    let mut encoded = String::new();
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn strip_generated_dc_date(bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|err| Error::Write(err.to_string()))?;
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| Error::Write(err.to_string()))?;
        let name = file.name().to_string();
        let method = if name == "mimetype" {
            CompressionMethod::Stored
        } else {
            file.compression()
        };
        let mut body = Vec::new();
        file.read_to_end(&mut body)
            .map_err(|err| Error::Write(err.to_string()))?;
        if name.ends_with(".opf")
            && let Ok(text) = std::str::from_utf8(&body)
        {
            body = strip_dc_date_elements(text).into_bytes();
        }
        let options = SimpleFileOptions::default().compression_method(method);
        out.start_file(name, options)
            .map_err(|err| Error::Write(err.to_string()))?;
        out.write_all(&body)
            .map_err(|err| Error::Write(err.to_string()))?;
    }
    let cursor = out.finish().map_err(|err| Error::Write(err.to_string()))?;
    Ok(cursor.into_inner())
}

fn strip_dc_date_elements(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<dc:date") {
        let next = start + "<dc:date".len();
        let boundary = rest.as_bytes().get(next).copied();
        if boundary.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b':')
        {
            out.push_str(&rest[..=start]);
            rest = &rest[start + 1..];
            continue;
        }
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail.find("</dc:date>") {
            rest = &tail[end + "</dc:date>".len()..];
        } else if let Some(end) = tail.find("/>") {
            rest = &tail[end + 2..];
        } else {
            rest = &rest[start + 1..];
        }
    }
    out.push_str(rest);
    out
}
