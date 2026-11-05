//! Platform-neutral entry points for turning one article URL into EPUB bytes.
//!
//! The CLI writes the bytes to a file; the Cloudflare Worker returns them in
//! an HTTP response. PDF bytes are rendered by printpdf on native targets
//! only; on wasm32 the caller renders the composed XHTML with the Workers
//! Browser Rendering binding instead.

use crate::error::Error;
use crate::fetch;
use crate::format::Format;
use crate::publish;
use crate::slug::slug_from_title;

pub use crate::images::EmbeddedImage;
pub use crate::meta::PageMeta;
pub use crate::toc::Section;

/// One composed article: metadata plus every representation the formats need.
pub struct Composed {
    pub title: String,
    pub meta: PageMeta,
    /// EPUB XHTML document for the chapter.
    pub xhtml: String,
    /// Sanitized HTML with image hrefs, before the EPUB XHTML wrapper.
    pub anchored: String,
    pub images: Vec<EmbeddedImage>,
    pub outline: Vec<Section>,
    /// Absent for PDF. That format has no cover and does not fetch share images.
    pub(crate) cover: Option<crate::cover::Cover>,
}

/// A web request: which article, which format.
pub struct RenderRequest {
    pub url: String,
    pub format: Format,
    pub verbose: bool,
}

/// A finished file, ready to return or to write.
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub file_name: String,
    pub title: String,
    pub format: Format,
}

pub fn require_http_url(raw: &str) -> Result<url::Url, Error> {
    let url = url::Url::parse(raw).map_err(|_| Error::Usage(format!("invalid url: {raw}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(Error::Usage("url must use http or https".to_string()));
    }
    if url.host_str().is_none() {
        return Err(Error::Usage(format!("invalid url: {raw}")));
    }
    Ok(url)
}

/// Fetch and compose the article without writing any file.
pub async fn compose_url(req: &RenderRequest) -> Result<Composed, Error> {
    let typed = require_http_url(&req.url)?;
    let http = fetch::client()?;
    let page = fetch::fetch_html(&http, &typed, req.verbose).await?;
    publish::choose_source(&http, &page, &typed, req.format, req.verbose).await
}

/// `<slug>.<ext>` for the article title, the same name the CLI writes.
pub fn file_name(title: &str, format: Format) -> Result<String, Error> {
    let slug = slug_from_title(title).ok_or(Error::BadSlug)?;
    Ok(format!("{slug}.{}", format.extension()))
}

/// Fetch the article and produce the whole file in memory.
pub async fn render(req: RenderRequest) -> Result<Rendered, Error> {
    let composed = compose_url(&req).await?;
    let bytes = match req.format {
        Format::Epub => publish::epub_bytes(&composed)?,
        Format::Pdf => pdf_bytes(&composed)?,
    };
    Ok(Rendered {
        file_name: file_name(&composed.title, req.format)?,
        title: composed.title,
        bytes,
        format: req.format,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn pdf_bytes(composed: &Composed) -> Result<Vec<u8>, Error> {
    publish::pdf_bytes(composed)
}

#[cfg(target_arch = "wasm32")]
fn pdf_bytes(_composed: &Composed) -> Result<Vec<u8>, Error> {
    Err(Error::Unsupported(
        "pdf is rendered by the worker's Browser Rendering binding".to_string(),
    ))
}
