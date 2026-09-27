# folio-rs implementation plan

Date: 2026-09-27

Crate `folio-rs`, binary `folio`, MIT, path `/Volumes/Data/Projects/rust/folio-rs`, intended repository `github.com/appelgriebsch/folio-rs`.

Decisions: `decisions.md`. Library and spec notes: `rust-page-export-stack.md`.

## Summary

`folio` takes one `http` or `https` URL and writes one file. The default file is an EPUB. `--format pdf` writes a PDF. The tool fetches the HTML with `reqwest` blocking. It does not run JavaScript, log in, or read `robots.txt`. It prefers one publisher reader link. For a PDF only, it can prefer one print-view link. It extracts the article with `dom_smoothie`, drops `script`, `iframe`, `audio`, and `video`, embeds images, and follows direct next pages up to 15. The file metadata carries the article title and, when the page has them, author, description, language, canonical URL, and publish date.

## Experts consulted

- **Rust** — `Cargo.toml` trees under `/Volumes/Data/Projects/rust`, and this plan. Static pipeline accepted. `printpdf` HTML must be well-formed XML.
- **Web frontend** — article HTML and print CSS. Match reader and print links by tokens, not substrings. Flatten images before sanitize. Do not execute `@media print`.
- **UI/UX** — the `folio` command. Stdout is only the written path. Warnings stay on stderr and still exit 0.
- **CI** — GitHub Actions and `just` files in the workspace. One `cargo test --locked` job. No Docker and no release workflow in v1.
- **Python** and **Swift** — present elsewhere in the workspace. Neither belongs in this tool.

No Bun server, GIS, Postgres, Spring, or observability stack applies.

## Pipeline

On each page:

1. GET with `reqwest::blocking`. Follow redirects. Timeout and a byte cap are explicit. Reject any scheme other than `http` or `https`.
2. Decode bytes with `encoding_rs`. Use the `Content-Type` charset when it is present. Otherwise prescan for `<meta charset>`. Otherwise use UTF-8. Do not use `Response::text` as the decoder.
3. On the raw document, read metadata and links before extraction. The document base is the response URL after redirects, overridden by the first valid `<base href>`. Do not resolve URLs against `rel=canonical`.
4. Choose the HTML source. Ticket 1 always uses the fetched page. Later tickets add the reader link and the print link.
5. Extract with `dom_smoothie`. No article text is a failed run. A script shell uses that same error.
6. Flatten kept images to one absolute `src`, then sanitize with a configured `ammonia`, then download.
7. Follow continuation from the document that was extracted. The cap includes the first page.
8. Write EPUB 3 with `rbook`, or a PDF with `printpdf`. `dcterms:modified` is the file time. The article publish date is `dc:date` when present.

Publish date is the first non-empty value among JSON-LD `datePublished` (prefer an `Article` type or subtype), Open Graph `article:published_time`, `citation_publication_date`, and Dublin Core `issued`. Folio reads these fields itself.

Identifier is an absolute `http(s)` canonical URL, otherwise the typed URL. Language is the page language, otherwise `und`. Title is required.

## Command

```text
folio <url> [--format pdf|epub] [-o|--output path] [-f|--force] [--max-pages N] [-v|--verbose]
```

- Default format is `epub`. `--format` is long-only so `-f` is force.
- If `--format` is omitted and `-o` ends in `.pdf` or `.epub`, use that extension. A disagreement exits 2 and writes nothing.
- Default path is `./<slug>.<ext>` in the current directory. The slug is NFKC, case-folded, apostrophes removed, other non-letter non-number runs become one hyphen, leading dots trimmed, capped at 80 characters and 200 bytes. An empty slug or a Windows device name exits 1 and tells the operator to pass `-o`. Do not use `untitled`.
- An existing output path exits 1 before image downloads. `--force` replaces only after the new bytes are complete, via a temp file in the same directory and rename. `--force` does not skip a missing title or a script shell.
- Stdout on success is the path and a newline. Stderr has `error:` and `warning:` lines. When stderr is a terminal, print one `fetch <url>` line per page. `-v` prints those lines even when stderr is not a terminal, plus why a reader or next link was chosen.
- Exit 0 when the file is complete. Exit 2 for usage, before a fetch. Exit 1 when the run accepted the arguments and left no new file. Exit 130 on interrupt, with no partial file.
- `-o -` exits 2. An existing directory for `-o` exits 1. Do not create missing parent directories.
- `--max-pages` is the total page count, including the first. Default 15. Allowed range 1–15. Out of range exits 2.
- Several next links, or the page cap, still write the pages already kept and warn. A failed reader fetch warns and uses the typed page. Omitted images warn with up to 10 URLs and a remainder count. A missing publish date is silent.

## Crates

Edition 2024. `rust-version` = `1.88` because `printpdf` and `image` 0.25 require it. Commit `Cargo.lock`. No `rust-toolchain.toml`. Package name `folio-rs`, binary name `folio`.

