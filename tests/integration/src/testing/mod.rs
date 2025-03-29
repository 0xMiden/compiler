//! This module provides core utilities for constructing tests outside of the primary
//! [crate::CompilerTest] infrastructure.
mod eval;
mod initializer;
pub mod setup;

pub use self::{
    eval::{eval_link_output, eval_package},
    initializer::Initializer,
};

/// Pretty-print `report` to a String
pub fn format_report(report: miden_assembly::diagnostics::Report) -> String {
    use core::fmt::Write;

    use miden_assembly::diagnostics::reporting::PrintDiagnostic;

    let mut labels_str = String::new();
    if let Some(labels) = report.labels() {
        for label in labels {
            if let Some(label) = label.label() {
                writeln!(&mut labels_str, "{}", label).unwrap();
            }
        }
    }

    let mut str = PrintDiagnostic::new(report).to_string();
    writeln!(&mut str, "{labels_str}").unwrap();

    str
}
