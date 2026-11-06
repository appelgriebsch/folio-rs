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

/// `require_http_url` plus a host policy for public-facing services such as
/// the Cloudflare Worker: loopback, link-local (including the
/// 169.254.169.254 metadata address), private, CGNAT, multicast, unspecified,
/// or documentation IP ranges, and localhost-style domain names are rejected.
/// The private CLI keeps the looser `require_http_url`.
pub fn require_public_http_url(raw: &str) -> Result<url::Url, Error> {
    let url = require_http_url(raw)?;
    let host = url.host_str().unwrap_or_default();
    if !url.host().is_some_and(host_is_public) {
        return Err(Error::Usage(format!("url host is not public: {host}")));
    }
    Ok(url)
}

fn host_is_public(host: url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(domain) => {
            let lower = domain.to_ascii_lowercase();
            !(lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
                || lower.ends_with(".home.arpa"))
        }
        url::Host::Ipv4(ip) => ipv4_is_public(ip),
        url::Host::Ipv6(ip) => ipv6_is_public(ip),
    }
}

fn ipv4_is_public(ip: std::net::Ipv4Addr) -> bool {
    if ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
    {
        return false;
    }
    let octets = ip.octets();
    // 100.64.0.0/10 carrier-grade NAT is not covered by `is_private`.
    !(octets[0] == 100 && octets[1] & 0xc0 == 64)
}

fn ipv6_is_public(ip: std::net::Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    let segments = ip.segments();
    // fe80::/10 link-local and fc00::/7 unique-local.
    if segments[0] & 0xffc0 == 0xfe80 || segments[0] & 0xfe00 == 0xfc00 {
        return false;
    }
    // 2001:db8::/32 documentation.
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return false;
    }
    // ::ffff:0:0/96 IPv4-mapped, ::/96 IPv4-compatible, and 64:ff9b::/96
    // NAT64 all embed an IPv4 address in the last 32 bits; judge that.
    let embeds_ipv4 = (segments[..5].iter().all(|&seg| seg == 0) && segments[5] == 0xffff)
        || segments[..6].iter().all(|&seg| seg == 0)
        || (segments[0] == 0x0064
            && segments[1] == 0xff9b
            && segments[2..6].iter().all(|&seg| seg == 0));
    if embeds_ipv4 {
        let v4 = std::net::Ipv4Addr::from([
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        ]);
        return ipv4_is_public(v4);
    }
    true
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
    let file_name = file_name(&composed.title, req.format)?;
    let (title, bytes) = render_composed(composed, req.format).await?;
    Ok(Rendered {
        file_name,
        title,
        bytes,
        format: req.format,
    })
}

/// Render bytes for an already-composed article. The CLI uses this with an
/// explicit output path, where a title-based file name is never needed (and
/// must not fail the run for an unslugifiable title).
pub(crate) async fn render_composed(
    composed: Composed,
    format: Format,
) -> Result<(String, Vec<u8>), Error> {
    let title = composed.title.clone();
    let bytes = match format {
        Format::Epub => publish::epub_bytes(&composed)?,
        Format::Pdf => pdf_bytes(composed).await?,
    };
    Ok((title, bytes))
}

/// printpdf rendering is synchronous CPU work; keep it off the runtime thread.
#[cfg(not(target_arch = "wasm32"))]
async fn pdf_bytes(composed: Composed) -> Result<Vec<u8>, Error> {
    tokio::task::spawn_blocking(move || publish::pdf_bytes(&composed))
        .await
        .map_err(|err| Error::Unsupported(format!("pdf render task failed: {err}")))?
}

#[cfg(target_arch = "wasm32")]
async fn pdf_bytes(_composed: Composed) -> Result<Vec<u8>, Error> {
    Err(Error::Unsupported(
        "pdf is rendered by the worker's Browser Rendering binding".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{file_name, require_public_http_url};
    use crate::Format;

    #[test]
    fn require_public_http_url_rejects_non_public_hosts() {
        for raw in [
            "http://127.0.0.1/x",
            "http://[::1]/x",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.5/x",
            "http://192.168.1.1/x",
            "http://172.16.0.1/x",
            "http://100.64.0.1/x",
            "http://0.0.0.0/x",
            "http://[fe80::1]/x",
            "http://[fc00::1]/x",
            "http://[::ffff:127.0.0.1]/x",
            "http://[64:ff9b::10.0.0.5]/x",
            "http://localhost/x",
            "http://foo.localhost/x",
            "http://printer.local/x",
            "http://nas.internal/x",
            "ftp://example.com/x",
            "file:///etc/passwd",
        ] {
            assert!(
                require_public_http_url(raw).is_err(),
                "{raw} must be rejected"
            );
        }
    }

    #[test]
    fn require_public_http_url_accepts_public_hosts() {
        for raw in [
            "https://example.com/article",
            "http://8.8.8.8/x",
            "https://[2606:4700:4700::1111]/x",
        ] {
            assert!(
                require_public_http_url(raw).is_ok(),
                "{raw} must be accepted"
            );
        }
    }

    #[test]
    fn require_http_url_stays_loose_for_the_cli() {
        // The private CLI may fetch loopback/local test servers; only the
        // public Worker path applies `require_public_http_url`.
        for raw in ["http://127.0.0.1:8080/x", "http://localhost/x"] {
            assert!(
                super::require_http_url(raw).is_ok(),
                "{raw} must stay allowed"
            );
        }
        assert!(super::require_http_url("ftp://example.com/x").is_err());
    }

    #[test]
    fn file_name_slugs_the_title() {
        assert_eq!(
            file_name("Caf\u{e9} Story!", Format::Epub).unwrap(),
            "caf\u{e9}-story.epub"
        );
        assert!(file_name("!!!", Format::Pdf).is_err());
    }
}
