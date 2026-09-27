use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use image::ImageEncoder;
use rbook::Epub;

struct Server {
    base: String,
    html_hits: Arc<AtomicUsize>,
    image_hits: Arc<AtomicUsize>,
    miss_hits: Arc<AtomicUsize>,
    png: Vec<u8>,
}

impl Server {
    fn start() -> Self {
        let png = tiny_png();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let html_hits = Arc::new(AtomicUsize::new(0));
        let image_hits = Arc::new(AtomicUsize::new(0));
        let miss_hits = Arc::new(AtomicUsize::new(0));
        let html_hits_t = Arc::clone(&html_hits);
        let image_hits_t = Arc::clone(&image_hits);
        let miss_hits_t = Arc::clone(&miss_hits);
        let png_t = png.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = handle(stream, &html_hits_t, &image_hits_t, &miss_hits_t, &png_t);
            }
        });
        Self {
            base: format!("http://{addr}"),
            html_hits,
            image_hits,
            miss_hits,
            png,
        }
    }
}

fn handle(
    mut stream: TcpStream,
    html_hits: &AtomicUsize,
    image_hits: &AtomicUsize,
    miss_hits: &AtomicUsize,
    png: &[u8],
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let rev = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("rev="))
        .unwrap_or("1");
    let title_query = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("title="))
        .map(percent_decode);
    let body = match path {
        "/photo.png" => {
            image_hits.fetch_add(1, Ordering::SeqCst);
            return respond(&mut stream, "200 OK", "image/png", png);
        }
        "/article" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            article_html(
                &host_of(&req),
                title_query.as_deref().unwrap_or("Article Headline"),
                rev,
                true,
            )
        }
        "/no-date" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            article_html(
                &host_of(&req),
                title_query.as_deref().unwrap_or("Undated Story"),
                rev,
                false,
            )
        }
        "/no-title" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            long_page(
                "<html><head></head><body><article>",
                "</article></body></html>",
            )
        }
        "/script" | "/empty-reader" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            "<html><head><title>Shell</title></head><body><script>document.write('hidden')</script></body></html>"
                .to_string()
        }
        "/one-reader" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Typed Headline",
                "TYPED_PAGE_MARKER",
                "typed-canonical",
                "de",
                r#"<link rel="reader" href="/reader-doc">"#,
                "",
                false,
            )
        }
        "/reader-doc" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Reader Headline",
                "READER_PAGE_MARKER",
                "reader-canonical",
                "en",
                "",
                r#"<p><a rel="reader" href="/second-hop">Elsewhere</a></p>"#,
                true,
            )
        }
        "/second-hop" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Second Hop",
                "SECOND_HOP_MARKER",
                "second-canonical",
                "en",
                "",
                "",
                false,
            )
        }
        "/several-readers" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Several Headline",
                "SEVERAL_PAGE_MARKER",
                "several-canonical",
                "de",
                r#"<link rel="reader" href="/reader-a"><link rel="readmode" href="/reader-b">"#,
                "",
                false,
            )
        }
        "/reader-miss" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Fallback Headline",
                "FALLBACK_PAGE_MARKER",
                "fallback-canonical",
                "de",
                r#"<link rel="reader" href="/missing-reader">"#,
                "",
                false,
            )
        }
        "/reader-empty" => {
            html_hits.fetch_add(1, Ordering::SeqCst);
            reader_fixture(
                &host_of(&req),
                "Empty Fallback Headline",
                "EMPTY_FALLBACK_MARKER",
                "empty-fallback-canonical",
                "de",
                r#"<link rel="reader" href="/empty-reader">"#,
                "",
                false,
            )
        }
        _ => {
            miss_hits.fetch_add(1, Ordering::SeqCst);
            return respond(&mut stream, "404 Not Found", "text/plain", b"missing");
        }
    };
    respond(
        &mut stream,
        "200 OK",
        "text/html; charset=utf-8",
        body.as_bytes(),
    )
}

