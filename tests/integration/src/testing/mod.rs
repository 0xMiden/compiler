//! This module provides core utilities for constructing tests outside of the primary
//! [crate::CompilerTest] infrastructure.
mod eval;
mod initializer;
pub mod setup;

use std::sync::Arc;

use miden_core::Felt;
use miden_debug::Executor;
use miden_mast_package::Package;
use miden_protocol::ProtocolLib;
use miden_standards::StandardsLib;
use midenc_session::STDLIB;

pub use self::{
    eval::{compile_link_output_to_package, compile_test_module, eval_link_output, eval_package},
    initializer::Initializer,
};

/// Creates an executor with standard library and base library loaded.
///
/// If a package is provided, its dependencies will also be added to the executor.
pub fn executor_with_std(args: Vec<Felt>, package: Option<&Package>) -> Executor {
    let mut exec = Executor::new(args);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut()
        .add(*std_library.digest(), std_library.clone().into());
    let protocol_library = Arc::new(ProtocolLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*protocol_library.digest(), protocol_library.clone().into());
    let standards_library = Arc::new(StandardsLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*standards_library.digest(), standards_library.clone().into());
    if let Some(pkg) = package {
        exec.with_dependencies(pkg.manifest.dependencies())
            .expect("Failed to set up dependencies");
    }
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

    let mut str = PrintDiagnostic::new(report).to_string();
    writeln!(&mut str, "{labels_str}").unwrap();

    str
}
