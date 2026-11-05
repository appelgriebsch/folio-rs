//! PDF checks for the integration tests. The library keeps its own copy under `#[cfg(test)]`.

use printpdf::{PdfDocument, PdfParseOptions};

/// Pull visible text back out of a saved PDF. Tests use this instead of poppler.
pub(crate) fn pdf_text(bytes: &[u8]) -> String {
    let mut warnings = Vec::new();
    let doc = PdfDocument::parse(
        bytes,
        &PdfParseOptions {
            fail_on_error: false,
        },
        &mut warnings,
    )
    .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
    doc.extract_text()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn pdf_info(bytes: &[u8]) -> printpdf::PdfDocumentInfo {
    let mut warnings = Vec::new();
    let doc = PdfDocument::parse(
        bytes,
        &PdfParseOptions {
            fail_on_error: false,
        },
        &mut warnings,
    )
    .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
    doc.metadata.info
}

pub(crate) fn pdf_font_names(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for key in [b"/BaseFont" as &[u8], b"/FontName"] {
        let mut rest = bytes;
        while let Some(at) = rest.windows(key.len()).position(|window| window == key) {
            let after = &rest[at + key.len()..];
            let start = after
                .iter()
                .position(|byte| !byte.is_ascii_whitespace() && *byte != b'/')
                .unwrap_or(after.len());
            let after = &after[start..];
            let end = after
                .iter()
                .position(|byte| {
                    byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>' | b']' | b')')
                })
                .unwrap_or(after.len());
            if let Ok(name) = std::str::from_utf8(&after[..end])
                && !name.is_empty()
            {
                names.push(name.to_string());
            }
            rest = &rest[at + key.len()..];
        }
    }
    names.sort();
    names.dedup();
    names
}

pub(crate) fn pdf_has_image(bytes: &[u8]) -> bool {
    let mut warnings = Vec::new();
    let doc = PdfDocument::parse(
        bytes,
        &PdfParseOptions {
            fail_on_error: false,
        },
        &mut warnings,
    )
    .unwrap_or_else(|err| panic!("pdf parse failed: {err}"));
    !doc.resources.xobjects.map.is_empty()
}
