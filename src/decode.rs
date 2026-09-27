use encoding_rs::Encoding;

/// Decode HTML bytes without `Response::text`.
///
/// Charset comes from the `Content-Type` header when it has one, otherwise from
/// an HTML meta prescan, otherwise UTF-8.
pub fn decode_html(bytes: &[u8], content_type: Option<&str>) -> String {
    let encoding = content_type
        .and_then(charset_from_content_type)
        .or_else(|| prescan_charset(bytes))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

pub fn charset_from_content_type(content_type: &str) -> Option<&'static Encoding> {
    let lower = content_type.to_ascii_lowercase();
    for part in lower.split(';').skip(1) {
        let part = part.trim();
        let Some(value) = part.strip_prefix("charset=") else {
            continue;
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if value.is_empty() {
            continue;
        }
        if let Some(encoding) = Encoding::for_label(value.as_bytes()) {
            return Some(encoding);
        }
    }
    None
}

pub fn prescan_charset(input: &[u8]) -> Option<&'static Encoding> {
    let bytes = &input[..input.len().min(2048)];
    let lower = bytes.to_ascii_lowercase();
    let mut i = 0;
    while i + 5 < lower.len() {
        let is_meta = lower[i..].starts_with(b"<meta")
            && (i + 5 == lower.len() || !lower[i + 5].is_ascii_alphanumeric());
        if !is_meta {
            i += 1;
            continue;
        }
        i += 5;
        let mut attrs = Vec::new();
        while i < lower.len() && lower[i] != b'>' {
            while i < lower.len() && (lower[i].is_ascii_whitespace() || lower[i] == b'/') {
                i += 1;
            }
            if i >= lower.len() || lower[i] == b'>' {
                break;
            }
            let name_start = i;
            while i < lower.len()
                && !lower[i].is_ascii_whitespace()
                && lower[i] != b'='
                && lower[i] != b'>'
                && lower[i] != b'/'
            {
                i += 1;
            }
            let name = String::from_utf8_lossy(&lower[name_start..i]).into_owned();
            while i < lower.len() && lower[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut value = String::new();
            if i < lower.len() && lower[i] == b'=' {
                i += 1;
                while i < lower.len() && lower[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < lower.len() && (lower[i] == b'"' || lower[i] == b'\'') {
                    let quote = lower[i];
                    i += 1;
                    let value_start = i;
                    while i < lower.len() && lower[i] != quote {
                        i += 1;
                    }
                    value = String::from_utf8_lossy(&lower[value_start..i]).into_owned();
                    if i < lower.len() {
                        i += 1;
                    }
                } else {
                    let value_start = i;
                    while i < lower.len() && !lower[i].is_ascii_whitespace() && lower[i] != b'>' {
                        i += 1;
                    }
                    value = String::from_utf8_lossy(&lower[value_start..i]).into_owned();
                }
            }
            if !name.is_empty() {
                attrs.push((name, value));
            }
        }
        if let Some((_, charset)) = attrs.iter().find(|(name, _)| name == "charset") {
            let charset = charset.trim();
            if let Some(encoding) = Encoding::for_label(charset.as_bytes()) {
                return Some(encoding);
            }
        }
        let content_type = attrs.iter().any(|(name, value)| {
            name == "http-equiv"
                && value
                    .split_whitespace()
                    .any(|token| token == "content-type")
        });
        if content_type {
            if let Some((_, content)) = attrs.iter().find(|(name, _)| name == "content") {
                if let Some(encoding) = charset_from_content_type(content) {
                    return Some(encoding);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::decode_html;

    #[test]
    fn content_type_beats_meta() {
        let bytes = b"<meta charset=\"utf-8\"><title>\xE9</title>";
        let text = decode_html(bytes, Some("text/html; charset=ISO-8859-1"));
        assert!(text.contains('é'), "{text}");
        assert!(!text.contains('\u{FFFD}'));
    }

    #[test]
    fn meta_prescan_when_header_has_no_charset() {
        let bytes = b"<html><head><meta charset=\"iso-8859-1\"></head><body>\xE9</body>";
        let text = decode_html(bytes, Some("text/html"));
        assert!(text.contains('é'), "{text}");
    }

    #[test]
    fn http_equiv_prescan() {
        let bytes =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=ISO-8859-1\"><p>\xE9";
        let text = decode_html(bytes, None);
        assert!(text.contains('é'), "{text}");
    }

    #[test]
    fn defaults_to_utf8() {
        let text = decode_html("café".as_bytes(), None);
        assert_eq!(text, "café");
    }
}
