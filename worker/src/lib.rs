//! folio as a Cloudflare Worker.
//!
//! `GET /?sourceUrl=<article url>&format=epub|pdf` and the same fields as a
//! JSON `POST /` body return the generated file for download. EPUB is built
//! in-process; PDF is rendered from the composed article by the account's
//! Browser Rendering binding (`BROWSER`).

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use folio_rs::error::Error;
use folio_rs::service::{self, Composed, RenderRequest};
use folio_rs::Format;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use worker::{
    event, Context, Env, Headers, Method, Request, Response, Result, RouteContext, Router,
};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = js_sys::Object, js_name = "Browser")]
    type Browser;
    #[wasm_bindgen(method, js_name = newPage)]
    fn new_page(this: &Browser) -> js_sys::Promise;

    #[wasm_bindgen(extends = js_sys::Object, js_name = "Page")]
    type BrowserPage;
    #[wasm_bindgen(method, js_name = setContent)]
    fn set_content(this: &BrowserPage, html: &str) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn pdf(this: &BrowserPage, options: &JsValue) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn close(this: &BrowserPage) -> js_sys::Promise;
}

impl worker::EnvBinding for Browser {
    const TYPE_NAME: &'static str = "Browser";
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "sourceUrl")]
    source_url: Option<String>,
    format: Option<String>,
}

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", handle)
        .post_async("/", handle)
        .get_async("/health", |_req, _ctx| async { Response::ok("ok") })
        .run(req, env)
        .await
}

async fn handle(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (source_url, format) = match req.method() {
        Method::Get => {
            let url = req.url()?;
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            let source_url = query.get("sourceUrl").cloned();
            (source_url, query.get("format").cloned())
        }
        Method::Post => {
            let payload: Payload = match req.json().await {
                Ok(payload) => payload,
                Err(_) => return json_error(400, "body must be a JSON object with sourceUrl"),
            };
            (payload.source_url, payload.format)
        }
        _ => return json_error(405, "use GET with query parameters or POST a JSON body"),
    };
    let source_url = match source_url {
        Some(url) if !url.trim().is_empty() => url,
        _ => return json_error(400, "missing sourceUrl"),
    };
    if let Err(err) = service::require_public_http_url(&source_url) {
        return json_error(400, &err.to_string());
    }
    let format = match format.as_deref() {
        Some(word) => match Format::parse(word) {
            Some(format) => format,
            None => return json_error(400, "format must be epub or pdf"),
        },
        None => Format::Epub,
    };
    let request = RenderRequest {
        url: source_url,
        format,
        verbose: false,
    };
    match format {
        Format::Epub => match service::render(request).await {
            Ok(rendered) => file_response(
                &rendered.bytes,
                &rendered.file_name,
                format,
                &rendered.title,
            ),
            Err(err) => json_error(status_for(&err), &err.to_string()),
        },
        Format::Pdf => pdf_via_browser(request, &ctx.env).await,
    }
}

async fn pdf_via_browser(request: RenderRequest, env: &Env) -> Result<Response> {
    let composed = match service::compose_url(&request).await {
        Ok(composed) => composed,
        Err(err) => return json_error(status_for(&err), &err.to_string()),
    };
    let file_name = match service::file_name(&composed.title, Format::Pdf) {
        Ok(name) => name,
        Err(err) => return json_error(status_for(&err), &err.to_string()),
    };
    let html = browser_html(&composed);
    let browser: Browser = match env.get_binding("BROWSER") {
        Ok(browser) => browser,
        Err(_) => {
            return json_error(
                501,
                "BROWSER binding is not configured; enable Browser Rendering on the account",
            )
        }
    };
    if html.len() > MAX_RENDER_HTML_BYTES {
        return json_error(502, "composed article is too large for browser rendering");
    }
    let page = match await_bounded(browser.new_page()).await {
        Ok(page) => page.unchecked_into::<BrowserPage>(),
        Err(err) => return json_error(502, &err),
    };
    let result = render_page_pdf(&page, &html).await;
    let _ = JsFuture::from(page.close()).await;
    match result {
        Ok(bytes) => file_response(&bytes, &file_name, Format::Pdf, &composed.title),
        Err(err) => json_error(502, &err),
    }
}

async fn render_page_pdf(page: &BrowserPage, html: &str) -> std::result::Result<Vec<u8>, String> {
    await_bounded(page.set_content(html)).await?;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"format".into(), &"A4".into())
        .map_err(|err| format_js_error(&err))?;
    js_sys::Reflect::set(&options, &"printBackground".into(), &JsValue::TRUE)
        .map_err(|err| format_js_error(&err))?;
    let margin = js_sys::Object::new();
    for side in ["top", "right", "bottom", "left"] {
        js_sys::Reflect::set(&margin, &side.into(), &"20mm".into())
            .map_err(|err| format_js_error(&err))?;
    }
    js_sys::Reflect::set(&options, &"margin".into(), &margin)
        .map_err(|err| format_js_error(&err))?;
    let value = await_bounded(page.pdf(&options.into())).await?;
    let array = js_sys::Uint8Array::new(&value);
    let mut bytes = vec![0u8; array.length() as usize];
    array.copy_to(&mut bytes);
    if bytes.len() > MAX_PDF_BYTES {
        return Err("rendered pdf exceeds the byte cap".to_string());
    }
    Ok(bytes)
}

