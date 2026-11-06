# folio-worker

The `folio` pipeline as a Cloudflare Worker: one HTTP request in, one EPUB or
PDF file out.

## API

```
GET  /?sourceUrl=<article url>[&format=epub]   -> EPUB download (public, rate limited)
POST /          {"sourceUrl": "<article url>"} -> EPUB download (public, rate limited)
GET  /pdf?sourceUrl=<article url>              -> PDF download (bearer auth, rate limited)
POST /pdf       {"sourceUrl": "<article url>"} -> PDF download (bearer auth, rate limited)
GET  /health    -> "ok" (open)
```

- `sourceUrl` is required and must be a public `http` or `https` URL:
  loopback, link-local (including `169.254.169.254`), private, CGNAT, and
  other non-public IP ranges, plus `localhost`-style names, are rejected with
  `400`.
- EPUB is generated on `/` and defaults to `format=epub`; asking for
  `format=pdf` on `/` answers `400` and points at `/pdf`.
- The `/pdf` route requires `Authorization: Bearer <token>` with the
  `FOLIO_API_TOKEN` Worker secret (`wrangler secret put FOLIO_API_TOKEN`).
  A missing or wrong token answers `401` before any fetching or rendering;
  the secret missing entirely answers `500`. Rotating the token means
  updating the Worker secret and the GitHub secret together.
- Both generate routes check the `GENERATE_LIMIT` rate-limit binding
  (per client IP via `CF-Connecting-IP`, 10 requests per 60 s) and answer
  `429` when over the limit. If the binding is not configured the generate
  routes answer `500` rather than running unthrottled. `/health` is not
  rate limited.
- Success: `200` with the file body, `Content-Type` of
  `application/epub+zip` or `application/pdf`, and
  `Content-Disposition: attachment; filename="<ascii-slug>.<ext>";
  filename*=UTF-8''<percent-encoded-slug>.<ext>` (RFC 5987).
- Errors are JSON `{"error": "..."}` with a status: `400` bad parameters,
  `401` missing or wrong bearer token on `/pdf`, `422` no article text, no
  title, or no usable file name on the page, `429` rate limit exceeded,
  `502` fetch or render failure, `501` PDF requested without the Browser
  Rendering binding, `500` anything else (including a missing
  `FOLIO_API_TOKEN` secret or rate-limit binding).

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
- ponytail: only the PDF route is authenticated (shared bearer token), the
  EPUB route is public behind the rate limit, and redirect or image targets
  are only constrained by Cloudflare's fetch path. It is an outbound
  fetcher by design; do not expose it broadly without Cloudflare Access,
  WAF, or similar in front.
- Browser Rendering requires the Browser Rendering product on the account;
  without the binding, `format=pdf` answers `501`. Browser calls are raced
  against a 30 s timeout and the rendered PDF is capped at 32 MiB.
- Worker CPU time limits apply to cover-image processing on very large
  share images; the same 4 MiB image cap as the CLI is enforced.
