use std::env;

use midenc_driver::{
    self as driver, ClapDiagnostic,
    diagnostics::{IntoDiagnostic, Report, WrapErr},
};

pub fn main() -> Result<(), Report> {
    if cfg!(not(debug_assertions)) && env::var_os("MIDENC_TRACE").is_none() {
        human_panic::setup_panic!();
    }

    // Initialize logger, but do not install it, leave that up to the command handler
    let mut builder = midenc_log::Builder::from_env("MIDENC_TRACE");
    builder.format_indent(Some(2));
    if let Ok(precision) = env::var("MIDENC_TRACE_TIMING") {
        match precision.as_str() {
            "s" => builder.format_timestamp_secs(),
            "ms" => builder.format_timestamp_millis(),
            "us" => builder.format_timestamp_micros(),
            "ns" => builder.format_timestamp_nanos(),
            other => {
                return Err(Report::msg(format!(
                    "invalid MIDENC_TRACE_TIMING precision, expected one of [s, ms, us, ns], got \
                     '{other}'"
                )));
            }
        };
    } else {
        builder.format_timestamp(None);
    }
    let logger = Box::new(builder.build());
    let filter = logger.filter();

    // Get current working directory
    let cwd = env::current_dir()
        .into_diagnostic()
        .wrap_err("could not read current working directory")?;

    match driver::run(cwd, env::args_os(), logger, filter) {
        Err(report) => match report.downcast::<ClapDiagnostic>() {
            Ok(err) => {
                // Remove the miette panic hook, so that clap errors can be reported without
                // the diagnostic-style formatting
                //drop(std::panic::take_hook());
                err.exit()
            }
            Err(report) => Err(report),
        },
        result => result,
    }
}
