use anyhow::Ok;
use cargo_miden::{run, Color, OutputType, Terminal, Verbosity};

fn main() -> anyhow::Result<()> {
    // Initialize logger
    let mut builder = env_logger::Builder::from_env("CARGO_MIDEN_LOG");
    builder.format_indent(Some(2));
    builder.format_timestamp(None);
    builder.init();

    if let Err(e) = run(std::env::args(), OutputType::Masm) {
        let terminal = Terminal::new(Verbosity::Normal, Color::Auto);
        terminal.error(format!("{e}"))?;
        std::process::exit(1);
    }
    Ok(())
}
