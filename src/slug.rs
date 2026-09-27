use unicode_normalization::UnicodeNormalization;

const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Filename stem for the article title.
///
/// NFKC, case-fold, drop apostrophes, collapse other non-letter non-number runs
/// into one hyphen, trim hyphens and dots, then cap at 80 characters and 200 bytes.
pub fn slug_from_title(title: &str) -> Option<String> {
    // NFKC splits ligatures. Unicode lowercase plus sharp-s covers the
    // length-changing default case fold that slug filenames need.
    let folded: String = title
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .replace('ß', "ss");
    let mut buf = String::new();
    let mut hyphen = false;
    for ch in folded.chars() {
        if is_apostrophe(ch) {
            continue;
        }
        if ch.is_alphanumeric() {
            buf.push(ch);
            hyphen = false;
        } else if !buf.is_empty() && !hyphen {
            buf.push('-');
            hyphen = true;
        }
    }
    let trimmed = buf.trim_matches(|c| c == '-' || c == '.').to_string();
    let mut out = String::new();
    for ch in trimmed.chars() {
        let next_bytes = out.len() + ch.len_utf8();
        if out.chars().count() >= 80 || next_bytes > 200 {
            break;
        }
        out.push(ch);
    }
    let out = out.trim_end_matches(|c| c == '-' || c == '.').to_string();
    if out.is_empty() || out.starts_with('.') || DEVICE_NAMES.contains(&out.as_str()) {
        None
    } else {
        Some(out)
    }
}

fn is_apostrophe(ch: char) -> bool {
    matches!(
        ch,
        '\'' | '\u{2019}' | '\u{2018}' | '\u{201B}' | '\u{02BC}' | '\u{FF07}'
    )
}

#[cfg(test)]
mod tests {
    use super::slug_from_title;

    #[test]
    fn folds_apostrophes_and_case() {
        assert_eq!(slug_from_title("Don't Stop").as_deref(), Some("dont-stop"));
        assert_eq!(
            slug_from_title("Straße Café").as_deref(),
            Some("strasse-café")
        );
    }

    #[test]
    fn nfkc_and_punctuation() {
        assert_eq!(slug_from_title("ﬁle name").as_deref(), Some("file-name"));
        assert_eq!(
            slug_from_title("  Hello---World!! ").as_deref(),
            Some("hello-world")
        );
    }

    #[test]
    fn rejects_empty_and_device_names() {
        assert_eq!(slug_from_title("..."), None);
        assert_eq!(slug_from_title("CON"), None);
        assert_eq!(slug_from_title("lpt3"), None);
        assert_eq!(slug_from_title("!!!"), None);
    }

    #[test]
    fn caps_characters_and_bytes() {
        let long = "a".repeat(200);
        let slug = slug_from_title(&long).unwrap();
        assert_eq!(slug.chars().count(), 80);
        assert!(slug.len() <= 200);

        let wide = "你".repeat(100);
        let slug = slug_from_title(&wide).unwrap();
        assert!(slug.len() <= 200);
        assert!(slug.chars().count() <= 80);
        assert_eq!(slug.len() % "你".len(), 0);
    }
}
