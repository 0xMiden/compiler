use midenc_session::diagnostics::{Diagnostic, Report, miette};
use thiserror::Error;

/// A WebAssembly translation error.
///
/// When a WebAssembly function can't be translated, one of these error codes will be returned
/// to describe the failure.
#[allow(missing_docs)]
#[derive(Error, Debug, Diagnostic)]
pub enum WasmError {
    /// The input WebAssembly code is invalid.
    ///
    /// This error code is used by a WebAssembly translator when it encounters invalid WebAssembly
    /// code. This should never happen for validated WebAssembly code.
    #[error("invalid input WebAssembly code at offset {offset}: {message}")]
    #[diagnostic()]
    InvalidWebAssembly {
        /// A string describing the validation error.
        message: String,
        /// The bytecode offset where the error occurred.
        offset: usize,
    },

    /// A feature used by the WebAssembly code is not supported by the Miden IR.
    #[error("unsupported WebAssembly code: {0}")]
    #[diagnostic()]
    Unsupported(String),

    /// Too many functions were declared in a module
    #[error("Too many declared functions in the module")]
    #[diagnostic()]
    FuncNumLimitExceeded,

    #[error("import metadata is missing: {0}")]
    #[diagnostic()]
    MissingImportMetadata(String),

    #[error("export metadata is missing: {0}")]
    #[diagnostic()]
    MissingExportMetadata(String),

    #[error(transparent)]
    DwarfError(#[from] gimli::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<wasmparser::BinaryReaderError> for WasmError {
    fn from(e: wasmparser::BinaryReaderError) -> Self {
        Self::InvalidWebAssembly {
            message: e.message().into(),
            offset: e.offset(),
        }
    }
}

/// A convenient alias for a `Result` that uses `WasmError` as the error type.
pub type WasmResult<T> = Result<T, Report>;

/// Emit diagnostics and return an `Err(WasmError::Unsupported(msg))` where `msg` the string built
/// by calling `format!` on the arguments to this macro.
#[macro_export]
macro_rules! unsupported_diag {
    ($diagnostics:expr, $($arg:tt)*) => {{
        return Err($diagnostics
            .diagnostic(midenc_session::diagnostics::Severity::Error)
            .with_message(format!($($arg)*))
            .into_report());
    }}
}
