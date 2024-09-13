use super::{Operation, Symbol};
use crate::Spanned;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionIdent {
    module: midenc_hir_symbol::Symbol,
    function: midenc_hir_symbol::Symbol,
}

#[derive(Spanned)]
pub struct Function {
    #[span]
    op: Operation,
    id: FunctionIdent,
    signature: Signature,
}
impl Symbol for Function {
    type Id = midenc_hir_symbol::Symbol;

    fn id(&self) -> Self::Id {
        self.id.function
    }
}

struct Function

/// Represents the calling convention of a function.
///
/// Calling conventions are part of a program's ABI (Application Binary Interface), and
/// they define things such how arguments are passed to a function, how results are returned,
/// etc. In essence, the contract between caller and callee is described by the calling convention
/// of a function.
///
/// Importantly, it is perfectly normal to mix calling conventions. For example, the public
/// API for a C library will use whatever calling convention is used by C on the target
/// platform (for Miden, that would be `SystemV`). However, internally that library may use
/// the `Fast` calling convention to allow the compiler to optimize more effectively calls
/// from the public API to private functions. In short, choose a calling convention that is
/// well-suited for a given function, to the extent that other constraints don't impose a choice
/// on you.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum CallConv {
    /// This calling convention is what I like to call "chef's choice" - the
    /// compiler chooses it's own convention that optimizes for call performance.
    ///
    /// As a result of this, it is not permitted to use this convention in externally
    /// linked functions, as the convention is unstable, and the compiler can't ensure
    /// that the caller in another translation unit will use the correct convention.
    Fast,
    /// The standard calling convention used for C on most platforms
    #[default]
    SystemV,
    /// A function which is using the WebAssembly Component Model "Canonical ABI".
    Wasm,
    /// A function with this calling convention must be called using
    /// the `syscall` instruction. Attempts to call it with any other
    /// call instruction will cause a validation error. The one exception
    /// to this rule is when calling another function with the `Kernel`
    /// convention that is defined in the same module, which can use the
    /// standard `call` instruction.
    ///
    /// Kernel functions may only be defined in a kernel [Module].
    ///
    /// In all other respects, this calling convention is the same as `SystemV`
    Kernel,
}
impl fmt::Display for CallConv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Fast => f.write_str("fast"),
            Self::SystemV => f.write_str("C"),
            Self::Wasm => f.write_str("wasm"),
            Self::Kernel => f.write_str("kernel"),
        }
    }
}

/// Represents whether an argument or return value has a special purpose in
/// the calling convention of a function.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum ArgumentPurpose {
    /// No special purpose, the argument is passed/returned by value
    #[default]
    Default,
    /// Used for platforms where the calling convention expects return values of
    /// a certain size to be written to a pointer passed in by the caller.
    StructReturn,
}
impl fmt::Display for ArgumentPurpose {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::StructReturn => f.write_str("sret"),
        }
    }
}

/// Represents how to extend a small integer value to native machine integer width.
///
/// For Miden, native integrals are unsigned 64-bit field elements, but it is typically
/// going to be the case that we are targeting the subset of Miden Assembly where integrals
/// are unsigned 32-bit integers with a standard twos-complement binary representation.
///
/// It is for the latter scenario that argument extension is really relevant.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum ArgumentExtension {
    /// Do not perform any extension, high bits have undefined contents
    #[default]
    None,
    /// Zero-extend the value
    Zext,
    /// Sign-extend the value
    Sext,
}
impl fmt::Display for ArgumentExtension {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Zext => f.write_str("zext"),
            Self::Sext => f.write_str("sext"),
        }
    }
}

/// Describes a function parameter or result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AbiParam {
    /// The type associated with this value
    pub ty: Type,
    /// The special purpose, if any, of this parameter or result
    pub purpose: ArgumentPurpose,
    /// The desired approach to extending the size of this value to
    /// a larger bit width, if applicable.
    pub extension: ArgumentExtension,
}
impl AbiParam {
    pub fn new(ty: Type) -> Self {
        Self {
            ty,
            purpose: ArgumentPurpose::default(),
            extension: ArgumentExtension::default(),
        }
    }

