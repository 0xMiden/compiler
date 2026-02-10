use anyhow::Ok;
use cargo_miden::{OutputType, run};

fn main() -> anyhow::Result<()> {
    // Initialize logger
    let mut builder = midenc_log::Builder::from_env("CARGO_MIDEN_LOG");
    builder.format_indent(Some(2));
    builder.format_timestamp(None);
    builder.init();

    if let Err(e) = run(std::env::args(), OutputType::Masm) {
        eprintln!("{e:?}");
        std::process::exit(1);
    }
    Ok(())
}
