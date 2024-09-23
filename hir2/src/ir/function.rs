use core::fmt;

use super::*;
use crate::{
    derive,
    dialects::hir::HirDialect,
    formatter,
    traits::{CallableOpInterface, SingleRegion},
    CallConv, Symbol, SymbolName, SymbolUse, SymbolUseIter, SymbolUseList, Visibility,
};

trait UsableSymbol = Usable<Use = SymbolUse>;

derive! {
    pub struct Function: Op {
        #[dialect]
        dialect: HirDialect,
        #[region]
        body: RegionRef,
        #[attr]
        name: Ident,
        #[attr]
        signature: Signature,
        /// The uses of this function as a symbol
        uses: SymbolUseList,
    }

    derives SingleRegion;
    implements UsableSymbol, Symbol, CallableOpInterface;
}

impl Usable for Function {
    type Use = SymbolUse;

    #[inline(always)]
    fn uses(&self) -> &EntityList<Self::Use> {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut EntityList<Self::Use> {
        &mut self.uses
    }
}

impl Symbol for Function {
    #[inline(always)]
    fn as_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        Self::name(self).as_symbol()
    }

    /// Set the name of this symbol
    fn set_name(&mut self, name: SymbolName) {
        let mut id = *self.name();
        id.name = name;
        Function::set_name(self, id)
    }

    /// Get the visibility of this symbol
    fn visibility(&self) -> Visibility {
        self.signature().visibility
    }

    /// Returns true if this symbol has private visibility
    #[inline]
    fn is_private(&self) -> bool {
        self.signature().is_private()
    }

    /// Returns true if this symbol has public visibility
    #[inline]
    fn is_public(&self) -> bool {
        self.signature().is_public()
    }

    /// Sets the visibility of this symbol
    fn set_visibility(&mut self, visibility: Visibility) {
        self.signature_mut().visibility = visibility;
    }

    /// Get all of the uses of this symbol that are nested within `from`
    fn symbol_uses(&self, from: OperationRef) -> SymbolUseIter {
        todo!()
    }

    /// Return true if there are no uses of this symbol nested within `from`
    fn symbol_uses_known_empty(&self, from: OperationRef) -> SymbolUseIter {
        todo!()
    }

    /// Attempt to replace all uses of this symbol nested within `from`, with the provided replacement
    fn replace_all_uses(&self, replacement: SymbolRef, from: OperationRef) -> Result<(), Report> {
        todo!()
    }

    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    #[inline]
    fn is_declaration(&self) -> bool {
        self.body().is_empty()
    }
}

impl CallableOpInterface for Function {
    fn get_callable_region(&self) -> Option<RegionRef> {
        if self.is_declaration() {
            None
        } else {
            self.regions().front().as_pointer()
        }
    }

    #[inline]
    fn signature(&self) -> &Signature {
        Function::signature(self)
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

impl fmt::Display for AbiParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_map();
        builder.entry(&"ty", &format_args!("{}", &self.ty));
        if !matches!(self.purpose, ArgumentPurpose::Default) {
            builder.entry(&"purpose", &format_args!("{}", &self.purpose));
        }
        if !matches!(self.extension, ArgumentExtension::None) {
            builder.entry(&"extension", &format_args!("{}", &self.extension));
        }
        builder.finish()
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
    /// The linkage/visibility that should be used for this function
    pub visibility: Visibility,
}

crate::define_attr_type!(Signature);

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .key(&"params")
            .value_with(|f| {
                let mut builder = f.debug_list();
                for param in self.params.iter() {
                    builder.entry(&format_args!("{param}"));
                }
                builder.finish()
            })
            .key(&"results")
            .value_with(|f| {
                let mut builder = f.debug_list();
                for param in self.params.iter() {
                    builder.entry(&format_args!("{param}"));
                }
                builder.finish()
            })
            .entry(&"cc", &format_args!("{}", &self.cc))
            .entry(&"visibility", &format_args!("{}", &self.visibility))
            .finish()
    }
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
            visibility: Visibility::Public,
        }
    }

    /// Returns true if this function is externally visible
    pub fn is_public(&self) -> bool {
        matches!(self.visibility, Visibility::Public)
    }

    /// Returns true if this function is only visible within it's containing module
    pub fn is_private(&self) -> bool {
        matches!(self.visibility, Visibility::Public)
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
        self.visibility == other.visibility
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
