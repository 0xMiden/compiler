use core::fmt;

use super::*;
use crate::{formatter, CallConv, Linkage};

#[derive(Spanned)]
pub struct Function {
    #[span]
    op: Operation,
    id: FunctionIdent,
    signature: Signature,
}
impl Symbol for Function {
    type Id = Ident;

    fn id(&self) -> Self::Id {
        self.id.function
    }
}
impl Function {
    pub fn signature(&self) -> &Signature {
        &self.signature
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
