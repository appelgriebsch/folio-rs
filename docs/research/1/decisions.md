# Decisions — folio-rs

Date: 2026-09-27

Settled in grilling on 2026-09-27. Facts they rely on are in `rust-page-export-stack.md`. The implementation plan is in `implementation-plan.md`.

## Tool and repository

**Chosen:** Crate `folio-rs` at `/Volumes/Data/Projects/rust/folio-rs`, binary `folio`, public MIT repository `github.com/appelgriebsch/folio-rs`.

**Why:** The home directory is not a git repo. Other Rust crates in that tree use MIT. The operator asked for a small Rust CLI and accepted a new public repo.

**Rejected:** Putting the crate in the home directory. A private repository. Naming the binary `folio-rs`.

## Acquisition

**Chosen:** One HTTP GET of the typed URL, redirects followed. No login, no cookies jar for a session, no headless browser. If the response HTML has no article text (a script shell), the run fails with a clear error. `robots.txt` is not read. The operator is responsible for whether they may copy the page.

**Why:** The operator asked for a small CLI and a single keyed URL, including its direct next pages. A browser binary contradicts that size. RFC 9309 is a crawler hint, not a fetch the operator already named.

**Rejected:** Headless Chrome or WebDriver for acquisition. A login or paywall flow. Consulting `robots.txt`.

## Reader view and extract

**Chosen:** When the page offers a publisher reading view, fetch that HTML and build the document from it. Otherwise extract the article from the fetched page. Browser reader mode is not a URL.

**Why:** The operator prefers a publisher reading view and, failing that, the article rather than the chrome.

**Rejected:** Always printing the full page. Treating Firefox or Safari reader mode as something the HTML file contains.

**Chosen detection:** There is no `rel=reader`. A publisher reading view is exactly one same-origin `http(s)` link that is not fragment-only. `rel` matches a whole token (`reader`, `readmode`, `reading-view`). The URL path matches a whole segment equal to one of those. The accessible name (`aria-label` if non-empty, otherwise the link text) is at most about 40 characters and contains one of those tokens as a word, or is the whole name `Leseansicht` or `Lesemodus`. Reject names that contain “screen reader” or “e-reader”. Fetch that URL once. If the fetch fails or the HTML has no article, use the typed URL. Do not follow a second hop. `rel=amphtml` is not a reading view. Zero or several matches means stay on the typed page and do not warn.

**Rejected:** Treating AMP as the reading view. Treating a print stylesheet as the reading view. Following a reader link when several match.

## Continuation pages

**Chosen:** Same host as the final URL after redirects (scheme may differ). Follow `rel=next` on `a` and `link` first. If more than one `rel=next` survives, stop. Do not fall through to text when `rel=next` is ambiguous. If `rel=next` is absent, follow a continuation only when exactly one link’s entire accessible name, case-folded, is one of: `next`, `next page`, `continue`, `weiter`, `weiterlesen`, `nächste`, `nächste seite`, `›`, `»`, `>`. Reject fragment-only links and links inside `nav`, `footer`, `aside`, or `role=navigation`. Stop at 15 pages including the first (`--max-pages`, default 15, allowed 1–15), on a repeated URL, or when zero or several candidates match. Several matches or the cap still writes the pages already kept and warns. Zero next links is a normal end. The EPUB identifier and the publish date stay those of the first page.

**Why:** `rel=next` is the specified sequence link. Anchor text is a heuristic. A single-candidate rule avoids crawling the rest of the site.

**Rejected:** Unbounded crawl. Following every “next” on the page. Treating schema.org `pagination` as a next URL (it is a page-range string).

## Output file

**Chosen:** One run writes one file. `--format pdf|epub`, default `epub`. `-o` sets the path. The default path is a slug of the title plus the extension, in the current directory. If that path exists, the run fails unless `--force`.

**Why:** The operator wants either format, with EPUB as the default archive, and no silent overwrite.

**Rejected:** Writing both formats in one run. Overwriting by default. Headless Chrome or wkhtmltopdf as the PDF renderer.

## PDF layout

**Chosen:** The PDF is built in-process from the extracted article (headings, paragraphs, embedded images). Title, author, and dates go in the PDF info dictionary. The PDF is not a picture of the website and does not execute CSS.

