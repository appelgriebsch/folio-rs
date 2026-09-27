use std::io::{IsTerminal, Read};
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use url::Url;

use crate::decode::decode_html;
use crate::error::Error;

pub const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;

pub struct Page {
    pub response_url: Url,
    pub html: String,
}

pub fn client() -> Result<Client, Error> {
    Client::builder()
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
        .user_agent("folio/0.1")
        .build()
        .map_err(|err| Error::Fetch {
            url: "client".to_string(),
            reason: err.to_string(),
        })
}

pub fn fetch_html(http: &Client, typed_url: &Url, verbose: bool) -> Result<Page, Error> {
    if verbose || std::io::stderr().is_terminal() {
        eprintln!("fetch {typed_url}");
    }
    let response = http
        .get(typed_url.clone())
        .send()
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
    let content_type = header_str(&response, reqwest::header::CONTENT_TYPE);
    let bytes = read_capped(response, MAX_HTML_BYTES).map_err(|err| {
        if err.kind() == std::io::ErrorKind::FileTooLarge {
            Error::TooLarge(response_url.to_string())
        } else {
            Error::Fetch {
                url: response_url.to_string(),
                reason: err.to_string(),
            }
        }
    })?;
    let html = decode_html(&bytes, content_type.as_deref());
    Ok(Page { response_url, html })
}

fn header_str(response: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn read_capped(mut reader: impl Read, max: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(buf);
        }
        if buf.len() + read > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "response exceeds the byte cap",
            ));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::read_capped;
    use std::io::{self, Read};

    struct FailRead;

    impl Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"))
        }
    }

    #[test]
    fn short_read_is_io_and_overflow_is_the_cap() {
        let err = read_capped(FailRead, 100).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);

        let err = read_capped(std::io::Cursor::new(vec![1u8, 2, 3, 4]), 3).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    }
}

fn fetch_err(url: &Url, err: reqwest::Error) -> Error {
    Error::Fetch {
        url: url.to_string(),
        reason: err.to_string(),
    }
}
