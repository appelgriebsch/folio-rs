#![forbid(unsafe_code)]

mod article;
mod cli;
mod decode;
mod error;
mod fetch;
mod images;
mod meta;
mod output;
mod publish;
mod reader;
mod sanitize;
mod slug;
mod xhtml;

use crate::error::Error;

pub fn run() -> i32 {
    output::install_interrupt_handler();
    let opts = match cli::parse() {
        Ok(opts) => opts,
        Err(code) => return code,
    };
    match publish::publish(opts) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(err) => report(&err),
    }
}

fn report(err: &Error) -> i32 {
    if let Error::NoArticle { url } = err {
        eprintln!("error: no article text at {url}");
        eprintln!("folio only reads the fetched HTML and does not run JavaScript");
        return 1;
    }
    eprintln!("error: {err}");
    err.code()
}
