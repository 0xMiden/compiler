use midenc_hir::{Signature, SymbolPath, dialects::builtin::FunctionRef, interner::Symbol};

use crate::intrinsics::Intrinsic;

/// Local core Wasm module functon or processed module import to be used for the translation of the
/// Wasm `call` op.
#[derive(Clone)]
pub enum CallableFunction {
    /// An intrinsic function for which calls will be lowered to a VM instruction
    Instruction {
        /// The recovered intrinsic
        intrinsic: Intrinsic,
        /// Function signature parsed from the core Wasm module
        signature: Signature,
    },
    /// An intrinsic function implemented in Miden Assembly
    Intrinsic {
        /// The recovered intrinsic
        intrinsic: Intrinsic,
        /// Reference to the function declaration or definition
        function_ref: FunctionRef,
        /// Function signature parsed from the core Wasm module
        signature: Signature,
    },
    /// All other functions
    Function {
        /// Module and function name parsed from the core Wasm module
        wasm_id: SymbolPath,
        /// Reference to the function declaration or definition
        function_ref: FunctionRef,
        /// Function signature parsed from the core Wasm module
        signature: Signature,
    },
}

impl CallableFunction {
    pub fn is_intrinsic(&self) -> bool {
        matches!(self, Self::Intrinsic { .. })
    }

    pub fn function_ref(&self) -> Option<FunctionRef> {
        match self {
            Self::Instruction { .. } => None,
            Self::Intrinsic { function_ref, .. } | Self::Function { function_ref, .. } => {
                Some(*function_ref)
            }
        }
    }

    pub fn function_name(&self) -> Symbol {
        match self {
            Self::Instruction { intrinsic, .. } | Self::Intrinsic { intrinsic, .. } => {
                intrinsic.function_name()
            }
            Self::Function { wasm_id, .. } => wasm_id.name(),
        }
    }

    pub fn signature(&self) -> &Signature {
        match self {
            Self::Instruction { signature, .. }
            | Self::Intrinsic { signature, .. }
            | Self::Function { signature, .. } => signature,
        }
    }

    pub fn symbol_path(&self) -> SymbolPath {
        match self {
            Self::Instruction { intrinsic, .. } | Self::Intrinsic { intrinsic, .. } => {
                intrinsic.into_symbol_path()
            }
            Self::Function { wasm_id, .. } => wasm_id.clone(),
        }
    }
}
