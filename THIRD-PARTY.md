# Third-party bill of materials

Direct dependencies from `Cargo.toml` / `Cargo.lock`, plus the GitHub Actions used by `.github/workflows/ci.yml`. OSV.dev was queried for the locked crate versions. `ttf-parser` 0.25.1 is [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192) (unmaintained; no CVE). Andreas accepted it on 2026-11-02: it comes in through `ab_glyph`, there is no CVE, and there is no maintained drop-in. Revisit when a maintained replacement exists. No other advisory was reported. No CVE was found for the action refs below.

| Name | Version | License | Critical update | CVEs |
| --- | --- | --- | --- | --- |
| ab_glyph | 0.2.32 | Apache-2.0 | | none found |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | | none found |
| actions/checkout | v7 (v7.0.1, `3d3c42e5aac5ba805825da76410c181273ba90b1`) | MIT | | none found |
| ammonia | 4.2.0 | MIT OR Apache-2.0 | | none found |
| clap | 4.6.7 | MIT OR Apache-2.0 | | none found |
| ctrlc | 3.5.2 | MIT/Apache-2.0 | | none found |
| dom_query | 0.27.0 | MIT | | none found |
| dom_smoothie | 0.18.2 | MIT | | none found |
| dtolnay/rust-toolchain | stable | MIT | | none found |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | | none found |
| IBM Plex Serif | 2.0.0 (font 3.006) | OFL-1.1, Reserved Font Name "Plex" | | not checked |
| image | 0.25.10 | MIT OR Apache-2.0 | | none found |
| image-webp | 0.2.4 | MIT OR Apache-2.0 | | none found |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | | none found |
| rbook | 0.7.10 | Apache-2.0 | | none found |
| reqwest | 0.13.5 | MIT OR Apache-2.0 | | none found |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | | none found |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | | none found |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | RUSTSEC-2026-0192 (unmaintained; accepted 2026-11-02, revisit on replacement) | none found |
| unicode-linebreak | 0.1.5 | Apache-2.0 | | none found |
| unicode-normalization | 0.1.25 | MIT OR Apache-2.0 | | none found |
| url | 2.5.8 | MIT OR Apache-2.0 | | none found |
| zip | 8.6.0 | MIT | | none found |

IBM Plex Serif is the unmodified upstream `IBMPlexSerif-Bold.ttf` (220924 bytes). The implementation plan estimated about 161 KB; the 2.0.0 file is larger, and it was not subset or renamed.
