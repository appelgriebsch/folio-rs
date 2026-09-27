# folio

`folio` fetches one web article and writes an EPUB.

It reads the HTML the server returns. It does not run JavaScript, log in, or follow a chain of next pages. The file on stdout is the path of the EPUB. Warnings go to stderr, and a warning still exits 0.

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

With no output path, the file is `./<title>.epub` in the current directory. The name comes from the article title: letters and numbers stay, other runs become hyphens, and the result is lowercased.

```sh
folio -o out/story.epub https://example.com/story
folio -o story.epub --force https://example.com/story
folio --format epub -v https://example.com/story
```

`-o` may be written as `-ostory.epub`. `--format` needs two dashes. `-f` is `--force`, not the format.

| Option | Meaning |
| --- | --- |
| URL | `http` or `https` article address. Required. |
| `-o`, `--output` PATH | Output file. `-` is rejected. The parent directory must already exist. |
| `-f`, `--force` | Replace an existing output file after the new EPUB is complete. |
| `--format epub` | Output format. EPUB is the only format. A `.pdf` path is rejected. |
| `--max-pages` N | Accepted, from 1 to 15, default 15. This version still fetches one page. |
| `-v`, `--verbose` | Print the fetch line even when stderr is not a terminal. |

Exit 2 means the command line or the URL was rejected. Exit 1 means the fetch, the article, or the write failed. If the page has no article text, folio says that it only reads the fetched HTML and does not run JavaScript.

## What the EPUB contains

The chapter is the article: title, byline and date when the page provides them, then the body. A same-origin reader view is used when the page has exactly one reader link. Otherwise the article page itself is used.

Images in the article are embedded when they are JPEG, PNG, or GIF, within the size cap. Other images are skipped with a warning. Each image scales to at most the screen width.

The table of contents has two levels. A single title heading is the chapter, and the entries under it are `h2` sections with `h3` subsections. When the page has several `h1` headings, those are the first level and `h2` headings are the second.

The cover is the page's share image, in this order: `og:image`, then `twitter:image`, then the article's JSON-LD image. If that file cannot be fetched, the EPUB is still written and the image is omitted with a warning.

## License

MIT.
