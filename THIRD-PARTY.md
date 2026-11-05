# Third-party bill of materials

Direct dependencies from `Cargo.toml` / `Cargo.lock`, plus the GitHub Actions used by `.github/workflows/ci.yml`. OSV.dev was queried for the locked crate versions. `ttf-parser` 0.25.1 is [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192) (unmaintained; no CVE). Andreas accepted it on 2026-11-02: it comes in through `ab_glyph`, there is no CVE, and there is no maintained drop-in. Revisit when a maintained replacement exists. `bincode` 1.3.3 is [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141) (unmaintained; no CVE). Andreas accepted it on 2026-11-05: it comes in through `printpdf` → `azul-layout` → `hyphenation` 0.8.4 (also as a build dependency), printpdf's `html` feature requires `text_layout_hyphenation`, so it cannot be turned off, and `hyphenation` 0.8.4 is the latest release. Revisit when printpdf or hyphenation drops it. No other advisory was reported. No CVE was found for the action refs below.

| Name | Version | License | Critical update | CVEs |
| --- | --- | --- | --- | --- |
| ab_glyph | 0.2.32 | Apache-2.0 | | none found |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | | none found |
| actions/checkout | v7 (v7.0.1, `3d3c42e5aac5ba805825da76410c181273ba90b1`) | MIT | | none found |
| allsorts-azul | 0.17.2 | MIT OR Apache-2.0 | | none found |
| ammonia | 4.2.0 | MIT OR Apache-2.0 | | none found |
| azul-core | 0.0.16 | MIT | | none found |
| azul-css | 0.0.16 | MIT | | none found |
| azul-layout | 0.0.16 | MIT | | none found |
| bincode | 1.3.3 | MIT | RUSTSEC-2025-0141 (unmaintained; accepted 2026-11-05, revisit when printpdf/hyphenation drops it) | none found |
| azul-simplecss | 0.2.3 | MPL-2.0 | weak copyleft; pulled by azul-css; accepted 2026-11-05 | none found |
| clap | 4.6.7 | MIT OR Apache-2.0 | | none found |
| ctrlc | 3.5.2 | MIT/Apache-2.0 | | none found |
| dom_query | 0.27.0 | MIT | | none found |
| dom_smoothie | 0.18.2 | MIT | | none found |
| dtolnay/rust-toolchain | stable | MIT | | none found |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | | none found |
| hyphenation | 0.8.4 | Apache-2.0 OR MIT | | none found |
| IBM Plex Serif | 2.0.0 (font 3.006) | OFL-1.1, Reserved Font Name "Plex" | | not checked |
| image | 0.25.10 | MIT OR Apache-2.0 | | none found |
| image-webp | 0.2.4 | MIT OR Apache-2.0 | | none found |
| lopdf | 0.44.0 | MIT | | none found |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | | none found |
| printpdf | 0.12.8 | MIT | | none found |
| rbook | 0.7.10 | Apache-2.0 | | none found |
| reqwest | 0.13.5 | MIT OR Apache-2.0 | | none found |
| rust-fontconfig | 5.0.2 | MIT | | none found |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | | none found |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | | none found |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | RUSTSEC-2026-0192 (unmaintained; accepted 2026-11-02, revisit on replacement) | none found |
| unicode-linebreak | 0.1.5 | Apache-2.0 | | none found |
| unicode-normalization | 0.1.25 | MIT OR Apache-2.0 | | none found |
| url | 2.5.8 | MIT OR Apache-2.0 | | none found |
| xmlparser | 0.13.6 | MIT OR Apache-2.0 | | none found |
| zip | 8.6.0 | MIT | | none found |

Rows for `printpdf` and the crates it pulls in (`allsorts-azul`, `azul-core`, `azul-css`, `azul-layout`, `azul-simplecss`, `hyphenation`, `lopdf`, `rust-fontconfig`) are the direct dependency plus the layout, font, and PDF crates. `xmlparser` is a direct dependency. Smaller transitive crates stay in `Cargo.lock`.

`azul-simplecss` 0.2.3 is MPL-2.0. `printpdf` reaches it through `azul-layout` → `azul-css`. MPL-2.0 is weak copyleft on that file, not MIT/Apache/BSD. Andreas accepted it on 2026-11-05: it is used unmodified from crates.io. `r-efi` 6.0.0 is MIT OR Apache-2.0 OR LGPL-2.1-or-later (the LGPL term is optional). `cssparser` and `selectors` (MPL-2.0) were already in the tree through `dom_query`.

IBM Plex Serif is the unmodified upstream files `IBMPlexSerif-Regular.ttf` (217332 bytes), `IBMPlexSerif-Italic.ttf` (227404 bytes), and `IBMPlexSerif-Bold.ttf` (220924 bytes). The implementation plan estimated about 161 KB for Bold; the 2.0.0 files are larger, and they were not subset or renamed.