fn host_of(req: &str) -> String {
    req.lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                if name.eq_ignore_ascii_case("host") {
                    Some(value.trim().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn article_html(host: &str, title: &str, rev: &str, with_date: bool) -> String {
    let date = if with_date {
        r#"
            <script type="application/ld+json">
              {"@type":"WebPage","headline":"Web headline","datePublished":"2010-01-01"}
            </script>
            <script type="application/ld+json">
              {"@type":"NewsArticle","headline":"Article Headline","datePublished":"2024-05-06",
               "author":{"@type":"Person","name":"Ada Lovelace"},"description":"From json-ld"}
            </script>
            <meta property="article:published_time" content="1999-01-01T00:00:00Z">
            <meta name="citation_publication_date" content="1998/1/1">
            <meta name="DC.issued" content="1997-01-01">
        "#
    } else {
        ""
    };
    let headline = if with_date {
        "Article Headline".to_string()
    } else {
        title.to_string()
    };
    format!(
        r#"<!DOCTYPE html>
        <html lang="de">
        <head>
          <meta charset="utf-8">
          <title>Site title should lose</title>
          <meta property="og:title" content="{headline}">
          {date}
          <link rel="canonical" href="http://{host}/canonical-article">
          <meta property="og:description" content="og description">
          <meta property="og:image" content="http://{host}/photo.png">
        </head>
        <body>
          <article>
            <p>FOLIO_ARTICLE_TEXT rev {rev}. The quick brown fox reads this folio article about rust, epub export, and fish &amp; chips. It keeps going so the extractor has a real paragraph to score, with commas, and enough words to look like the story itself.</p>
            <p>Second paragraph of the same article, still about the folio export, with more sentences so readability keeps this node and not the chrome around it. The photo sits beside the copy.</p>
            <h2 id="kept-section">Kept section</h2>
            <p>Third paragraph continues the account of how a single page becomes one file, without fetching another page and without running a script.</p>
            <h3>Nested part</h3>
            <img src="/photo.png" alt="folio photo">
            <script>SECRET_SCRIPT_PAYLOAD</script>
            <iframe src="https://evil.example/frame">SECRET_IFRAME_PAYLOAD</iframe>
            <audio src="/a.mp3">SECRET_AUDIO_PAYLOAD</audio>
            <video src="/v.mp4">SECRET_VIDEO_PAYLOAD</video>
          </article>
        </body>
        </html>"#
    )
}

fn reader_fixture(
    host: &str,
    title: &str,
    marker: &str,
    canonical: &str,
    lang: &str,
    extra_head: &str,
    extra_body: &str,
    with_reader_meta: bool,
) -> String {
    let meta = if with_reader_meta {
        r#"<script type="application/ld+json">
              {"@type":"NewsArticle","headline":"Reader Headline","datePublished":"2020-02-03",
               "author":{"@type":"Person","name":"Grace Hopper"},"description":"From the reader"}
            </script>"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html>
        <html lang="{lang}">
        <head>
          <meta charset="utf-8">
          <title>{title}</title>
          <link rel="canonical" href="http://{host}/{canonical}">
          {meta}
          {extra_head}
        </head>
        <body>
          {extra_body}
          <article>
            <p>{marker}. The quick brown fox reads this folio article about rust, epub export, and fish and chips. It keeps going so the extractor has a real paragraph to score, with commas, and enough words to look like the story itself.</p>
            <p>Second paragraph of the same article, still about the folio export, with more sentences so readability keeps this node and not the chrome around it.</p>
            <p>Third paragraph continues the account of how a single page becomes one file, without fetching another page and without running a script.</p>
          </article>
        </body>
        </html>"#
    )
}

fn long_page(prefix: &str, suffix: &str) -> String {
    let mut body = String::from(prefix);
    for i in 0..8 {
        body.push_str(&format!(
            "<p>Paragraph {i} has enough words, commas, and sentences to be an article without a title element anywhere in the head.</p>"
        ));
    }
    body.push_str(suffix);
    body
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn tiny_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbImage::from_fn(8, 8, |x, y| {
        image::Rgb([(x * 20) as u8, 40, (y * 20) as u8])
    });
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(image.as_raw(), 8, 8, image::ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

fn workdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "folio-rs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}

fn folio(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_folio"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    Output {
        status: out.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn open_epub(path: &Path) -> Epub {
    Epub::open(path).unwrap_or_else(|err| panic!("rbook failed to open {}: {err}", path.display()))
}

fn resource_text(epub: &Epub) -> String {
    epub.manifest()
        .iter()
        .filter_map(|entry| entry.read_str().ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn resource_bytes(epub: &Epub) -> Vec<Vec<u8>> {
    epub.manifest()
        .iter()
        .filter_map(|entry| entry.read_bytes().ok())
        .collect()
}

#[test]
fn writes_epub_with_metadata_image_and_without_active_content() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/article", server.base);
    let out = folio(&dir, &["-v", "--max-pages", "15", &url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert!(
        out.stderr.contains(&format!("fetch {url}")),
        "{}",
        out.stderr
    );
    let path = dir.join("article-headline.epub");
    assert_eq!(out.stdout, format!("./article-headline.epub\n"));
    assert!(path.is_file());
    let epub = open_epub(&path);
    assert_eq!(epub.metadata().title().unwrap().value(), "Article Headline");
    assert_eq!(
        epub.metadata().identifier().unwrap().value(),
        format!("{}/canonical-article", server.base)
    );
    let language = epub
        .metadata()
        .languages()
        .next()
        .unwrap()
        .value()
        .to_string();
    assert_eq!(language, "de");
    let published = epub.metadata().published().unwrap();
    assert_eq!(published.date().year(), 2024);
    assert_eq!(published.date().month(), 5);
    assert_eq!(published.date().day(), 6);
    let creators: Vec<_> = epub
        .metadata()
        .creators()
        .map(|c| c.value().to_string())
        .collect();
    assert_eq!(creators, vec!["Ada Lovelace".to_string()]);
    let text = resource_text(&epub);
    assert!(text.contains("FOLIO_ARTICLE_TEXT"), "{text}");
    assert!(
        text.contains("fish &amp; chips") || text.contains("fish & chips"),
        "{text}"
    );
    assert!(!text.contains("SECRET_SCRIPT_PAYLOAD"), "{text}");
    assert!(!text.contains("SECRET_IFRAME_PAYLOAD"), "{text}");
    assert!(!text.contains("SECRET_AUDIO_PAYLOAD"), "{text}");
    assert!(!text.contains("SECRET_VIDEO_PAYLOAD"), "{text}");
    assert!(
        resource_bytes(&epub)
            .iter()
            .any(|bytes| bytes == &server.png)
    );
    assert!(server.image_hits.load(Ordering::SeqCst) >= 1);
    let cover = epub.manifest().cover_image().expect("banner is the cover");
    assert_eq!(cover.read_bytes().unwrap(), server.png);
    let toc = epub.toc().contents().unwrap();
    let chapter = toc.get(0).unwrap();
    assert_eq!(chapter.label(), "Article Headline");
    assert_eq!(chapter.len(), 1, "section entries");
    let section = chapter.get(0).unwrap();
    assert_eq!(section.label(), "Kept section");
    let section_href = section.href_raw().unwrap().to_string();
    assert!(
        section_href.contains("chapter.xhtml#kept-section"),
        "{section_href}"
    );
    let nested = section.get(0).unwrap();
    assert_eq!(nested.label(), "Nested part");
    assert!(nested.href_raw().unwrap().to_string().contains('#'));
    assert!(nested.is_empty());
}

#[test]
fn missing_date_is_silent_and_omitted() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/no-date", server.base);
    let out = folio(&dir, &["-o", "story.epub", &url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert!(
        !out.stderr.to_ascii_lowercase().contains("date"),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("warning:"), "{}", out.stderr);
    let epub = open_epub(&dir.join("story.epub"));
    assert!(epub.metadata().published().is_none());
    assert_eq!(epub.metadata().title().unwrap().value(), "Undated Story");
    assert_eq!(epub.metadata().languages().next().unwrap().value(), "de");
    let opf = package_xml(&epub);
    assert!(!opf.contains("<dc:date"), "{opf}");
    assert!(opf.contains("dc:title"), "{opf}");
    assert!(opf.contains("dc:identifier"), "{opf}");
    assert!(opf.contains("dc:language"), "{opf}");
}

fn package_xml(epub: &Epub) -> String {
    epub.read_resource_str(epub.package().location().as_str())
        .unwrap_or_default()
}

#[test]
fn missing_title_writes_nothing() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/no-title", server.base);
    let out = folio(&dir, &["-o", "missing.epub", &url]);
    assert_eq!(out.status, 1, "stderr={}", out.stderr);
    assert!(
        out.stderr.contains("error: missing title"),
        "{}",
        out.stderr
    );
    assert!(!dir.join("missing.epub").exists());
    assert_eq!(out.stdout, "");
}

#[test]
fn script_shell_explains_that_html_is_not_executed() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/script", server.base);
    let out = folio(&dir, &["-o", "shell.epub", &url]);
    assert_eq!(out.status, 1, "stderr={}", out.stderr);
    assert!(
        out.stderr
            .contains(&format!("error: no article text at {url}")),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("only reads the fetched HTML"),
        "{}",
        out.stderr
    );
    assert!(!dir.join("shell.epub").exists());
}

#[test]
fn existing_output_without_force_is_unchanged() {
    let server = Server::start();
    let dir = workdir();
    let first = format!("{}/article?rev=1", server.base);
    let second = format!("{}/article?rev=2", server.base);
    let out = folio(&dir, &["-o", "out.epub", &first]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    let bytes = std::fs::read(dir.join("out.epub")).unwrap();
    let images = server.image_hits.load(Ordering::SeqCst);
    let again = folio(&dir, &["-o", "out.epub", &second]);
    assert_eq!(again.status, 1, "stderr={}", again.stderr);
    assert!(again.stderr.contains("error:"), "{}", again.stderr);
    assert_eq!(std::fs::read(dir.join("out.epub")).unwrap(), bytes);
    assert_eq!(server.image_hits.load(Ordering::SeqCst), images);
    let replaced = folio(&dir, &["--force", "-o", "out.epub", &second]);
    assert_eq!(replaced.status, 0, "stderr={}", replaced.stderr);
    let epub = open_epub(&dir.join("out.epub"));
    let text = resource_text(&epub);
    assert!(text.contains("rev 2"), "{text}");
    assert!(!text.contains("rev 1"), "{text}");
}

#[test]
fn force_does_not_skip_a_missing_title() {
    let server = Server::start();
    let dir = workdir();
    let path = dir.join("kept.epub");
    std::fs::write(&path, b"original").unwrap();
    let url = format!("{}/no-title", server.base);
    let out = folio(&dir, &["--force", "-o", "kept.epub", &url]);
    assert_eq!(out.status, 1, "stderr={}", out.stderr);
    assert!(out.stderr.contains("missing title"), "{}", out.stderr);
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
}

#[test]
fn usage_failures_exit_2_before_fetch() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/article", server.base);
    let before = server.html_hits.load(Ordering::SeqCst);
    for args in [
        vec!["--max-pages", "0", url.as_str()],
        vec!["--max-pages", "16", url.as_str()],
        vec!["--format", "pdf", url.as_str()],
        vec!["-o", "-", url.as_str()],
        vec!["-o", "story.pdf", url.as_str()],
        vec!["-format", url.as_str()],
    ] {
        let out = folio(&dir, &args);
        assert_eq!(out.status, 2, "args={args:?} stderr={}", out.stderr);
        assert!(out.stdout.is_empty(), "{args:?}");
    }
    assert_eq!(server.html_hits.load(Ordering::SeqCst), before);
    assert!(!dir.join("story.pdf").exists());
    assert!(!dir.join("rmat").exists());
    let dashed = folio(&dir, &["-format", &url]);
    assert!(
        dashed
            .stderr
            .contains("error: --format takes two dashes; -f is --force"),
        "{}",
        dashed.stderr
    );

    let bare = folio(&dir, &[]);
    assert_eq!(bare.status, 2);

    let bad = folio(&dir, &["ftp://example.com/a"]);
    assert_eq!(bad.status, 2, "{}", bad.stderr);
    assert!(bad.stderr.contains("error:"), "{}", bad.stderr);
}

#[test]
fn refuses_missing_parent_directory_and_device_slug() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/no-date?title=CON", server.base);
    let out = folio(&dir, &[&url]);
    assert_eq!(out.status, 1, "stderr={}", out.stderr);
    assert!(out.stderr.contains("pass -o"), "{}", out.stderr);
    assert_eq!(out.stdout, "");

    let nested = folio(
        &dir,
        &[
            "-o",
            "missing/dir/book.epub",
            &format!("{}/article", server.base),
        ],
    );
    assert_eq!(nested.status, 1, "stderr={}", nested.stderr);
    assert!(nested.stderr.contains("error:"), "{}", nested.stderr);
    assert!(!dir.join("missing").exists());

    std::fs::create_dir(dir.join("a-dir")).unwrap();
    let as_dir = folio(&dir, &["-o", "a-dir", &format!("{}/article", server.base)]);
    assert_eq!(as_dir.status, 1, "stderr={}", as_dir.stderr);

    std::fs::write(dir.join("notdir"), b"file").unwrap();
    let before = server.html_hits.load(Ordering::SeqCst);
    let parent_file = folio(
        &dir,
        &[
            "-o",
            "notdir/book.epub",
            &format!("{}/article", server.base),
        ],
    );
    assert_eq!(parent_file.status, 1, "stderr={}", parent_file.stderr);
    assert!(
        parent_file
            .stderr
            .contains("error: output parent is not a directory: notdir"),
        "{}",
        parent_file.stderr
    );
    assert_eq!(server.html_hits.load(Ordering::SeqCst), before);
}

#[test]
fn one_reader_link_builds_the_epub_from_that_document() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/one-reader", server.base);
    let before = server.html_hits.load(Ordering::SeqCst);
    let out = folio(&dir, &["-v", "-o", "reader.epub", &url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert!(
        out.stderr
            .contains(&format!("reader view: {}/reader-doc (rel)", server.base)),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("warning:"), "{}", out.stderr);
    assert_eq!(server.html_hits.load(Ordering::SeqCst), before + 2);
    assert_eq!(server.miss_hits.load(Ordering::SeqCst), 0);
    let epub = open_epub(&dir.join("reader.epub"));
    let text = resource_text(&epub);
    assert!(text.contains("READER_PAGE_MARKER"), "{text}");
    assert!(!text.contains("TYPED_PAGE_MARKER"), "{text}");
    assert!(!text.contains("SECOND_HOP_MARKER"), "{text}");
    assert_eq!(epub.metadata().title().unwrap().value(), "Reader Headline");
    assert_eq!(
        epub.metadata().identifier().unwrap().value(),
        format!("{}/reader-canonical", server.base)
    );
    assert_eq!(epub.metadata().languages().next().unwrap().value(), "en");
    let published = epub.metadata().published().unwrap();
    assert_eq!(published.date().year(), 2020);
    assert_eq!(published.date().month(), 2);
    assert_eq!(published.date().day(), 3);
    let creators: Vec<_> = epub
        .metadata()
        .creators()
        .map(|creator| creator.value().to_string())
        .collect();
    assert_eq!(creators, vec!["Grace Hopper".to_string()]);
}

#[test]
fn several_reader_links_keep_the_typed_page_without_a_warning() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/several-readers", server.base);
    let before = server.html_hits.load(Ordering::SeqCst);
    let out = folio(&dir, &["-v", "-o", "several.epub", &url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert!(
        out.stderr
            .contains("reader view: kept the article page (several reader links)"),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("warning:"), "{}", out.stderr);
    assert_eq!(server.html_hits.load(Ordering::SeqCst), before + 1);
    assert_eq!(server.miss_hits.load(Ordering::SeqCst), 0);
    let epub = open_epub(&dir.join("several.epub"));
    let text = resource_text(&epub);
    assert!(text.contains("SEVERAL_PAGE_MARKER"), "{text}");
    assert_eq!(epub.metadata().title().unwrap().value(), "Several Headline");
    assert_eq!(
        epub.metadata().identifier().unwrap().value(),
        format!("{}/several-canonical", server.base)
    );
}

#[test]
fn failed_reader_fetch_warns_and_uses_the_typed_page() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/reader-miss", server.base);
    let reader = format!("{}/missing-reader", server.base);
    let out = folio(&dir, &["-v", "-o", "fallback.epub", &url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert!(
        out.stderr.contains(&format!("reader view: {reader} (rel)")),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains(&format!(
            "warning: reader view failed ({reader}, 404); used the article page"
        )),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("error:"), "{}", out.stderr);
    assert_eq!(server.miss_hits.load(Ordering::SeqCst), 1);
    let epub = open_epub(&dir.join("fallback.epub"));
    let text = resource_text(&epub);
    assert!(text.contains("FALLBACK_PAGE_MARKER"), "{text}");
    assert_eq!(
        epub.metadata().title().unwrap().value(),
        "Fallback Headline"
    );
    assert_eq!(
        epub.metadata().identifier().unwrap().value(),
        format!("{}/fallback-canonical", server.base)
    );

    let empty_url = format!("{}/reader-empty", server.base);
    let empty_reader = format!("{}/empty-reader", server.base);
    let empty = folio(&dir, &["-o", "empty-fallback.epub", &empty_url]);
    assert_eq!(empty.status, 0, "stderr={}", empty.stderr);
    assert!(
        empty.stderr.contains(&format!(
            "warning: reader view failed ({empty_reader}, no article text); used the article page"
        )),
        "{}",
        empty.stderr
    );
    assert!(!empty.stderr.contains("error:"), "{}", empty.stderr);
    let epub = open_epub(&dir.join("empty-fallback.epub"));
    let text = resource_text(&epub);
    assert!(text.contains("EMPTY_FALLBACK_MARKER"), "{text}");
    assert!(
        !text.contains("SECRET") && !text.contains("hidden"),
        "{text}"
    );
}

#[test]
fn slug_from_title_is_the_default_path() {
    let server = Server::start();
    let dir = workdir();
    let url = format!("{}/no-date?title=Don%27t%20Stop", server.base);
    let out = folio(&dir, &[&url]);
    assert_eq!(out.status, 0, "stderr={}", out.stderr);
    assert_eq!(out.stdout, "./dont-stop.epub\n");
    assert!(dir.join("dont-stop.epub").is_file());
}
