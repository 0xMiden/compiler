use anyhow::Ok;

fn main() -> anyhow::Result<()> {
    // Initialize logger
    let mut builder = midenc_log::Builder::from_env("CARGO_MIDEN_LOG");
    builder.format_indent(Some(2));
    builder.format_timestamp(None);
    builder.init();

    if let Err(e) = cargo_miden::run(std::env::args()) {
        eprintln!("{e:?}");
        std::process::exit(1);
    }
    Ok(())
}
