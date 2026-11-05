# folio-worker

The `folio` pipeline as a Cloudflare Worker: one HTTP request in, one EPUB or
PDF file out.

## API

```
GET /?sourceUrl=<article url>&format=epub|pdf
POST /        {"sourceUrl": "<article url>", "format": "epub" | "pdf"}
GET /health    -> "ok"
```

- `sourceUrl` is required and must be a public `http` or `https` URL:
  loopback, link-local (including `169.254.169.254`), private, CGNAT, and
  other non-public IP ranges, plus `localhost`-style names, are rejected with
  `400`. `format` defaults to `epub`.
- Success: `200` with the file body, `Content-Type` of
  `application/epub+zip` or `application/pdf`, and
  `Content-Disposition: attachment; filename="<ascii-slug>.<ext>";
  filename*=UTF-8''<percent-encoded-slug>.<ext>` (RFC 5987).
- Errors are JSON `{"error": "..."}` with a status: `400` bad parameters,
  `422` no article text, no title, or no usable file name on the page,
  `502` fetch or render failure, `501` PDF requested without the Browser
  Rendering binding, `500` anything else.

EPUB is built entirely inside the Worker from the same pipeline as the CLI
(reader view, sanitized article, embedded images, generated cover). PDF is
rendered from the composed article by the account's
[Browser Rendering](https://developers.cloudflare.com/browser-rendering/)
binding (`BROWSER` in `wrangler.toml`), because the CLI's printpdf renderer
does not compile to WebAssembly. The article images are inlined as data URIs,
so the headless browser makes no network requests of its own.

## Build and deploy

```sh
cd worker
cargo install worker-build   # or: npm install -g worker-build
worker-build --release       # writes build/worker/shim.js + the wasm bundle
npx wrangler deploy          # uses wrangler.toml
```

Develop locally with `npx wrangler dev`.

## Notes

- The Worker makes outbound requests to `sourceUrl` and to the article's
  images; set the `User-Agent` expectation of the target sites accordingly.
  `sourceUrl` is validated with `folio_rs::service::require_public_http_url`,
  which rejects loopback, link-local, private, and other non-public hosts.
- ponytail: the Worker itself has no authentication or rate limiting, and
  redirect or image targets are only constrained by Cloudflare's fetch path.
  It is an outbound fetcher by design; do not expose it broadly without
  Cloudflare Access, WAF, or similar in front.
- Browser Rendering requires the Browser Rendering product on the account;
  without the binding, `format=pdf` answers `501`. Browser calls are raced
  against a 30 s timeout and the rendered PDF is capped at 32 MiB.
- Worker CPU time limits apply to cover-image processing on very large
  share images; the same 4 MiB image cap as the CLI is enforced.
