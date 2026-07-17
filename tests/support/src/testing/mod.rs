//! This module provides core utilities for constructing tests outside of the primary
//! [crate::CompilerTest] infrastructure.

mod eval;
mod initializer;
pub mod setup;

use std::sync::Arc;

use miden_assembly::serde::Serializable;
use miden_core::Felt;
use miden_debug::Executor;
use miden_mast_package::Package;
use miden_protocol::{ProtocolLib, transaction::TransactionKernel};
use miden_standards::StandardsLib;

pub use self::{
    eval::{
        compile_miden_component_to_package, compile_test_module, compile_test_module_with_masm,
        eval_miden_component, eval_miden_component_with_advice_stack, eval_package,
        eval_package_with_advice_stack, run_masm_vs_rust,
    },
    initializer::Initializer,
};

/// Creates an executor with standard library and base library loaded.
///
/// If a package is provided, its dependencies will also be added to the executor.
pub fn executor_with_std(args: Vec<Felt>) -> Executor {
    let mut exec = Executor::new(args);

    // Register the standard library so dependencies can be resolved at runtime.
    let core_library = miden_core_lib::CoreLibrary::default();
    exec.with_package(core_library.package())
        .expect("failed to register core package");
    // The debug executor path does not automatically install core-library event handlers, but
    // integration tests execute core helpers such as `u64::div` through the VM.
    for (event, handler) in core_library.handlers() {
        if matches!(
            miden_debug::Event::from(event.clone()),
            miden_debug::Event::UserDefined(_) | miden_debug::Event::Unknown(_)
        ) {
            exec.register_event_handler(event, handler)
                .expect("failed to register core library event handler");
        }
    }

    let tx_kernel = TransactionKernel::package();
    let protocol_lib = ProtocolLib::default().package();
    exec.with_package(tx_kernel).expect("failed to register tx-kernel package");
    exec.with_package(protocol_lib).expect("failed to register protocol package");
    exec.with_package(Arc::new(StandardsLib::default().as_ref().clone()))
        .expect("failed to register standards package");

    exec
}

/// Pretty-print `report` to a String
pub fn format_report(report: miden_assembly::diagnostics::Report) -> String {
    use core::fmt::Write;

    use miden_assembly::diagnostics::reporting::PrintDiagnostic;

    let mut labels_str = String::new();
    if let Some(labels) = report.labels() {
        for label in labels {
            if let Some(label) = label.label() {
                writeln!(&mut labels_str, "{label}").unwrap();
            }
        }
    }

    let mut str = PrintDiagnostic::new_without_color(report).to_string();
    writeln!(&mut str, "{labels_str}").unwrap();

    str
}

/// Returns the serialized byte size of the MastForest with stripped debug info
pub fn stripped_mast_size_str(package: &Package) -> String {
    package.mast_forest().to_bytes().len().to_string()
}
