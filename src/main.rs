#![forbid(unsafe_code)]

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = runtime.block_on(folio_rs::run());
    std::process::exit(code);
}
