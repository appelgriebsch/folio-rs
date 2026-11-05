#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Epub,
    Pdf,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Epub => "epub",
            Format::Pdf => "pdf",
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Format::Epub => "application/epub+zip",
            Format::Pdf => "application/pdf",
        }
    }

    /// Case-insensitive `epub` or `pdf`, as used by the web service's `format` parameter.
    pub fn parse(word: &str) -> Option<Format> {
        match word.trim().to_ascii_lowercase().as_str() {
            "epub" => Some(Format::Epub),
            "pdf" => Some(Format::Pdf),
            _ => None,
        }
    }
}
