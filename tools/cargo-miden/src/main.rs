use cargo_miden::CommandOutput;

fn main() -> anyhow::Result<()> {
    // Initialize logger
    let mut builder = midenc_log::Builder::from_env("CARGO_MIDEN_LOG");
    builder.format_indent(Some(2));
    builder.format_timestamp(None);
    builder.init();

    match cargo_miden::run(std::env::args()) {
        Ok(Some(CommandOutput::BuildCommandOutput { output })) => {
            for artifact_path in output {
                println!("Compiled {}", artifact_path.display());
            }
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("{e:?}");
            std::process::exit(1);
        }
    }
    Ok(())
}
