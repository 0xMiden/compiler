use core::fmt;

use crate::{
    formatter, CallConv, EntityRef, OpOperandRange, OpOperandRangeMut, RegionRef, Symbol,
    SymbolNameAttr, SymbolRef, Type, UnsafeIntrusiveEntityRef, Value, ValueRef, Visibility,
};

/// A call-like operation is one that transfers control from one function to another.
///
/// These operations may be traditional static calls, e.g. `call @foo`, or indirect calls, e.g.
/// `call_indirect v1`. An operation that uses this interface cannot _also_ implement the
/// `CallableOpInterface`.
pub trait CallOpInterface {
    /// Get the callee of this operation.
    ///
    /// A callee is either a symbol, or a reference to an SSA value.
    fn callable_for_callee(&self) -> Callable;
    /// Sets the callee for this operation.
    fn set_callee(&mut self, callable: Callable);
    /// Get the operands of this operation that are used as arguments for the callee
    fn arguments(&self) -> OpOperandRange<'_>;
    /// Get a mutable reference to the operands of this operation that are used as arguments for the
    /// callee
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_>;
    /// Resolve the callable operation for the current callee to a `CallableOpInterface`, or `None`
    /// if a valid callable was not resolved, using the provided symbol table.
    ///
    /// This method is used to perform callee resolution using a cached symbol table, rather than
    /// traversing the operation hierarchy looking for symbol tables to try resolving with.
    fn resolve_in_symbol_table(&self, symbols: &dyn crate::SymbolTable) -> Option<SymbolRef>;
    /// Resolve the callable operation for the current callee to a `CallableOpInterface`, or `None`
    /// if a valid callable was not resolved.
    fn resolve(&self) -> Option<SymbolRef>;
}

/// A callable operation is one who represents a potential function, and may be a target for a call-
/// like operation (i.e. implementations of `CallOpInterface`). These operations may be traditional
/// function ops (i.e. `Function`), as well as function reference-producing operations, such as an
/// op that creates closures, or captures a function by reference.
///
/// These operations may only contain a single region.
pub trait CallableOpInterface {
    /// Returns the region on the current operation that is callable.
    ///
    /// This may return `None` in the case of an external callable object, e.g. an externally-
    /// defined function reference.
    fn get_callable_region(&self) -> Option<RegionRef>;
    /// Returns the signature of the callable
    fn signature(&self) -> &Signature;
}

#[doc(hidden)]
pub trait AsCallableSymbolRef {
    fn as_callable_symbol_ref(&self) -> SymbolRef;
}
impl<T: Symbol + CallableOpInterface> AsCallableSymbolRef for T {
    #[inline(always)]
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        unsafe { SymbolRef::from_raw(self as &dyn Symbol) }
    }
}
impl<T: Symbol + CallableOpInterface> AsCallableSymbolRef for UnsafeIntrusiveEntityRef<T> {
    #[inline(always)]
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        let t_ptr = Self::as_ptr(self);
        unsafe { SymbolRef::from_raw(t_ptr as *const dyn Symbol) }
    }
}

/// A [Callable] represents a symbol or a value which can be used as a valid _callee_ for a
/// [CallOpInterface] implementation.
///
/// Symbols are not SSA values, but there are situations where we want to treat them as one, such
/// as indirect calls. Abstracting over whether the callable is a symbol or an SSA value allows us
/// to focus on the call semantics, rather than the difference between the type types of value.
#[derive(Debug, Clone)]
pub enum Callable {
    Symbol(SymbolNameAttr),
    Value(ValueRef),
}
impl From<&SymbolNameAttr> for Callable {
    fn from(value: &SymbolNameAttr) -> Self {
        Self::Symbol(value.clone())
    }
}
impl From<SymbolNameAttr> for Callable {
    fn from(value: SymbolNameAttr) -> Self {
        Self::Symbol(value)
    }
}
impl From<ValueRef> for Callable {
    fn from(value: ValueRef) -> Self {
        Self::Value(value)
    }
}
impl Callable {
    #[inline(always)]
    pub fn new(callable: impl Into<Self>) -> Self {
        callable.into()
    }

    pub fn is_symbol(&self) -> bool {
        matches!(self, Self::Symbol(_))
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    pub fn as_symbol_name(&self) -> Option<&SymbolNameAttr> {
        match self {
            Self::Symbol(ref name) => Some(name),
            _ => None,
        }
    }

    pub fn as_value(&self) -> Option<EntityRef<'_, dyn Value>> {
        match self {
            Self::Value(ref value_ref) => Some(value_ref.borrow()),
            _ => None,
        }
    }

    pub fn unwrap_symbol_name(self) -> SymbolNameAttr {
        match self {
            Self::Symbol(name) => name,
            Self::Value(value_ref) => panic!("expected symbol, got {}", value_ref.borrow().id()),
        }
    }

    pub fn unwrap_value_ref(self) -> ValueRef {
        match self {
            Self::Value(value) => value,
            Self::Symbol(ref name) => panic!("expected value, got {name}"),
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
