use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Epub,
}

#[derive(Debug, Parser)]
#[command(
    name = "folio",
    version,
    about = "Write an EPUB of one web article",
    arg_required_else_help = true
)]
struct Cli {
    /// Article URL. Only http and https are accepted.
    #[arg(value_name = "URL")]
    url: String,

    /// Output format. Long-only so -f stays force. Only epub in this version.
    #[arg(long, value_enum)]
    format: Option<Format>,

    /// Output path. `-` is rejected.
    #[arg(short, long, value_name = "PATH")]
    output: Option<PathBuf>,

    /// Replace an existing output file after the new bytes are complete.
    #[arg(short, long)]
    force: bool,

    /// Page cap, including the first page. Accepted now; this version fetches one page.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u8).range(1..=15))]
    max_pages: u8,

    /// Print the fetch line even when stderr is not a terminal.
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub url: String,
    pub format: Option<Format>,
    pub output: Option<PathBuf>,
    pub force: bool,
    pub max_pages: u8,
    pub verbose: bool,
}

pub fn parse() -> Result<Options, i32> {
    match Cli::try_parse() {
        Ok(cli) => Ok(Options {
            url: cli.url,
            format: cli.format,
            output: cli.output,
            force: cli.force,
            max_pages: cli.max_pages,
            verbose: cli.verbose,
        }),
        Err(err) => {
            let _ = err.print();
            Err(err.exit_code())
        }
    }
}

pub fn require_http_url(raw: &str) -> Result<url::Url, Error> {
    let url = url::Url::parse(raw).map_err(|_| Error::Usage(format!("invalid url: {raw}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(Error::Usage("url must use http or https".to_string()));
    }
    if url.host_str().is_none() {
        return Err(Error::Usage(format!("invalid url: {raw}")));
    }
    Ok(url)
}
