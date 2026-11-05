#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use reqwest::Client;
#[cfg(not(target_arch = "wasm32"))]
use reqwest::redirect::Policy;
use url::Url;

use crate::decode::decode_html;
use crate::error::Error;

pub const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_REDIRECTS: usize = 10;

pub struct Page {
    pub response_url: Url,
    pub html: String,
}

pub fn client() -> Result<Client, Error> {
    // Timeouts and the redirect policy hook exist on native clients only;
    // the wasm32 client runs inside the Fetch API, which applies its own
    // redirect and deadline handling.
    #[cfg(not(target_arch = "wasm32"))]
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(Policy::custom(|attempt| {
            let scheme = attempt.url().scheme();
            if scheme != "http" && scheme != "https" {
                return attempt.error("redirect target is not http or https");
            }
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            attempt.follow()
        }))
        .user_agent("folio/0.1");
    #[cfg(target_arch = "wasm32")]
    let builder = Client::builder().user_agent("folio/0.1");
    builder.build().map_err(|err| Error::Fetch {
        url: "client".to_string(),
        reason: err.to_string(),
    })
}

pub async fn fetch_html(http: &Client, typed_url: &Url, verbose: bool) -> Result<Page, Error> {
    if verbose || stderr_is_terminal() {
        crate::log_line(&format!("fetch {typed_url}"));
    }
    let response = http
        .get(typed_url.clone())
        .send()
        .await
        .map_err(|err| fetch_err(typed_url, err))?;
    let response_url = response.url().clone();
    if response_url.scheme() != "http" && response_url.scheme() != "https" {
        return Err(Error::Fetch {
            url: typed_url.to_string(),
            reason: "response url is not http or https".to_string(),
        });
    }
    if !response.status().is_success() {
        return Err(Error::Fetch {
            url: response_url.to_string(),
            reason: format!("HTTP {}", response.status()),
        });
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_HTML_BYTES as u64)
    {
        return Err(Error::TooLarge(response_url.to_string()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = read_capped(response, MAX_HTML_BYTES).await.map_err(|err| {
        if err == "cap" {
            Error::TooLarge(response_url.to_string())
        } else {
            Error::Fetch {
                url: response_url.to_string(),
                reason: err,
            }
        }
    })?;
    let html = decode_html(&bytes, content_type.as_deref());
    Ok(Page { response_url, html })
}

/// Native responses stream chunk by chunk so the cap stops the read early.
#[cfg(not(target_arch = "wasm32"))]
async fn read_capped(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        if exceeds_cap(buf.len(), chunk.len(), max) {
            return Err("cap".to_string());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// The wasm32 response has no chunk stream, so the body cannot be capped
/// mid-read. Fail closed unless the server declares a Content-Length within
/// the cap; a missing or oversized header is rejected before reading, so a
/// lying Content-Length cannot buffer past `max`.
#[cfg(target_arch = "wasm32")]
async fn read_capped(response: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    match response.content_length() {
        Some(len) if len <= max as u64 => {}
        Some(_) => return Err("cap".to_string()),
        None => return Err("missing content length".to_string()),
    }
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if exceeds_cap(0, bytes.len(), max) {
        return Err("cap".to_string());
    }
    Ok(bytes.to_vec())
}

fn exceeds_cap(buf_len: usize, chunk_len: usize, max: usize) -> bool {
    buf_len + chunk_len > max
}

fn fetch_err(url: &Url, err: reqwest::Error) -> Error {
    Error::Fetch {
        url: url.to_string(),
        reason: err.to_string(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stderr_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn stderr_is_terminal() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::exceeds_cap;

    #[test]
    fn cap_counts_the_incoming_chunk() {
        assert!(!exceeds_cap(0, 100, 100));
        assert!(exceeds_cap(1, 100, 100));
        assert!(exceeds_cap(0, 101, 100));
    }
}
