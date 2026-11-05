#![forbid(unsafe_code)]

mod article;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod cover;
mod decode;
pub mod error;
mod fetch;
pub mod format;
mod images;
mod meta;
#[cfg(not(target_arch = "wasm32"))]
mod output;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pdf;
mod publish;
mod reader;
mod sanitize;
pub mod service;
mod slug;
mod toc;
mod xhtml;

pub use error::Error;
pub use format::Format;
pub use service::{Rendered, RenderRequest};

/// Diagnostics go to stderr on native targets. On wasm32 there is no stderr,
/// so the line is dropped; the worker surfaces errors in its HTTP response.
pub(crate) fn log_line(message: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
    #[cfg(target_arch = "wasm32")]
    let _ = message;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn run() -> i32 {
    output::install_interrupt_handler();
    let opts = match cli::parse() {
        Ok(opts) => opts,
        Err(code) => return code,
    };
    match publish::publish(opts).await {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(err) => report(&err),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn report(err: &Error) -> i32 {
    if let Error::NoArticle { url } = err {
        eprintln!("error: no article text at {url}");
        eprintln!("folio only reads the fetched HTML and does not run JavaScript");
        return 1;
    }
    eprintln!("error: {err}");
    err.code()
}
