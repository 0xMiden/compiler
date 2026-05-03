use miden_assembly_syntax::diagnostics::Report;

/// Result type used by the MASM disassembler.
pub type Result<T> = core::result::Result<T, Report>;

pub(crate) fn error(message: impl Into<String>) -> Report {
    Report::msg(message.into())
}
