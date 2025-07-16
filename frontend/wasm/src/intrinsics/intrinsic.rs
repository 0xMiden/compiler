use midenc_hir::{
    diagnostics::{miette, Diagnostic},
    interner::{symbols, Symbol},
    FunctionType, SymbolNameComponent, SymbolPath, Type,
};

use super::{crypto, debug, felt, mem};

/// Error raised when an attempt is made to use or load an unrecognized intrinsic
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("unrecognized intrinsic: '{0}'")]
#[diagnostic()]
pub struct UnknownIntrinsicError(SymbolPath);

/// An intrinsic function, of a known kind.
///
/// This is used instead of [SymbolPath] as it encodes information known/validated about the
/// intrinsic up to the point it was encoded in this type.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Intrinsic {
    /// A debugging intrinsic
    Debug(Symbol),
    /// A memory intrinsic
    Mem(Symbol),
    /// A field element intrinsic
    Felt(Symbol),
    /// A cryptographic intrinsic
    Crypto(Symbol),
}

/// Attempt to recognize an intrinsic function from the given [SymbolPath].
///
/// The path must be a valid absolute path to a function in a known intrinsic module
///
/// NOTE: This does not validate that the intrinsic function is known.
impl TryFrom<&SymbolPath> for Intrinsic {
    type Error = UnknownIntrinsicError;

    fn try_from(path: &SymbolPath) -> Result<Self, Self::Error> {
        let mut components = path.components().peekable();

        // Ignore the root component if present
        components.next_if_eq(&SymbolNameComponent::Root);

        // Must be in the 'intrinsics' namespace
        components
            .next_if_eq(&SymbolNameComponent::Component(symbols::Intrinsics))
            .ok_or_else(|| UnknownIntrinsicError(path.clone()))?;

        // Must be a known 'intrinsics' module (handled last)
        let kind = components
            .next()
            .map(|c| c.as_symbol_name())
            .ok_or_else(|| UnknownIntrinsicError(path.clone()))?;

        // The last component, if present, must be a leaf, i.e. function name
        let function = components
            .next_if(|c| c.is_leaf())
            .map(|c| c.as_symbol_name())
            .ok_or_else(|| UnknownIntrinsicError(path.clone()))?;

        match kind {
            symbols::Debug => Ok(Self::Debug(function)),
            symbols::Mem => Ok(Self::Mem(function)),
            symbols::Felt => Ok(Self::Felt(function)),
            symbols::Crypto => Ok(Self::Crypto(function)),
            _ => Err(UnknownIntrinsicError(path.clone())),
        }
    }
}

impl Intrinsic {
    /// Get a [SymbolPath] corresponding to this intrinsic
    pub fn into_symbol_path(self) -> SymbolPath {
        let mut path = self.module_path();
        path.set_name(self.function_name());
        path
    }

    /// Get a [Symbol] corresponding to the module in the `intrinsics` namespace where this
    /// intrinsic is defined.
    pub fn module_name(&self) -> Symbol {
        match self {
            Self::Debug(_) => symbols::Debug,
            Self::Mem(_) => symbols::Mem,
            Self::Felt(_) => symbols::Felt,
            Self::Crypto(_) => symbols::Crypto,
        }
    }

    /// Get a [SymbolPath] corresponding to the module containing this intrinsic
    pub fn module_path(&self) -> SymbolPath {
        match self {
            Self::Debug(_) => SymbolPath::from_iter(debug::MODULE_PREFIX.iter().copied()),
            Self::Mem(_) => SymbolPath::from_iter(mem::MODULE_PREFIX.iter().copied()),
            Self::Felt(_) => SymbolPath::from_iter(felt::MODULE_PREFIX.iter().copied()),
            Self::Crypto(_) => SymbolPath::from_iter(crypto::MODULE_PREFIX.iter().copied()),
        }
    }

    /// Get the name of the intrinsic function as a [Symbol]
    pub fn function_name(&self) -> Symbol {
        match self {
            Self::Debug(function)
            | Self::Mem(function)
            | Self::Felt(function)
            | Self::Crypto(function) => *function,
        }
    }

    /// Get the [FunctionType] of this intrinsic, if it is implemented as a function.
    ///
    /// Returns `None` for intrinsics which are unknown, or correspond to native instructions.
    pub fn function_type(&self) -> Option<FunctionType> {
        match self {
            Self::Mem(function) => mem::function_type(*function),
            // All debugging intrinsics are currently implemented as native instructions
            Self::Debug(_) => None,
            // All field element intrinsics are currently implemented as native instructions
            Self::Felt(_) => None,
            // Crypto intrinsics are converted to function calls
            Self::Crypto(function) => {
                match function.as_str() {
                    "hmerge" => {
                        // The WASM import signature: takes 2 i32 pointers (digests array pointer + result pointer)
                        let sig = midenc_hir::FunctionType::new(
                            midenc_hir::CallConv::Wasm,
                            vec![
                                // Pointer to array of two digests
                                Type::I32,
                                // Result pointer
                                Type::I32,
                            ],
                            vec![], // No returns - writes to the result pointer
                        );
                        Some(sig)
                    }
                    _ => None,
                }
            }
        }
    }

    /// Get the [IntrinsicsConversionResult] representing how this intrinsic will be lowered.
    ///
    /// Returns `None` for intrinsics which are unknown.
    pub fn conversion_result(&self) -> Option<IntrinsicsConversionResult> {
        match self {
            Self::Mem(function) => {
                mem::function_type(*function).map(IntrinsicsConversionResult::FunctionType)
            }
            Self::Debug(_) | Self::Felt(_) => Some(IntrinsicsConversionResult::MidenVmOp),
            // Crypto intrinsics are converted to function calls
            Self::Crypto(_function) => self
                .function_type()
                .map(IntrinsicsConversionResult::FunctionType)
                .or(Some(IntrinsicsConversionResult::MidenVmOp)),
        }
    }
}

/// Represents how an intrinsic will be converted to IR
pub enum IntrinsicsConversionResult {
    /// As a function
    FunctionType(FunctionType),
    /// As a native instruction
    MidenVmOp,
}

impl IntrinsicsConversionResult {
    pub fn is_function(&self) -> bool {
        matches!(self, IntrinsicsConversionResult::FunctionType(_))
    }

    pub fn is_operation(&self) -> bool {
        matches!(self, IntrinsicsConversionResult::MidenVmOp)
    }
}