    pub fn sret(ty: Type) -> Self {
        assert!(ty.is_pointer(), "sret parameters must be pointers");
        Self {
            ty,
            purpose: ArgumentPurpose::StructReturn,
            extension: ArgumentExtension::default(),
        }
    }
}
impl formatter::PrettyPrint for AbiParam {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let mut doc = const_text("(") + const_text("param") + const_text(" ");
        if !matches!(self.purpose, ArgumentPurpose::Default) {
            doc += const_text("(") + display(self.purpose) + const_text(")") + const_text(" ");
        }
        if !matches!(self.extension, ArgumentExtension::None) {
            doc += const_text("(") + display(self.extension) + const_text(")") + const_text(" ");
        }
        doc + text(format!("{}", &self.ty)) + const_text(")")
    }
}

/// A [Signature] represents the type, ABI, and linkage of a function.
///
/// A function signature provides us with all of the necessary detail to correctly
/// validate and emit code for a function, whether from the perspective of a caller,
/// or the callee.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Signature {
    /// The arguments expected by this function
    pub params: Vec<AbiParam>,
    /// The results returned by this function
    pub results: Vec<AbiParam>,
    /// The calling convention that applies to this function
    pub cc: CallConv,
    /// The linkage that should be used for this function
    pub linkage: Linkage,
}
impl Signature {
    /// Create a new signature with the given parameter and result types,
    /// for a public function using the `SystemV` calling convention
    pub fn new<P: IntoIterator<Item = AbiParam>, R: IntoIterator<Item = AbiParam>>(
        params: P,
        results: R,
    ) -> Self {
        Self {
            params: params.into_iter().collect(),
            results: results.into_iter().collect(),
            cc: CallConv::SystemV,
            linkage: Linkage::External,
        }
    }

    /// Returns true if this function is externally visible
    pub fn is_public(&self) -> bool {
        matches!(self.linkage, Linkage::External)
    }

    /// Returns true if this function is only visible within it's containing module
    pub fn is_private(&self) -> bool {
        matches!(self.linkage, Linkage::Internal)
    }

    /// Returns true if this function is a kernel function
    pub fn is_kernel(&self) -> bool {
        matches!(self.cc, CallConv::Kernel)
    }

    /// Returns the number of arguments expected by this function
    pub fn arity(&self) -> usize {
        self.params().len()
    }

    /// Returns a slice containing the parameters for this function
    pub fn params(&self) -> &[AbiParam] {
        self.params.as_slice()
    }

    /// Returns the parameter at `index`, if present
    #[inline]
    pub fn param(&self, index: usize) -> Option<&AbiParam> {
        self.params.get(index)
    }

    /// Returns a slice containing the results of this function
    pub fn results(&self) -> &[AbiParam] {
        match self.results.as_slice() {
            [AbiParam { ty: Type::Unit, .. }] => &[],
            [AbiParam {
                ty: Type::Never, ..
            }] => &[],
            results => results,
        }
    }
}
impl Eq for Signature {}
impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.linkage == other.linkage
            && self.cc == other.cc
            && self.params.len() == other.params.len()
            && self.results.len() == other.results.len()
    }
}
impl formatter::PrettyPrint for Signature {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let cc = if matches!(self.cc, CallConv::SystemV) {
            None
        } else {
            Some(
                const_text("(")
                    + const_text("cc")
                    + const_text(" ")
                    + display(self.cc)
                    + const_text(")"),
            )
        };

        let params = self.params.iter().fold(cc.unwrap_or(Document::Empty), |acc, param| {
            if acc.is_empty() {
                param.render()
            } else {
                acc + const_text(" ") + param.render()
            }
        });

        if self.results.is_empty() {
            params
        } else {
            let open = const_text("(") + const_text("result");
            let results = self
                .results
                .iter()
                .fold(open, |acc, e| acc + const_text(" ") + text(format!("{}", &e.ty)))
                + const_text(")");
            if matches!(params, Document::Empty) {
                results
            } else {
                params + const_text(" ") + results
            }
        }
    }
}
