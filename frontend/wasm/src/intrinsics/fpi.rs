//! The raw foreign-procedure-invocation entry point.
//!
//! `miden::protocol::tx::execute_foreign_procedure_indirect` is not an export of any package: the
//! compiler lowers it to `hir.exec_fpi` itself. It keeps the name the SDK binds it under, so it is
//! recognised by full path before any package is consulted.

use midenc_hir::{CallConv, FunctionType, SymbolPath, Type};

/// The stub name the SDK uses for the raw FPI executor.
pub const FPI_INDIRECT_PATH: &str = "::miden::protocol::tx::execute_foreign_procedure_indirect";

/// Whether `path` names the raw FPI executor.
pub fn is_fpi_indirect(path: &SymbolPath) -> bool {
    path.to_library_path().to_string() == FPI_INDIRECT_PATH
        || alloc::format!("::{}", path.to_library_path()) == FPI_INDIRECT_PATH
}

/// The executor's import signature: one invocation pointer in, sixteen felts out.
pub fn signature() -> FunctionType {
    FunctionType::new(CallConv::Wasm, [Type::I32], alloc::vec![Type::Felt; 16])
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::*;

    #[test]
    fn the_fpi_indirect_procedure_is_recognised_by_its_full_path_only() {
        let id = midenc_hir::FunctionIdent::from_str(
            "miden::protocol::tx::execute_foreign_procedure_indirect",
        )
        .unwrap();
        assert!(is_fpi_indirect(&SymbolPath::from_masm_function_id(id)));
        let other = midenc_hir::FunctionIdent::from_str("miden::protocol::tx::get_block_timestamp")
            .unwrap();
        assert!(!is_fpi_indirect(&SymbolPath::from_masm_function_id(other)));
        let sig = signature();
        assert_eq!(sig.params(), &[midenc_hir::Type::I32]);
        assert_eq!(sig.results().len(), 16);
    }
}
