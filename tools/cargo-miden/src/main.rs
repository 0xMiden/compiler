use cargo_miden::CommandOutput;
use midenc_log::SuppressKnownDependencyErrors;

/// Initializes the global logger, suppressing the known-harmless dependency errors.
fn init_logger() {
    let mut builder = midenc_log::Builder::from_env("CARGO_MIDEN_LOG");
    builder.format_indent(Some(2));
    builder.format_timestamp(None);
    let logger = builder.build();
    let max_level = logger.filter();
    log::set_boxed_logger(Box::new(SuppressKnownDependencyErrors::new(logger)))
        .expect("logger already initialized");
    log::set_max_level(max_level);
}

fn main() -> anyhow::Result<()> {
    init_logger();

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