/// The EPUB XHTML references images as `images/img-N.ext`; inline each as a
/// data URI so the headless browser needs no network access.
fn browser_html(composed: &Composed) -> String {
    let mut html = composed.xhtml.clone();
    for image in &composed.images {
        let data_uri = format!(
            "data:{};base64,{}",
            image.media_type,
            BASE64.encode(&image.bytes)
        );
        html = html.replace(
            &format!("src=\"{}\"", image.href),
            &format!("src=\"{data_uri}\""),
        );
    }
    html
}

fn file_response(bytes: &[u8], file_name: &str, format: Format, title: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", format.media_type())?;
    headers.set("Content-Disposition", &content_disposition(file_name))?;
    if title.chars().all(|ch| ch.is_ascii() && !ch.is_control()) {
        headers.set("X-Folio-Title", title)?;
    }
    Ok(Response::from_bytes(bytes.to_vec())?.with_headers(headers))
}

/// RFC 5987: an ASCII-safe `filename` fallback plus `filename*` with the
/// UTF-8 percent-encoded name, so non-ASCII slugs survive the download.
fn content_disposition(file_name: &str) -> String {
    let ascii: String = file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii() && ch != '"' && ch != '\\' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        percent_encode_attr(file_name)
    )
}

fn percent_encode_attr(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || "!#$&+-.^_`|~".contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn json_error(status: u16, message: &str) -> Result<Response> {
    let body = serde_json::json!({ "error": message });
    let mut response = Response::from_json(&body)?.with_status(status);
    response
        .headers_mut()
        .set("Content-Type", "application/json")?;
    Ok(response)
}

fn status_for(err: &Error) -> u16 {
    match err {
        Error::Usage(_) => 400,
        Error::NoArticle { .. } | Error::MissingTitle(_) | Error::BadSlug => 422,
        Error::Fetch { .. } | Error::TooLarge(_) | Error::ArticleTooLarge(_) => 502,
        Error::Unsupported(_) => 501,
        _ => 500,
    }
}

const BROWSER_RENDER_TIMEOUT_MS: u32 = 30_000;
const MAX_RENDER_HTML_BYTES: usize = 16 * 1024 * 1024;
const MAX_PDF_BYTES: usize = 32 * 1024 * 1024;

/// Race a Browser Rendering promise against a timer so a hung page cannot
/// pin the Worker. When the runtime has no setTimeout, the promise is
/// returned unchanged.
fn with_timeout(promise: js_sys::Promise, millis: u32) -> js_sys::Promise {
    let global = js_sys::global();
    let Ok(set_timeout) = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout")) else {
        return promise;
    };
    let set_timeout: &js_sys::Function = set_timeout.unchecked_ref();
    let timeout = js_sys::Promise::new(&mut |_resolve, reject| {
        let reject: js_sys::Function = reject.unchecked_into();
        let on_elapsed = wasm_bindgen::closure::Closure::once_into_js(move |_: JsValue| {
            let _ = reject.call1(
                &JsValue::NULL,
                &JsValue::from_str("browser rendering timed out"),
            );
        });
        let _ = set_timeout.call2(&global, &on_elapsed, &JsValue::from(millis));
    });
    js_sys::Promise::race(&js_sys::Array::of2(&promise, &timeout))
}

async fn await_bounded(promise: js_sys::Promise) -> std::result::Result<JsValue, String> {
    JsFuture::from(with_timeout(promise, BROWSER_RENDER_TIMEOUT_MS))
        .await
        .map_err(|err| format_js_error(&err))
}

fn format_js_error(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| {
            js_sys::JSON::stringify(err)
                .ok()
                .and_then(|s| s.as_string())
        })
        .unwrap_or_else(|| "browser rendering call failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::{content_disposition, status_for, Payload};
    use folio_rs::error::Error;

    #[test]
    fn status_for_maps_errors_to_statuses() {
        assert_eq!(status_for(&Error::Usage("x".to_string())), 400);
        assert_eq!(status_for(&Error::BadSlug), 422);
        assert_eq!(status_for(&Error::MissingTitle("u".to_string())), 422);
        assert_eq!(
            status_for(&Error::NoArticle {
                url: "u".to_string()
            }),
            422
        );
        assert_eq!(
            status_for(&Error::Fetch {
                url: "u".to_string(),
                reason: "r".to_string()
            }),
            502
        );
        assert_eq!(status_for(&Error::TooLarge("u".to_string())), 502);
        assert_eq!(status_for(&Error::Unsupported("x".to_string())), 501);
        assert_eq!(status_for(&Error::Write("x".to_string())), 500);
    }

    #[test]
    fn content_disposition_keeps_ascii_names_and_adds_rfc5987() {
        let header = content_disposition("service-story.epub");
        assert!(
            header.contains("filename=\"service-story.epub\""),
            "{header}"
        );
        assert!(
            header.contains("filename*=UTF-8''service-story.epub"),
            "{header}"
        );
    }

    #[test]
    fn content_disposition_percent_encodes_non_ascii_slugs() {
        let header = content_disposition("caf\u{e9}-story.epub");
        assert!(header.contains("filename=\"caf_-story.epub\""), "{header}");
        assert!(
            header.contains("filename*=UTF-8''caf%C3%A9-story.epub"),
            "{header}"
        );
    }

    #[test]
    fn payload_takes_source_url_only() {
        let payload: Payload =
            serde_json::from_str(r#"{"sourceUrl":"https://example.com/a","format":"pdf"}"#)
                .unwrap();
        assert_eq!(payload.source_url.as_deref(), Some("https://example.com/a"));
        assert_eq!(payload.format.as_deref(), Some("pdf"));
        let payload: Payload = serde_json::from_str(r#"{"url":"https://example.com/a"}"#).unwrap();
        assert!(payload.source_url.is_none());
    }
}
