//! folio as a Cloudflare Worker.
//!
//! `GET /?sourceUrl=<article url>&format=epub|pdf` and the same fields as a
//! JSON `POST /` body return the generated file for download. EPUB is built
//! in-process; PDF is rendered from the composed article by the account's
//! Browser Rendering binding (`BROWSER`).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use folio_rs::error::Error;
use folio_rs::service::{self, Composed, RenderRequest};
use folio_rs::Format;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use worker::{Context, Env, Headers, Method, Request, Response, Result, RouteContext, Router, event};

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
    url: Option<String>,
    format: Option<String>,
}

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", handle)
        .post_async("/", handle)
        .get_async("/health", |_req, _ctx| async {
            Response::ok("ok")
        })
        .run(req, env)
        .await
}

async fn handle(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (source_url, format) = match req.method() {
        Method::Get => {
            let url = req.url()?;
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            let source_url = query
                .get("sourceUrl")
                .or_else(|| query.get("url"))
                .cloned();
            (source_url, query.get("format").cloned())
        }
        Method::Post => {
            let payload: Payload = req.json().await?;
            (payload.source_url.or(payload.url), payload.format)
        }
        _ => return json_error(405, "use GET with query parameters or POST a JSON body"),
    };
    let source_url = match source_url {
        Some(url) if !url.trim().is_empty() => url,
        _ => return json_error(400, "missing sourceUrl"),
    };
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
            Ok(rendered) => file_response(&rendered.bytes, &rendered.file_name, format, &rendered.title),
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
    let page = match JsFuture::from(browser.new_page()).await {
        Ok(page) => page.unchecked_into::<BrowserPage>(),
        Err(err) => return json_error(502, &format_js_error(&err)),
    };
    let result = render_page_pdf(&page, &html).await;
    let _ = JsFuture::from(page.close()).await;
    match result {
        Ok(bytes) => file_response(&bytes, &file_name, Format::Pdf, &composed.title),
        Err(err) => json_error(502, &err),
    }
}

async fn render_page_pdf(page: &BrowserPage, html: &str) -> std::result::Result<Vec<u8>, String> {
    JsFuture::from(page.set_content(html))
        .await
        .map_err(|err| format_js_error(&err))?;
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
    let value = JsFuture::from(page.pdf(&options.into()))
        .await
        .map_err(|err| format_js_error(&err))?;
    let array = js_sys::Uint8Array::new(&value);
    let mut bytes = vec![0u8; array.length() as usize];
    array.copy_to(&mut bytes);
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
        html = html.replace(&format!("src=\"{}\"", image.href), &format!("src=\"{data_uri}\""));
    }
    html
}

fn file_response(bytes: &[u8], file_name: &str, format: Format, title: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", format.media_type())?;
    headers.set(
        "Content-Disposition",
        &format!("attachment; filename=\"{file_name}\""),
    )?;
    if title.is_ascii() {
        headers.set("X-Folio-Title", title)?;
    }
    Ok(Response::from_bytes(bytes.to_vec())?.with_headers(headers))
}

fn json_error(status: u16, message: &str) -> Result<Response> {
    let body = serde_json::json!({ "error": message });
    let mut response = Response::from_json(&body)?.with_status(status);
    response.headers_mut().set("Content-Type", "application/json")?;
    Ok(response)
}

fn status_for(err: &Error) -> u16 {
    match err {
        Error::Usage(_) => 400,
        Error::NoArticle { .. } | Error::MissingTitle(_) => 422,
        Error::Fetch { .. } | Error::TooLarge(_) | Error::ArticleTooLarge(_) => 502,
        Error::Unsupported(_) => 501,
        _ => 500,
    }
}

fn format_js_error(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| js_sys::JSON::stringify(err).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "browser rendering call failed".to_string())
}