| Job | Crate |
| --- | --- |
| HTTP | `reqwest` 0.13, feature `blocking`, rustls. v1 application code stays synchronous. A later UI can use the async API. |
| Decode | `encoding_rs` |
| Extract and link scan | `dom_smoothie` (`dom_query`). Do not add `scraper` unless `dom_query` cannot see `rel` on `link` and `a`. |
| Sanitize | `ammonia` 4, configured |
| EPUB | `rbook` |
| PDF | `printpdf` 0.12, feature `html`, `PdfDocumentInfo` |
| CLI | `clap` 4 derive |
| Errors | `thiserror` 2 |
| JSON-LD | `serde_json` |
| URLs | `url` |
| Images | `image` 0.25, only the formats that are embedded |

## Sanitizer and images

Flatten images before `ammonia`. The default policy drops `srcset` and `data:` URLs.

- A real `src` is a non-empty `http`, `https`, or `data` URL and is not a placeholder. Placeholders: a 1×1 GIF or PNG, a short `data:` URL (about 133 characters of base64) unless the type is `image/svg+xml`, or `width` and `height` both 1. Do not replace a real `http(s)` `src` with `data-src`.
- `srcset`: if width and density descriptors are mixed, ignore `srcset` and use `src`. Otherwise the largest width, else the highest density. If the body exceeds a few megabytes, try the next-smaller candidate, then omit the image.
- `picture`: use the `img` when it is real. Otherwise one `source` whose type is jpeg, png, or gif. Skip `media` on `source`.
- Embed only `image/jpeg`, `image/png`, `image/gif`, and `image/svg+xml`. Omit SVG from the PDF when the renderer cannot paint it. Keep an SVG in the EPUB only when it has no `script`, no event handlers, and no `foreignObject`.
- Drop a decoded image when either side is 1 pixel. The 2 KB rule applies only when the bytes cannot be decoded. Do not treat a cross-origin host as a tracker.
- A failed image is omitted. The article still succeeds.

Ammonia keeps `figcaption`, tables, and `sup` by default. Also allow `id` on kept tags and generic `dir`. Add `iframe`, `video`, `audio`, `embed`, `object`, `noscript`, `canvas`, `svg`, `math`, and `form` to the clean-content set so their contents do not unwrap. Before that, if a `noscript` parses as one `img` and the previous sibling is a placeholder `img`, keep the noscript image. If a `math` node has a TeX `annotation`, copy that text out, then drop `math`. Leave `class`, `style`, and event handlers blocked.

Serialize XHTML for both writers. Ammonia emits HTML5. An EPUB content document and `printpdf` `from_html` need well-formed XML.

## CI

`.github/workflows/ci.yml`: one job named `test`, `ubuntu-latest`, `actions/checkout@v7`, `dtolnay/rust-toolchain@stable`, `cargo test --locked`. Triggers: push to `main`, and `pull_request`. `permissions: contents: read`. Concurrency cancels superseded runs. No fmt, clippy, cache, MSRV job, Docker, or release workflow in v1.

## Risks

- `printpdf` lays out a limited XHTML subset. Ordinary article HTML fails until the PDF writer emits a small XML subset. The binary is heavy because of that layout engine.
- Reader and print lists miss labels that are not listed. Substring search is rejected because it selects `/imprint` and “screen reader”.
- `rel=next` can mean the next post. The cap and the single-candidate rule limit that.
- A real `http(s)` placeholder in `src`, with the photo only in `data-src`, stays the placeholder.
- Fifteen pages with large images can use a lot of memory. Cap HTML bytes, image bytes, and redirect hops.
- Exit 0 with warnings means a short or image-poor file is still success.
- `reqwest::blocking` starts a Tokio runtime inside the library. Application code does not spawn tasks.

## Tests

Use a local HTTP fixture server. Do not call the public network in tests.

- Script, iframe, audio, and video contents are absent. An image is in the file.
- Missing title exits 1 and writes nothing.
- Existing output without `--force` exits 1 and is unchanged. `--force` replaces it.
- Metadata order and a missing date.
- Reader link, several reader links, and a failed reader fetch.
- Print link changes the PDF source and not the EPUB source.
- `rel=next` chain, ambiguous `rel=next`, text next-link, loop, and the page cap.
- EPUB package has `dc:identifier`, `dc:title`, `dc:language`, and `dc:date` when a date exists. `rbook` opens the file it wrote.
- PDF info dictionary has title and author.

## Tickets

Four tracer bullets. Ticket 1 has no blockers. Tickets 2, 3, and 4 depend only on ticket 1 and can proceed in parallel after it.

### 1 — Single-page EPUB

**Goal:** `folio <url>` writes one EPUB of the article on that page.

**Depends on:** none.

**Excludes:** reader links, print links, next pages, and PDF. `--format pdf` is not in this ticket.