**Print check, PDF only:** When exactly one same-origin `http(s)` link matches, the PDF is built from that HTML after the same extract and sanitize. This does not change the EPUB source. Match the accessible name, as a whole string, against: `print`, `print article`, `print this article`, `print version`, `printable version`, `printer-friendly`, `printer friendly`, `print view`, `Druckansicht`, `Druckversion`. A path segment may equal `print`, `printable`, or `printer-friendly`. Do not search the path for the substring `print` (`/imprint` is not a print view). A `media="print"` stylesheet or `@media print` block is not applied and does not pull in a browser. If both a reader link and a print link exist, the PDF uses the print link and the EPUB uses the reader link. Discover `rel=next` on the variant that was actually extracted.

**Why:** The operator accepted an in-process PDF and then asked to look at a print layout for PDF export. Print CSS needs a browser. A print-view HTML link is the check that stays inside the static fetcher.

**Rejected:** Chrome print-to-PDF so that print CSS would apply. Using the print link as the EPUB source.

## Images

**Chosen:** Download images that belong to the kept article and embed the bytes. Resolve relative URLs against the document base URL (`<base href>` if present, otherwise the document URL). If `src` is a real URL, do not replace it with `data-src`. Drop trackers and images smaller than about 2 KB or 1×1. A failed image is omitted. The article still succeeds.

**Why:** The operator wants images in the file and a document that still builds when one image fails.

**Rejected:** Hotlinking remote image URLs inside the EPUB or PDF. Failing the whole run on one image error.

Working rule for “real `src`”, from that decision plus the HTML spec (not a separate product choice): `src` is real when it is a non-empty `http`, `https`, or `data` URL and not a 1×1 placeholder. Otherwise a `data-src` URL may be used. When `srcset` is present, pick the candidate with the largest width descriptor. If candidates use only density descriptors, pick the highest density. A CLI has no viewport, and the spec does not name a non-browser default.

## Metadata

**Chosen:** Title is required; a missing title fails the run. Author, description, language, canonical URL, and publish date are written when the page exposes them. A missing publish date does not fail the run. The document identifier is the canonical URL, otherwise the typed URL. Language is `und` when the page does not say.

**Why:** EPUB 3 requires identifier, title, and language. The operator wants article metadata and the publish date when it exists, and a successful file when the date does not.

**Rejected:** Inventing a publish date. Using `dateModified` or a bare `time[datetime]` as the publish date. Failing closed when optional fields are absent.

**Publish date order:** the first non-empty value among JSON-LD `datePublished`, Open Graph `article:published_time`, `citation_publication_date`, and Dublin Core `issued`. Specs name these fields and do not rank them. This order prefers the article vocabulary, then the citation date, then issuance. `dateModified`, `article:modified_time`, and a bare `time[datetime]` are not the publish date. Read these four fields in folio. Do not copy `dom_smoothie`’s published-time order.

## Libraries

**Chosen:**

- HTTP: `reqwest` 0.13 with the `blocking` feature. The operator wants the same client a later web UI can use in async form. v1 does not start a Tokio runtime in application code. Set a timeout and a byte cap. `reqwest::blocking` may start a runtime inside the library.
- Decode: `encoding_rs`. Charset from `Content-Type`, else an HTML meta prescan, else UTF-8. Do not use `Response::text` as the decoder.
- Extract: `dom_smoothie`. Link scan uses its `dom_query` document. Publish-date order stays in folio.
- Sanitize: `ammonia`, configured (not only the defaults). Flatten image URLs before `clean`.
- EPUB: `rbook`.
- PDF: `printpdf` with the `html` feature and `PdfDocumentInfo`.
- CLI: `clap` 4 derive. Errors: `thiserror` 2. JSON-LD: `serde_json`. URLs: `url`. Image bytes: `image`.

**Rejected:** `ureq` for v1. `readabilityrs`. `epub-builder`. Headless Chrome and wkhtmltopdf. A Python or Swift implementation.

## Command behavior

**Chosen:** Stdout is one line, the path of the finished file. Stderr holds `error:` and `warning:` lines. Exit 0 when the file is complete, including when warnings were printed. Exit 2 for usage, before a fetch. Exit 1 when arguments were accepted and no new file was left. Exit 130 on interrupt, with no partial file. Write via a temp file in the same directory and rename. `--force` only replaces an existing output file. `-o` / `--output`. If `--format` is omitted and `-o` ends in `.pdf` or `.epub`, use that extension. If `--format` and that extension disagree, exit 2. `-v` / `--verbose` is the decision trace. Version is `-V`. When stderr is a terminal, print one `fetch <url>` line per page. `-o -` is exit 2. Do not create missing parent directories.

**Why:** The operator can script on the path. Warnings must not look like failure. A PDF named file must not contain an EPUB.

**Rejected:** Prompts. Writing the document bytes to stdout. A `--strict` mode in v1. A progress bar.
