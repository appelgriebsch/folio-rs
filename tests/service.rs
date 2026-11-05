//! The web-service path: `service::render` turns one URL into file bytes in
//! memory, which is what the Cloudflare Worker returns in the HTTP response.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use folio_rs::service::{RenderRequest, render};
use folio_rs::Format;

const ARTICLE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Service Story</title>
<meta property="og:title" content="Service Story">
<meta name="author" content="Ada Lovelace">
</head>
<body>
<article>
<h1>Service Story</h1>
<p>An article body long enough for the reader to keep, with several words in it.</p>
</article>
</body>
</html>"#;

fn serve_once() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                ARTICLE.len(),
                ARTICLE
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}/story")
}

#[tokio::test]
async fn render_returns_epub_bytes_and_a_file_name() {
    let url = serve_once();
    let rendered = render(RenderRequest {
        url,
        format: Format::Epub,
        verbose: false,
    })
    .await
    .expect("render should succeed");
    assert_eq!(rendered.format, Format::Epub);
    assert_eq!(rendered.file_name, "service-story.epub");
    assert_eq!(rendered.title, "Service Story");
    assert!(rendered.bytes.starts_with(b"PK"), "an EPUB is a zip");
}

#[tokio::test]
async fn render_returns_pdf_bytes_on_native() {
    let url = serve_once();
    let rendered = render(RenderRequest {
        url,
        format: Format::Pdf,
        verbose: false,
    })
    .await
    .expect("render should succeed");
    assert_eq!(rendered.file_name, "service-story.pdf");
    assert!(rendered.bytes.starts_with(b"%PDF"), "a PDF starts with %PDF");
}

#[tokio::test]
async fn render_rejects_a_non_http_url() {
    let result = render(RenderRequest {
        url: "ftp://example.test/x".to_string(),
        format: Format::Epub,
        verbose: false,
    })
    .await;
    let err = result.err().expect("ftp is not a valid article url");
    assert_eq!(err.code(), 2);
}
