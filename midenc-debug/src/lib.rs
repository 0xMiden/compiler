#![feature(iter_array_chunks)]
#![allow(unused)]

mod cli;
mod config;
mod debug;
mod exec;
mod felt;
mod logger;
mod ui;

use std::rc::Rc;

use midenc_session::{
    diagnostics::{IntoDiagnostic, Report},
    HumanDuration, Session,
};

pub use self::{
    cli::Debugger,
    config::DebuggerConfig,
    debug::*,
    exec::*,
    felt::{bytes_to_words, Felt, Felt as TestFelt, FromMidenRepr, ToMidenRepr},
};

pub type ExecutionResult<T> = Result<T, Report>;

pub fn run(
    inputs: Option<DebuggerConfig>,
    args: Vec<miden_processor::Felt>,
    session: Rc<Session>,
    logger: Box<dyn log::Log>,
) -> ExecutionResult<()> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    let rt = builder.enable_all().build().into_diagnostic()?;
    rt.block_on(async move { start_ui(inputs, args, session, logger).await })
}

pub fn run_noninteractively(
    inputs: Option<DebuggerConfig>,
    args: Vec<miden_processor::Felt>,
    num_outputs: usize,
    session: Rc<Session>,
) -> ExecutionResult<()> {
    use std::time::Instant;

    use midenc_hir::formatter::ToHex;

    println!("===============================================================================");
    println!("Run program: {}", session.inputs[0].file_name());
    println!("-------------------------------------------------------------------------------");

    let state = ui::State::from_inputs(inputs, args, session)?;

    println!(
        "Executed program with hash {} in {}",
        state.package.digest().to_hex(),
        HumanDuration::from(state.execution_duration),
    );

    // write the stack outputs to the screen.
    println!("Output: {:?}", state.execution_trace.outputs().stack_truncated(num_outputs));

    // calculate the percentage of padded rows
    let trace_len_summary = state.execution_trace.trace_len_summary();
    let padding_percentage = (trace_len_summary.padded_trace_len() - trace_len_summary.trace_len())
        * 100
        / trace_len_summary.padded_trace_len();

    // print the required cycles for each component
    println!(
        "VM cycles: {} extended to {} steps ({}% padding).
├── Stack rows: {}
├── Range checker rows: {}
└── Chiplets rows: {}
├── Hash chiplet rows: {}
├── Bitwise chiplet rows: {}
├── Memory chiplet rows: {}
└── Kernel ROM rows: {}",
        trace_len_summary.trace_len(),
        trace_len_summary.padded_trace_len(),
        padding_percentage,
        trace_len_summary.main_trace_len(),
        trace_len_summary.range_trace_len(),
        trace_len_summary.chiplets_trace_len().trace_len(),
        trace_len_summary.chiplets_trace_len().hash_chiplet_len(),
        trace_len_summary.chiplets_trace_len().bitwise_chiplet_len(),
        trace_len_summary.chiplets_trace_len().memory_chiplet_len(),
        trace_len_summary.chiplets_trace_len().kernel_rom_len(),
    );

    Ok(())
}

pub fn trace(
    _options: Option<DebuggerConfig>,
    _args: Vec<String>,
    _session: Rc<Session>,
) -> ExecutionResult<ExecutionTrace> {
    todo!()
}

pub async fn start_ui(
    inputs: Option<DebuggerConfig>,
    args: Vec<miden_processor::Felt>,
    session: Rc<Session>,
    logger: Box<dyn log::Log>,
) -> Result<(), Report> {
    use ratatui::crossterm as term;

    logger::DebugLogger::install(logger);

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = term::terminal::disable_raw_mode();
        let _ = term::execute!(std::io::stdout(), term::terminal::LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let mut app = ui::App::new(inputs, args, session).await?;
    app.run().await?;

    Ok(())
}