**Done when:** `cargo test` passes. A fixture server produces an EPUB that `rbook` can open, with the title, language, identifier, article text, and embedded image, and without script, iframe, audio, or video payload. Missing title and an existing output path fail as specified. CI runs `cargo test --locked`.

**Steps:**

1. Create the crate at `/Volumes/Data/Projects/rust/folio-rs`. Edition 2024, `rust-version` 1.88, license MIT, binary name `folio`. Commit `Cargo.lock`.
2. Add the crates in the table except `printpdf`. Enable `reqwest` feature `blocking`.
3. CLI as in Command, without the `pdf` format value.
4. Fetch, decode, base URL, metadata, `dom_smoothie` extract, image flatten, ammonia, image download, `rbook` write, atomic rename.
5. Map errors with `thiserror`. No `unsafe`. No `unwrap` on library errors that a bad page can cause.
6. Fixtures and the tests this ticket owns.
7. Add `.github/workflows/ci.yml` as specified under CI.

### 2 — Reader view

**Goal:** When exactly one reader link matches, the EPUB is built from that document.

**Depends on:** ticket 1.

**Excludes:** print links, pagination, and PDF.

**Done when:** A fixture with one reader link produces an EPUB whose text comes from the reader HTML. Several matches stay on the typed page and do not warn. A failed reader fetch warns on stderr and still writes the EPUB from the typed page.

**Steps:**

1. Scan the raw `dom_query` document before extract. Rules are in `decisions.md` (whole token, path segment, short accessible name, `Leseansicht`, `Lesemodus`, reject “screen reader” and “e-reader”).
2. Fetch that URL once. On failure or empty article, use the typed page and warn with the reader URL and status.
3. Do not follow a second hop. Extract the chosen document.
4. `-v` prints why the reader link was chosen.
5. Add the three fixture tests.

### 3 — Continuation pages

**Goal:** Follow direct next pages and append them to the same EPUB.

**Depends on:** ticket 1.

**Excludes:** reader links and PDF. Do not change first-page metadata.

**Done when:** A three-page fixture becomes one EPUB with three sections. Two `rel=next` links stop after the current page, warn, and still write the file. Page 16 is not fetched when the cap is 15. A repeated URL stops with no extra warning.

**Steps:**

1. After extract, find continuation on that document. `rel=next` on `a` and `link` first. More than one match stops the chain. Otherwise the accessible-name list in `decisions.md`. Reject fragment-only links and links in `nav`, `footer`, `aside`, or `role=navigation`.
2. Same host as the final response URL. Scheme may differ. Cap is `--max-pages` (default 15), counting the first page.
3. Concatenate extracted bodies in order. Identifier, title, and publish date stay those of the first page.
4. Warn once for several candidates. Warn once when the cap stops the walk and a further next link was visible.
5. Fixture tests for the chain, the ambiguous `rel=next`, the text label, the loop, and the cap.

### 4 — PDF and print view

**Goal:** `--format pdf` writes one in-process PDF. A single print-view link supplies the PDF HTML and not the EPUB HTML.

**Depends on:** ticket 1. It does not wait for tickets 2 or 3. If ticket 3 is already merged, the PDF uses the same continuation walker on the PDF source document.

**Excludes:** Chrome. Executing print CSS. Using the print link as the EPUB source.

**Done when:** A fixture PDF has the title and author in the info dictionary and contains the image. Video text is absent. A print-link fixture: PDF text comes from the print HTML, EPUB text comes from the article HTML. `--format pdf` with `-o story.epub` exits 2.

**Steps:**

1. Add `printpdf` with `html`. Add `pdf` to `--format`. Apply the `-o` extension rule.
2. Serialize a small XHTML subset from the sanitized article (`p`, headings, `img`, lists, tables, `blockquote`, `br`). Escape text. Point `img src` at names in the byte map `printpdf` expects.
3. Set `PdfDocumentInfo` title, author, and subject from the description. Use the article publish date as the creation date when the page has one. Otherwise use the file time. `dcterms:modified` on the EPUB stays the file time. The info dictionary has no separate publish-date key.
4. Print-link matcher from `decisions.md`, including `Druckansicht` and `Druckversion`. Fetch once. Failure falls back to the reader-or-typed source and warns.
5. Discover `rel=next` on the HTML that was extracted for that format.
6. Parse the PDF back in tests and read the info dictionary.
7. Omit SVG bytes the renderer cannot paint. The run still succeeds.

## Filing

Epic: https://github.com/appelgriebsch/folio-rs/issues/1

Children:

- https://github.com/appelgriebsch/folio-rs/issues/2 — single-page EPUB. No blockers.
- https://github.com/appelgriebsch/folio-rs/issues/5 — reader view. Blocked by issue 2.
- https://github.com/appelgriebsch/folio-rs/issues/3 — continuation pages. Blocked by issue 2.
- https://github.com/appelgriebsch/folio-rs/issues/4 — PDF and print view. Blocked by issue 2.

Labels: issue 1 has `epic` and `has-plan`. Issues 2–5 have `has-plan`.
