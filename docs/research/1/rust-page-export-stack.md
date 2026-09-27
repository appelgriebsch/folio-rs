# Rust page-export stack: HTML fetch, article extract, EPUB, PDF

Date: 2026-09-27

Versions below are the newest non-yanked release reported by crates.io or the matching docs.rs crate page on this date. Where a behavior is not stated in the crate README, docs.rs, or upstream source that was read, it is marked unverified.

## Question

For a small Rust CLI that fetches a web article URL and writes PDF and/or EPUB: which HTTP client to use, what a static fetch cannot see, which crates extract article HTML, which page-level metadata signals are actually specified, how multi-page articles are indicated, how to drop script/iframe/audio/video while keeping images, and which EPUB and PDF crates can carry title, author, date, language, identifier, description, and embedded images.

## Findings

### 1. Fetching HTML

[reqwest 0.13.5](https://crates.io/crates/reqwest) (published 2026-09-08, MIT OR Apache-2.0, repo [seanmonstar/reqwest](https://github.com/seanmonstar/reqwest)) is the higher-level HTTP client documented at [docs.rs/reqwest](https://docs.rs/reqwest). The crate docs describe async and blocking clients, redirects, proxies, cookies, and TLS. Default features on 0.13.5 include `default-tls` (rustls), `charset`, `http2`, and `system-proxy`. `blocking` is not a default feature. [`Response::text`](https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html#method.text) decodes using the `charset` parameter of `Content-Type`, and defaults to UTF-8 if that parameter is absent. With the `charset` feature disabled, it only attempts UTF-8.

That is not the same as HTML’s encoding algorithm. The [encoding sniffing algorithm](https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding) uses out-of-band metadata such as the transport `Content-Type`, and otherwise may [prescan](https://html.spec.whatwg.org/multipage/urls-and-fetching.html#extracting-character-encodings-from-meta-elements) the byte stream for a `meta` charset. A client that stops at `Response::text` will not apply `<meta charset>` when the HTTP header has no charset parameter.

Maintained alternatives, not drop-in browsers:

- [ureq 3.4.2](https://crates.io/crates/ureq) (2026-09-13, MIT OR Apache-2.0): crates.io description, “Simple, safe HTTP client.” Blocking, no JS.
- [hyper 1.11.1](https://crates.io/crates/hyper) (2026-08-28, MIT): lower-level HTTP library. reqwest is the usual wrapper when the CLI only needs “GET this URL and give me bytes.”
- [wreq 0.16.1](https://crates.io/crates/wreq) (2026-08-27, Apache-2.0): another HTTP client (“privacy-aware”). Not required if reqwest’s feature set is enough.

A static client returns the HTTP response body. It does not build a DOM and does not run scripts. Anything that exists only after JavaScript (client-rendered article text, `src` written by a lazy-load script, cookie-consent gates, infinite scroll) is absent. The bytes can still contain server-rendered HTML, `<noscript>` content, and metadata in `<head>`.

Headless browser crates execute the page, so they can return the post-script DOM and print it:

- [chromiumoxide 0.9.1](https://docs.rs/crate/chromiumoxide/0.9.1) (2026-02-25, MIT OR Apache-2.0). README: controls Chrome/Chromium over the DevTools Protocol, launches headless by default, and can download a browser with the `fetcher` feature. `Page::pdf` calls Chrome print-to-PDF. Tokio only. System dependency: a Chrome/Chromium binary, unless fetched.
- [headless_chrome 1.0.22](https://docs.rs/crate/headless_chrome/1.0.22) (2026-06-11, MIT). README: DevTools Protocol, synchronous API, `Tab::print_to_pdf`, optional fetch of a known-good Chromium. Same system dependency.
- [fantoccini 0.22.1](https://crates.io/crates/fantoccini) (2026-02-28, MIT OR Apache-2.0) and [thirtyfour 0.37.5](https://crates.io/crates/thirtyfour) (2026-08-12, MIT OR Apache-2.0) speak WebDriver. They need a running WebDriver plus a browser; they are not HTTP clients.

### 2. Article / reader extraction

These crates do not all do the same job. “Extract the article” is separate from “parse HTML,” “sanitize HTML,” and “turn HTML into Markdown.”

| Crate | Latest confirmed | License | Last publish | What it does | Images |
| --- | --- | --- | --- | --- | --- |
| [dom_smoothie 0.18.2](https://crates.io/crates/dom_smoothie) | 0.18.2 | MIT | 2026-09-21 | README: follows [mozilla/readability](https://github.com/mozilla/readability). `Readability::parse` returns an `Article` with title, byline, excerpt, site name, published time, modified time, `image`, `content` (HTML), and text. Optional `document_url` rewrites relative URLs to absolute. | `article.image` is a metadata field. `content` is HTML of the extracted node. README example uses a `lazy-image-3` fixture. Exact tag policy is “follows readability.js,” not a separate documented allow-list. |
| [readabilityrs 0.1.4](https://crates.io/crates/readabilityrs) | 0.1.4 | Apache-2.0 | 2026-08-10 | README: Rust port of Mozilla Readability.js. Returns title, byline, body HTML, excerpt, site name, language, publication time. Metadata order stated by the README: JSON-LD, then Open Graph, Twitter, Dublin Core, then plain meta. | README says a Markdown standardization pass rewrites lazy-loaded images. It also says relative links inside extracted HTML are **not** rewritten against the base URL. |
| [readability-js 0.1.6](https://crates.io/crates/readability-js) | 0.1.6 | UPL-1.0 | 2026-09-17 | crates.io: “A Rust wrapper for Mozilla's Readability.js.” | Image behavior and which JS engine it embeds: **unverified** beyond that description. |
| [libreadability 0.2.0](https://crates.io/crates/libreadability) | 0.2.0 | MIT | 2026-02-28 | crates.io: “Rust port of go-readability — extract readable content from HTML.” Repo [nchapman/readability-rs](https://github.com/nchapman/readability-rs). | Image preservation: **unverified** (README not read for this note). |
| [readability-rust 0.1.0](https://crates.io/crates/readability-rust) | 0.1.0 | Apache-2.0 | 2025-07-22 | crates.io: “A Rust port of Mozilla's Readability library.” One release. | **Unverified.** |
| [readability 0.3.0](https://crates.io/crates/readability) | 0.3.0 | MIT | 2023-12-20 | [README](https://github.com/kumabook/readability): port of **Arc90** readability, not Mozilla’s. `extractor::scrape(url)` returns `content` (HTML) and `text`. It performs its own fetch. | Image policy: **not documented** in that README. |
| [readable-readability 0.4.0](https://crates.io/crates/readable-readability) | 0.4.0 | MIT | 2022-12-17 | crates.io: “Really fast readability.” No release since 2022. | **Unverified.** |
| [article_scraper 2.3.1](https://crates.io/crates/article_scraper) | 2.3.1 | GPL-3.0-or-later | 2026-03-01 | crates.io: fivefilters full-text configs plus Mozilla readability. | **Unverified.** GPL is a distribution constraint. |
| [llm_readability 0.0.17](https://crates.io/crates/llm_readability) | 0.0.17 | MIT | 2026-04-30 | crates.io: “Readability library for LLM's.” Not described as a Mozilla port. | **Unverified.** |
| [ammonia 4.2.0](https://crates.io/crates/ammonia) | 4.2.0 | MIT OR Apache-2.0 | 2026-09-17 | HTML sanitizer, not an article extractor. See stripping below. | Keeps `img` under the default policy; does not choose the article node. |
| [scraper 0.27.0](https://crates.io/crates/scraper) | 0.27.0 | ISC | 2026-05-11 | crates.io: “HTML parsing and querying with CSS selectors.” You supply the selectors. | Keeps whatever your selectors keep. |
| [lol_html 3.0.1](https://crates.io/crates/lol_html) | 3.0.1 | BSD-3-Clause | 2026-07-29 | crates.io: streaming HTML rewriter with CSS selectors. Not an article extractor. | Rewrites only the elements you select. |
| [htmd 0.5.5](https://crates.io/crates/htmd) | 0.5.5 | Apache-2.0 | 2026-07-27 | crates.io: HTML-to-Markdown (turndown.js style). Not an article extractor. | Conversion of `img` to Markdown images: **unverified** here. |
| [html2text 0.17.1](https://crates.io/crates/html2text) | 0.17.1 | MIT | 2026-04-19 | Renders HTML as plain text. Drops structure you would want in EPUB/PDF. | Not an image pipeline. |
| [kuchiki 0.8.1](https://crates.io/crates/kuchiki) | 0.8.1 | MIT | 2020-08-05 | HTML tree library. Last publish 2020. Superseded in practice by `scraper` for new code. | n/a |
| [webpage 2.0.1](https://crates.io/crates/webpage) | 2.0.1 | MIT | 2024-05-03 | crates.io: fetches title, description, language, links, RSS, Open Graph, Schema.org. Metadata helper, not a full article body extractor. | Open Graph image only, per the crate description. |

There is no crate named `readability-rs` on crates.io. The name is used by the [libreadability](https://github.com/nchapman/readability-rs) repository. [readabilityrs](https://crates.io/crates/readabilityrs) is a different crate.

#### How Mozilla Readability picks the article node

[Readability.js](https://github.com/mozilla/readability/blob/main/Readability.js) (file header: based on Arc90 readability 1.7.1) documents the pass in `parse()` and `_grabArticle`:

1. Read JSON-LD before scripts are removed (`_getJSONLD`), then `_removeScripts` (drops `script` and `noscript`).
2. `_prepDocument` strips `style` and repairs some markup.
3. `_grabArticle` scores elements. The comment on that function: “Using a variety of metrics (content score, classname, element types), find the content that is most likely to be the stuff a user wants to read.”
4. Default scored tags (`DEFAULT_TAGS_TO_SCORE`): `section, h2, h3, h4, h5, h6, p, td, pre`.
5. `_initializeNode` adds a tag prior: `DIV` +5; `PRE` / `TD` / `BLOCKQUOTE` +3; `ADDRESS`, lists, `FORM` −3; `H1`–`H6` and `TH` −5; then a class/id weight (`_getClassWeight`). Class/id regexes treat tokens such as `article`, `content`, `main` as positive and `sidebar`, `comment`, `footer`, `header` as negative. `unlikelyCandidates` removes nodes whose class/id matches ad, banner, comment, footer, sidebar, related, and similar tokens, unless `okMaybeItsACandidate` also matches.
6. Text length, commas, and link density adjust scores. The highest-scoring candidate is taken, then siblings above a score threshold are pulled in. The result is wrapped in a `div`.
7. `_prepArticle` cleans that node (see stripping). `parse()` returns `title`, `byline`, `dir`, `lang`, `content` (serialized HTML), `textContent`, `excerpt`, `siteName`, `publishedTime`.

Its own metadata fallback for `publishedTime`, in `_getArticleMetadata`, is JSON-LD `datePublished`, then `article:published_time`, then `parsely-pub-date`. That order is this program’s, not a rule in the HTML or schema.org specs.

#### Signals that are in the page

**HTML link relations** are defined in the [WHATWG HTML living standard, link types](https://html.spec.whatwg.org/multipage/links.html#linkTypes) (snapshot read 2026-09-25):

- [`rel=alternate`](https://html.spec.whatwg.org/multipage/links.html#rel-alternate): alternate representations of the current document (syndication feeds, translations, and so on, depending on `type` / `hreflang`).
- [`rel=canonical`](https://html.spec.whatwg.org/multipage/links.html#link-type-canonical): the `href` is the preferred URL for this document. The spec points at [RFC 6596](https://www.rfc-editor.org/rfc/rfc6596). Allowed on `link`, not on `a`.
- [`rel=next`](https://html.spec.whatwg.org/multipage/links.html#link-type-next) and [`rel=prev`](https://html.spec.whatwg.org/multipage/links.html#link-type-prev): the document is part of a sequence, and the link is the next or previous logical document. Allowed on `link`, `a`, `area`, and `form`. For historical reasons, `previous` is a synonym of `prev`.

**AMP.** `rel=amphtml` is **not** in the WHATWG link-type table. The [AMP HTML format](https://github.com/ampproject/amphtml/blob/main/docs/spec/amp-html-format.md) says a canonical document should point at its AMP document with `<link rel="amphtml" href="...">`, and the AMP document points back with `rel=canonical`. The same spec says the `amphtml` relation should be readable without executing JavaScript. AMP documents use `amp-img` rather than unconstrained `<img>`. Whether any given consumer still prefers AMP is outside that spec.

**Open Graph** ([ogp.me](https://ogp.me/)) uses RDFa-style `<meta property="..." content="...">`, not the HTML `name` attribute. Required properties: `og:title`, `og:type`, `og:image`, `og:url`. Optional: `og:description`, `og:audio`, and structured `og:image:*` (type, width, height, alt). For `og:type` = `article` (namespace `https://ogp.me/ns/article#`): `article:published_time`, `article:modified_time`, `article:expiration_time` (datetime), `article:author` (profile array), `article:section`, `article:tag`.

**schema.org Article** ([schema.org/Article](https://schema.org/Article)), expressible as JSON-LD (`<script type="application/ld+json">`), microdata, or RDFa:

- `headline`: text, “Headline of the article.”
- `author` / `creator`: Organization or Person.
- `datePublished`: Date or DateTime, “Date of first publication or broadcast.”
- `dateModified`: most recent modification. Distinct from `datePublished`.
- `dateCreated`: when the work was created. Also distinct.
- `image`: ImageObject or URL.
- `description`, `inLanguage`, `identifier` are inherited from CreativeWork / Thing.

**Google Scholar / Highwire citation meta** is not a WHATWG vocabulary. [Google Scholar inclusion](https://scholar.google.com/intl/en/scholar/inclusion.html) documents `citation_title`, `citation_publication_date` (and `DC.title` / `DC.issued` as a worse fallback), plus journal/volume/issue/page tags. The publication-date tag is required for Scholar inclusion and should be the date you would cite, in `YYYY/M/D` when a full date exists. WHATWG [standard metadata names](https://html.spec.whatwg.org/multipage/semantics.html#standard-metadata-names) are a short list (`application-name`, `author`, `description`, `generator`, `keywords`, `referrer`, `theme-color`, `color-scheme`). `citation_*` and `DC.*` are not among them. Other `meta name` values are still legal HTML; they just have no processing model in the HTML spec.

**Dublin Core.** Terms are specified by DCMI in [DCMI Metadata Terms](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/) (issued 2020-01-20; the page also notes ISO 15836-1 / 15836-2 for the element set and selected terms). Relevant properties, in the `/terms/` namespace unless noted:

- `title` / `http://purl.org/dc/terms/title` (and the older `dc:title` in `/elements/1.1/`).
- `creator`: “An entity responsible for making the resource.”
- `date`: a general date. More specific subproperties include `issued` (“Date of formal issuance”) and `created`.
- `description`, `identifier`, `language`.

DCMI also publishes a convention for putting those terms in HTML `meta` and `link` elements ([dc-html](https://www.dublincore.org/specifications/dublin-core/dc-html/)). That convention is not part of the WHATWG HTML standard. Pages in the wild use both `name="DC.date"` and `name="dc.date"`; the HTML spec does not define either spelling.

#### What reader mode is not

Firefox Reader View is a browser UI. The [Readability.js README](https://github.com/mozilla/readability) says the library is the one used for that view: you pass a DOM document, it returns an article object. Mozilla’s support page describes the feature as a clutter-free view ([Firefox Reader View](https://support.mozilla.org/en-US/kb/firefox-reader-view-clutter-free-web-pages)). There is no `rel=reader`, no `<reader>` element, and no reader-mode entry in the WHATWG link-type table. A static HTML file does not “contain reader mode.” Safari Reader and Chrome reader-style views are the same kind of thing: client features that run an extractor on a document the browser already has.

### 3. Multi-page articles

Per the HTML spec, `rel=next` means “next logical document in the sequence,” and `rel=prev` means the previous one. A document may belong to more than one sequence. On a `link` element, user agents should also treat `next` like a `dns-prefetch`, `preconnect`, or `prefetch` hint. The spec does not say the link is “page 2 of this article,” does not require the words “Next page,” and does not define a page counter.

[schema.org/pagination](https://schema.org/pagination) is a text description of page ranges (“1-6, 9, 55”), equivalent to `bibo:pages`. It is not a URL of the next HTML page. `pageStart` / `pageEnd` are the split-out form of that range. `isPartOf` / `hasPart` can relate works, but they do not define a next-page link.

Anchor text such as “Next”, “Next page”, or “»” is a heuristic. Readability.js even has `REGEXPS.nextLink` for that kind of guess (`next|weiter|continue|»`). That regex is not a specification, and this note did not find `rel=next` consulted by the current `Readability.js` grab path.

### 4. Stripping script, iframe, audio, video; keeping img

**In the page, before extraction.** A static rewriter can drop elements by tag. `scraper` or `lol_html` will do that if you write the selectors. Nothing in the HTML spec removes those elements for you.

**Mozilla Readability output** ([source](https://github.com/mozilla/readability/blob/main/Readability.js)):

- `_removeScripts` removes every `script` and `noscript` in the document before the article is chosen. JSON-LD is read first.
- `_prepArticle` unconditionally `_clean`s `object`, `embed`, `footer`, `link`, `aside`, `iframe`, `input`, `textarea`, `select`, `button`, except when an `object` / `embed` / `iframe` matches `allowedVideoRegex` (YouTube, Vimeo, Dailymotion, and a few other hosts). The comment on `_clean`: “Unless it's a youtube/vimeo video.”
- It does **not** have a blanket `_clean(articleContent, "video")` or `"audio"`. Those elements can remain. `_fixLazyImages` runs on `img`, `picture`, and `figure`, and may copy a lazy URL from another attribute onto `src` or `srcset`.
- `img` is kept and passed through `_fixRelativeUris`, which absolutizes `src`, `poster`, and `srcset`.

So Readability output is not “no video.” It drops generic iframes and most plugin embeds, keeps images, and keeps some video embeds and `<video>`/`<audio>` unless a later sanitizer removes them.

**ammonia 4.2.0** default policy, from [`Builder::default`](https://docs.rs/ammonia/4.2.0/src/ammonia/lib.rs.html) in the published source:

- `clean_content_tags` is `script` and `style`: those elements and their contents are removed.
- `img` is an allowed tag. Default attributes on `img` are `align`, `alt`, `height`, `src`, `width`. `srcset` is not in that set, so a default clean strips `srcset`.
- The default tag set in that function does not include `iframe`, `video`, `audio`, `embed`, or `object`. Those elements are dropped (their text children may survive unless you also list them as clean-content tags). The docs.rs HTML view of the source drops a few tokens that look like HTML (`cite` is the obvious one); confirm the full set in the raw `.rs` if you need every tag. `ammonia::clean` is this default.

To omit `video` and `audio` after Readability, run a sanitizer. ammonia’s default already drops them and keeps `img`, but it will also drop `srcset` unless you add that attribute.

### 5. EPUB

EPUB 3.3 package metadata is specified in [EPUB 3.3 § 5.5](https://www.w3.org/TR/epub-33/#sec-pkg-metadata). The `metadata` element must contain:

- `dc:identifier` (one or more; one is the unique identifier),
- `dc:title` (one or more),
- `dc:language` (one or more; BCP 47; the first in document order is the primary language),
- exactly one `dcterms:modified`, UTC, `YYYY-MM-DDThh:mm:ssZ`.

Those `dc:*` elements use the namespace `http://purl.org/dc/elements/1.1/`. Everything else is optional, including:

- `dc:creator` (display name of a creator; repeat the element for several creators; document order is display priority),
- `dc:date` (publication date of the EPUB; at most one; not the same as `dcterms:modified`; ISO 8601 / W3C date-time recommended),
- `dc:description`.

**epub-builder 0.8.3** ([docs.rs](https://docs.rs/crate/epub-builder/latest), MPL-2.0, published 2026-04-10, [lise-henry/epub-builder](https://github.com/lise-henry/epub-builder/)). `EpubBuilder::metadata` keys documented in the 0.8.3 source: `author`, `title`, `lang`, `direction`, `generator`, `toc_name`, `subject`, `description`, `license`. Separate methods: `set_publication_date`, `set_uuid`, and a setter whose doc comment is the last-modified date (`date_modified` on the metadata struct). Default EPUB version in older docs was V20; 0.8.3’s `EpubVersion` includes `V20`, `V30`, and `V33`. Images are not downloaded. `add_resource(path, reader, mime)` and `add_cover_image` write bytes into the zip under `OEBPS`. The XHTML you supply must point at those paths. The crate does not parse `<img src>` and fetch them.

**rbook 0.7.10** ([docs.rs](https://docs.rs/crate/rbook/latest), Apache-2.0, published 2026-07-01). Reads and writes EPUB 2 and 3. The builder documents `identifier`, `title`, and `language` as required, plus `author` / `creator`, `description`, `publisher`, `publication_date` / `modified_date`, and `cover_image` (path or bytes). Chapters take XHTML bytes. Same model: you embed image bytes; the crate does not fetch the web.

**epub 2.1.5** ([crates.io](https://crates.io/crates/epub), GPL-3.0, 2025-10-29) is a **reader** (“Library to support the reading of epub files”), not a writer.

### 6. PDF

The document information dictionary is the PDF file’s Title / Author / CreationDate bag. Adobe’s [pdfmark reference](https://opensource.adobe.com/dc-acrobat-sdk-docs/library/pdfmark/pdfmark_Basic.html) lists optional Info keys `Title`, `Author`, `Subject`, `Keywords`, `Creator`, `Producer`, `CreationDate`, `ModDate`, all strings. The [Acrobat JavaScript `doc.info`](https://opensource.adobe.com/dc-acrobat-sdk-docs/library/jsapiref/doc.html) object exposes the same names and says they come from that dictionary. The normative text is ISO 32000; the free Adobe PDF 1.7 reference linked from the [printpdf README](https://github.com/fschutt/printpdf) is [pdfreference1.7old.pdf](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf). Dates in modern PDF are a defined string form; this note does not restate the production grammar.

What can actually turn HTML plus images into pages:

| Tool | Confirmed version | Renders HTML? | Images | Info dictionary | System dependency |
| --- | --- | --- | --- | --- | --- |
| [printpdf 0.12.8](https://docs.rs/crate/printpdf/0.12.8) (2026-09-05, MIT) | yes | README: `PdfDocument::from_html` with the `html` feature, “basic layouts,” still evolving. Not a browser. Images are a caller-supplied `BTreeMap` of **named** bytes, not URLs fetched from the network. | `PdfDocumentInfo` has `document_title`, `author`, `creator`, `producer`, `creation_date`, `modification_date`, `subject`, `keywords`. Docs say this stays aligned with the PDF Info dictionary and XMP. | None for the programmatic API. HTML layout is in-process (`azul-layout`), not WebKit/Chromium. |
| [lopdf 0.45.0](https://docs.rs/crate/lopdf/latest) (2026-09-08, MIT) | no | Low-level PDF objects. Optional `embed_image` embeds rasters via the `image` crate. You write content streams yourself. | `change_producer` edits the Info dictionary’s producer. `load_metadata` reads title and page count. Other Info keys are ordinary objects you can add; there is no HTML renderer. | None. |
| [genpdf 0.2.0](https://docs.rs/crate/genpdf/latest) (2021-06-17, Apache-2.0 OR MIT) | no | README: layout of Rust elements (paragraphs, tables) on top of an old printpdf. Optional `images` feature embeds images. | `Document::set_title` only. If unset, the title is empty. No author/date API in the method list reviewed. | None. Last release 2021; still downloaded, but not current. |
| [pdf-writer 0.15.0](https://crates.io/crates/pdf-writer) (2026-05-27, MIT OR Apache-2.0) | no | Step-by-step PDF writer (Typst). Not HTML. | You emit whatever PDF objects you write, including Info, but that is hand-built. | None. |
| [wkhtmltopdf crate 0.4.0](https://crates.io/crates/wkhtmltopdf) (2021-05-04, MIT) | yes, via the external tool | Bindings to wkhtmltopdf (Qt WebKit). The upstream repo [wkhtmltopdf/wkhtmltopdf](https://github.com/wkhtmltopdf/wkhtmltopdf) was **archived on 2023-01-02** and is read-only. README: HTML and images through that engine, headless. | Whether this binding sets Title/Author/CreationDate: **unverified**. | The wkhtmltopdf binary. Unmaintained upstream. |
| [html2pdf 0.9.0](https://crates.io/crates/html2pdf) (2026-08-28, Apache-2.0 OR MIT) | yes, by description | crates.io: “Convert HTML to PDF using a Headless Chrome browser.” Small download count. | Info-dictionary support: **unverified** (README not fully read). | Chrome/Chromium. |
| chromiumoxide / headless_chrome | see §1 | Yes. Navigate, then `Page::pdf` / `print_to_pdf`. The PDF is what Chrome prints, including images the browser fetched. | The crate READMEs show the PDF bytes coming back from Chrome. They do not document writing Info `Title` / `Author`. Treat those fields as unverified unless you set them in a later PDF pass. | Chrome or Chromium on the machine, or the crate’s downloader. |

[pdfium-render 0.9.4](https://crates.io/crates/pdfium-render) wraps Pdfium (Chromium’s PDF library). That is a PDF engine, not an HTML-to-PDF converter. [typst](https://crates.io/crates/typst) typesets its own markup, not article HTML.

### 7. Publish-date fields (no invented ranking)

The specs define different properties. They do not say which one wins when several disagree. A CLI can record all of them. Fields that actually mean “when this was published,” and where that meaning is defined:

| Field | Where it is specified | What the spec says it is |
| --- | --- | --- |
| `datePublished` | [schema.org/Article](https://schema.org/Article) (JSON-LD, microdata, or RDFa) | Date or DateTime. “Date of first publication or broadcast.” |
| `article:published_time` | [Open Graph, article object](https://ogp.me/) | datetime. “When the article was first published.” Applies when `og:type` is `article`. |
| `citation_publication_date` | [Google Scholar inclusion](https://scholar.google.com/intl/en/scholar/inclusion.html) | The date that would be cited. Not a WHATWG or schema.org property. Preferred shape there is `2010/5/12` or a year. |
| `dcterms:issued` / `DC.issued` | [DCMI Metadata Terms](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/#issued); HTML spelling is the [dc-html](https://www.dublincore.org/specifications/dublin-core/dc-html/) convention, and Scholar cites `DC.issued` as a weak fallback | Date of formal issuance of the resource. |
| `dc:date` / `dcterms:date` | same DCMI terms | A date associated with the resource, broader than issuance. |
| `time[datetime]` | [WHATWG `time` element](https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-time-element) | A machine-readable date or time for that element. The HTML spec does not say it is the article’s publication date. It becomes one only if the author marks it that way (for example schema.org microdata `itemprop="datePublished"` on the same element, which schema.org’s own examples do). |

Nearby fields that are **not** the publication date: schema.org `dateModified` and `dateCreated`; Open Graph `article:modified_time`; Scholar’s `citation_online_date` (repository ingest, which Scholar says must not be used as the publication date); EPUB `dcterms:modified` (last change to the EPUB file) versus EPUB `dc:date` (publication date of the EPUB).

Readability.js prefers `datePublished`, then `article:published_time`, then a Parse.ly meta name. That is an implementation order, useful only if you are matching Reader View, not a spec.

### 8. Practical constraints

- **robots.txt and ToS.** [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309) is a file of requests to crawlers. It is not an HTTP authentication scheme, and reqwest will not read it unless you do. A site’s terms of service are a legal document, not a response header the client enforces.
- **Charset.** Honor `Content-Type`’s charset when it is present (reqwest does, if `charset` is on). If it is absent, UTF-8 from `Response::text` can be wrong for pages whose only declaration is `<meta charset>` or a BOM. The HTML sniffing algorithm is the one that covers that case.
- **Relative image URLs.** Resolve against the document base URL: the `<base href>` if present, otherwise the document’s URL ([document base URL](https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url), [`base` element](https://html.spec.whatwg.org/multipage/semantics.html#the-base-element), [URL standard](https://url.spec.whatwg.org/)). `rel=canonical` is the preferred URL for the document, not the base used to resolve relative references.
- **`srcset`.** The [srcset attribute](https://html.spec.whatwg.org/multipage/images.html#srcset-attribute) is a list of candidate URLs with width or density descriptors. A browser picks one using viewport and device pixel ratio. A CLI has no viewport. Taking the first candidate, or the largest width, is a choice you make; the spec does not name a default for non-browsers. ammonia’s default policy drops `srcset` even if you keep `src`.
- **Lazy load.** [`loading=lazy`](https://html.spec.whatwg.org/multipage/urls-and-fetching.html#lazy-loading-attributes) on `img` / `iframe` defers the fetch. The URL is still in `src` or `srcset`. A static client can read it. `data-src` and `data-srcset` are [custom data attributes](https://html.spec.whatwg.org/multipage/dom.html#embedding-custom-non-visible-data-with-the-data-*-attributes). HTML gives them no loading behavior. Pages that put a 1×1 placeholder in `src` and the real URL in `data-src` will export the placeholder unless something like Readability’s `_fixLazyImages` copies the real URL over.
- **Image size.** HTML does not cap image bytes. EPUB and PDF will embed whatever you fetch. `og:image:width` / `og:image:height` and `srcset` width descriptors are hints, not file sizes. The response `Content-Type` and `Content-Length` are the HTTP facts.

## Implications for a CLI

- A static `reqwest` (or `ureq`) GET is enough when the article HTML is in the response. It will not see a JS-rendered DOM. Use chromiumoxide or headless_chrome only when you must, and expect a Chrome/Chromium binary.
- `Response::text` is not an HTML charset decoder. If `Content-Type` has no charset, run the HTML prescan or you will mis-decode some pages.
- For extraction, `dom_smoothie` 0.18.2 and `readabilityrs` 0.1.4 are the Mozilla-style ports with 2026 releases and readable READMEs. `dom_smoothie` can absolutize URLs; `readabilityrs` documents that it currently does not. The crate `readability` 0.3.0 is Arc90’s algorithm and is older. `scraper`, `lol_html`, `htmd`, and `ammonia` do not find the article.
- Readability’s HTML still contains `img`, and can contain `<video>`, `<audio>`, and a few allow-listed video iframes. ammonia’s default removes `script` (and its contents), `style`, `iframe`, `video`, and `audio`, and keeps `img`/`src`, but strips `srcset`.
- Multi-page fetch has one specified hook: `rel=next` / `rel=prev`. Link text is not specified. schema.org `pagination` is a page-range string.
- Publication date has several specified fields and no specified precedence. `time[datetime]` alone is not a publication date.
- EPUB: epub-builder or rbook can set title, author, language, identifier, description, and dates, and can embed image bytes you already have. Neither fetches images. EPUB 3 requires `dc:identifier`, `dc:title`, `dc:language`, and `dcterms:modified`. `dc:creator`, `dc:date`, and `dc:description` are optional. The `epub` crate only reads.
- PDF: nothing in pure Rust reviewed here is a browser. printpdf’s `html` feature lays out a limited HTML subset and takes images by name. genpdf and lopdf do not render HTML. wkhtmltopdf’s upstream project is archived. Headless Chrome can print the real page, images included, but the Rust wrappers do not document setting PDF Info Title/Author; printpdf’s `PdfDocumentInfo` does expose those Info fields if you build or post-process the file yourself.
- robots.txt will not stop the client. Relative URLs, `srcset`, and `data-src` will, if you ignore them.

## Sources

- reqwest crate and `Response::text`: https://crates.io/crates/reqwest , https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html
- ureq, hyper, wreq: https://crates.io/crates/ureq , https://crates.io/crates/hyper , https://crates.io/crates/wreq
- chromiumoxide README and crate page: https://github.com/mattsse/chromiumoxide/blob/main/README.md , https://docs.rs/crate/chromiumoxide/0.9.1
- headless_chrome crate page (README embedded): https://docs.rs/crate/headless_chrome/1.0.22
- HTML encoding sniffing: https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding
- HTML link types (`alternate`, `canonical`, `next`, `prev`): https://html.spec.whatwg.org/multipage/links.html#linkTypes
- RFC 6596 (canonical): https://www.rfc-editor.org/rfc/rfc6596
- AMP HTML document discovery: https://github.com/ampproject/amphtml/blob/main/docs/spec/amp-html-format.md
- Open Graph protocol: https://ogp.me/
- schema.org Article: https://schema.org/Article
- schema.org pagination: https://schema.org/pagination
- DCMI Metadata Terms: https://www.dublincore.org/specifications/dublin-core/dcmi-terms/
- DCMI HTML encoding note: https://www.dublincore.org/specifications/dublin-core/dc-html/
- WHATWG standard metadata names: https://html.spec.whatwg.org/multipage/semantics.html#standard-metadata-names
- WHATWG `time`: https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-time-element
- WHATWG document base URL and `base`: https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url , https://html.spec.whatwg.org/multipage/semantics.html#the-base-element
- WHATWG `srcset`: https://html.spec.whatwg.org/multipage/images.html#srcset-attribute
- WHATWG lazy loading: https://html.spec.whatwg.org/multipage/urls-and-fetching.html#lazy-loading-attributes
- WHATWG `data-*`: https://html.spec.whatwg.org/multipage/dom.html#embedding-custom-non-visible-data-with-the-data-*-attributes
- URL standard: https://url.spec.whatwg.org/
- RFC 9309 (robots.txt): https://www.rfc-editor.org/rfc/rfc9309
- Google Scholar inclusion (citation_* meta): https://scholar.google.com/intl/en/scholar/inclusion.html
- Mozilla Readability.js source and README: https://github.com/mozilla/readability/blob/main/Readability.js , https://github.com/mozilla/readability
- Firefox Reader View (product help): https://support.mozilla.org/en-US/kb/firefox-reader-view-clutter-free-web-pages
- dom_smoothie 0.18.2 README on docs.rs: https://docs.rs/crate/dom_smoothie/0.18.2
- readabilityrs README: https://github.com/theiskaa/readabilityrs/blob/master/README.md
- Arc90-port readability README: https://github.com/kumabook/readability/blob/master/README.md
- Other crates (crates.io records used for version, license, date, description): https://crates.io/crates/readability-js , https://crates.io/crates/libreadability , https://crates.io/crates/readability-rust , https://crates.io/crates/readable-readability , https://crates.io/crates/article_scraper , https://crates.io/crates/llm_readability , https://crates.io/crates/scraper , https://crates.io/crates/lol_html , https://crates.io/crates/htmd , https://crates.io/crates/html2text , https://crates.io/crates/webpage , https://crates.io/crates/kuchiki , https://crates.io/crates/fantoccini , https://crates.io/crates/thirtyfour
- ammonia 4.2.0 default tag policy (published source): https://docs.rs/ammonia/4.2.0/src/ammonia/lib.rs.html
- EPUB 3.3 package metadata: https://www.w3.org/TR/epub-33/#sec-pkg-metadata
- epub-builder 0.8.3 docs and source: https://docs.rs/epub-builder/0.8.3/epub_builder/struct.EpubBuilder.html , https://docs.rs/epub-builder/0.8.3/src/epub_builder/epub.rs.html
- rbook 0.7.10: https://docs.rs/crate/rbook/latest
- epub (reader) crate: https://crates.io/crates/epub
- Adobe pdfmark Info dictionary: https://opensource.adobe.com/dc-acrobat-sdk-docs/library/pdfmark/pdfmark_Basic.html
- Acrobat `doc.info`: https://opensource.adobe.com/dc-acrobat-sdk-docs/library/jsapiref/doc.html
- printpdf README and metadata types: https://github.com/fschutt/printpdf/blob/master/README.md , https://docs.rs/printpdf/0.12.8/printpdf/struct.PdfDocumentInfo.html
- lopdf 0.45.0: https://docs.rs/crate/lopdf/latest , https://docs.rs/lopdf/0.45.0/lopdf/struct.Document.html
- genpdf 0.2.0: https://docs.rs/crate/genpdf/latest , https://docs.rs/genpdf/0.2.0/genpdf/struct.Document.html
- wkhtmltopdf archived repo and bindings crate: https://github.com/wkhtmltopdf/wkhtmltopdf , https://crates.io/crates/wkhtmltopdf
- html2pdf, pdf-writer, pdfium-render: https://crates.io/crates/html2pdf , https://crates.io/crates/pdf-writer , https://crates.io/crates/pdfium-render
