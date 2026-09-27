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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !shorts_are_only_f_v_or_o(&args) {
        eprintln!("error: --format takes two dashes; -f is --force");
        return Err(2);
    }
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

/// `-f` and `-v` may cluster. `-o` is its own token or `-o` glued to a path.
/// A one-dash word such as `-format` is rejected so Clap cannot read it as
/// `--force` plus an output file named `rmat`.
pub(crate) fn shorts_are_only_f_v_or_o(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            break;
        }
        if let Some(body) = arg.strip_prefix("--") {
            let name = body.split('=').next().unwrap_or(body);
            if matches!(name, "output" | "format" | "max-pages") && !body.contains('=') {
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            if arg == "-h" || arg == "-V" {
                index += 1;
                continue;
            }
            let body = &arg[1..];
            if let Some(path) = body.strip_prefix('o') {
                index += if path.is_empty() { 2 } else { 1 };
                continue;
            }
            if !body.is_empty() && body.bytes().all(|byte| byte == b'f' || byte == b'v') {
                index += 1;
                continue;
            }
            return false;
        }
        index += 1;
    }
    true
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

#[cfg(test)]
mod tests {
    use super::shorts_are_only_f_v_or_o;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn one_dash_format_is_rejected_and_real_shorts_are_kept() {
        assert!(!shorts_are_only_f_v_or_o(&args(&[
            "-format",
            "http://example.test"
        ])));
        assert!(!shorts_are_only_f_v_or_o(&args(&["-fo", "story.epub"])));
        assert!(shorts_are_only_f_v_or_o(&args(&[
            "-fv",
            "-o",
            "story.epub",
            "--format",
            "epub",
            "http://example.test",
        ])));
        assert!(shorts_are_only_f_v_or_o(&args(&[
            "-ostory.epub",
            "http://example.test"
        ])));
    }
}
