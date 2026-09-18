/// Initializes the global logger, configured exactly as `midenc`'s is.
fn init_logger() -> anyhow::Result<()> {
    let (logger, max_level) = midenc_log::midenc_logger().map_err(|err| anyhow::anyhow!(err))?;
    log::set_boxed_logger(logger).expect("logger already initialized");
    log::set_max_level(max_level);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    init_logger()?;

    match cargo_miden::run(std::env::args()) {
        // Nothing is printed for a finished build: the driver has already announced the package
        // it wrote, and this is the same build.
        Ok(_) => {}
        Err(e) => {
            eprintln!("{e:?}");
            std::process::exit(1);
        }
    }
    Ok(())
}
