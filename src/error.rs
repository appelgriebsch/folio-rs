use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),

    #[error("missing title at {0}")]
    MissingTitle(String),

    #[error("no article text at {url}")]
    NoArticle { url: String },

    #[error("output already exists: {0}; pass --force to replace it")]
    Exists(String),

    #[error("output path is a directory: {0}")]
    IsDir(String),

    #[error("output directory does not exist: {0}")]
    NoParent(String),

    #[error("output parent is not a directory: {0}")]
    ParentNotDir(String),

    #[error("cannot use output path {path}: {reason}")]
    UnusablePath { path: String, reason: String },

    #[error("cannot name the output file from the title; pass -o")]
    BadSlug,

    #[error("failed to fetch {url}: {reason}")]
    Fetch { url: String, reason: String },

    #[error("article is too large to read at {0}")]
    ArticleTooLarge(String),

    #[error("response exceeds the byte cap for {0}")]
    TooLarge(String),

    #[error("failed to write the epub: {0}")]
    Write(String),
}

impl Error {
    pub fn code(&self) -> i32 {
        match self {
            Error::Usage(_) => 2,
            _ => 1,
        }
    }
}
