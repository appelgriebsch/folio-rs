# folio

`folio` fetches one web article and writes an EPUB or a PDF.

It reads the HTML the server returns. It does not run JavaScript, log in, or follow a chain of next pages. The file on stdout is the path of the file. Warnings go to stderr, and a warning still exits 0.

## Build

Rust 1.88 or newer:

```sh
cargo build --release
./target/release/folio --help
```

The binary is named `folio`.

## Usage

```sh
folio https://example.com/story
```

With no output path, the file is `./<title>.epub` in the current directory, or `./<title>.pdf` when the format is PDF. The name comes from the article title: letters and numbers stay, other runs become hyphens, and the result is lowercased. If `--format` is omitted and `-o` ends in `.pdf` or `.epub`, that extension picks the format. A flag that disagrees with that extension exits 2 and writes nothing.

```sh
folio -o out/story.epub https://example.com/story
folio -o story.epub --force https://example.com/story
folio --format pdf -v https://example.com/story
```

`-o` may be written as `-ostory.epub`. `--format` needs two dashes. `-f` is `--force`, not the format.

| Option | Meaning |
| --- | --- |
| URL | `http` or `https` article address. Required. |
| `-o`, `--output` PATH | Output file. `-` is rejected. The parent directory must already exist. |
| `-f`, `--force` | Replace an existing output file after the new file is complete. |
| `--format pdf\|epub` | Output format. Default `epub`. `.pdf` and `.epub` paths must agree with the flag. |
| `--max-pages` N | Accepted, from 1 to 15, default 15. This version still fetches one page. |
| `-v`, `--verbose` | Print the fetch line even when stderr is not a terminal, and why a reader or print link was chosen. |

Exit 2 means the command line or the URL was rejected. Exit 1 means the fetch, the article, or the write failed. If the page has no article text, folio says that it only reads the fetched HTML and does not run JavaScript.

## What the PDF contains

`--format pdf` writes one A4 PDF (210×297 mm, 20 mm margins) of the same article. It starts with the title, then the byline and date when the page has them, then the body. There is no cover page. Text is IBM Plex Serif, bundled with the program. System fonts are not used. If the article contains a character that font cannot draw, folio prints one warning and still writes the PDF.

When the article page has exactly one same-origin print link, the PDF is built from that page and the EPUB is not. The link's accessible name, as a whole string, is one of: print, print article, print this article, print version, printable version, printer-friendly, printer friendly, print view, Druckansicht, Druckversion. A path segment may be exactly `print`, `printable`, or `printer-friendly`. A print page that cannot be fetched or composed warns and uses the article page. A `media="print"` stylesheet is not applied.

The info dictionary carries the title, the author, and the description as the subject. The creation date is the article's publish date when the page has one: `YYYY-MM-DD` or `YYYY/MM/DD`, optionally with a time whose seconds may be omitted. A date-only value is midnight UTC. A timestamp keeps its offset. An impossible calendar day or clock is ignored. With no usable publish date, the creation date is the file time, and the modification date is always the file time.

JPEG, PNG, and GIF images are embedded. An SVG is omitted with a warning and the PDF is still written.

## What the EPUB contains

The chapter is the article: title, byline and date when the page provides them, then the body. A same-origin reader view is used when the page has exactly one reader link. Otherwise the article page itself is used.

Images in the article are embedded when they are JPEG, PNG, or GIF, within the size cap. Other images are skipped with a warning. Each image scales to at most the screen width.

The table of contents has two levels. A single title heading is the chapter, and the entries under it are `h2` sections with `h3` subsections. When the page has several `h1` headings, those are the first level and `h2` headings are the second.

The cover is always a portrait JPEG (`images/cover.jpg`, with `cover.xhtml` first in the spine). Folio tries up to three share images, the typed page's banner first, in this order: `og:image`, then `twitter:image`, then the article's JSON-LD image. A usable image is JPEG, PNG, GIF, or WebP and at least 400 px wide. A landscape or square image sits near the top on a solid fill: the image's dominant colour darkened for white text, or off-white with black text when the image is flat and light. If none can be used, the cover is typographic: the title in IBM Plex Serif on a dark colour chosen from the host name. A title the font cannot draw, or that needs shaping (Arabic, Hebrew, Thai, and similar scripts), is replaced by the site name. Each image that cannot be used is omitted with a warning, and the EPUB is still written.

## Web service (Cloudflare Workers)

The `worker/` directory is a [workers-rs](https://github.com/cloudflare/workers-rs)
service that exposes the same pipeline over HTTP: pass `sourceUrl` and
`format` (`epub` or `pdf`) and it returns the generated file for download.
EPUB is rendered in-process; PDF is rendered from the composed article through
the account's Browser Rendering binding. See [worker/README.md](worker/README.md)
for the API and deploy steps.

## License

MIT.
