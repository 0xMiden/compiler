use core::fmt;

use crate::{
    EntityRef, Op, OpOperandRange, OpOperandRangeMut, RegionRef, Symbol, SymbolPath, SymbolRef,
    UnsafeIntrusiveEntityRef, Value, ValueRef,
    dialects::builtin::attributes::{Signature, SymbolRefAttr},
};

/// A call-like operation is one that transfers control from one function to another.
///
/// These operations may be traditional static calls, e.g. `call @foo`, or indirect calls, e.g.
/// `call_indirect v1`. An operation that uses this interface cannot _also_ implement the
/// `CallableOpInterface`.
pub trait CallOpInterface: Op {
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
pub trait CallableOpInterface: Op {
    /// Returns the region on the current operation that is callable.
    ///
    /// This may return `None` in the case of an external callable object, e.g. an externally-
    /// defined function reference.
    fn get_callable_region(&self) -> Option<RegionRef>;
    /// Returns the signature of the callable
    fn signature(&self) -> Signature;
}

/// A marker trait for all operations which are callable symbols
pub trait CallableSymbol: Symbol + CallableOpInterface {}

impl<T: Symbol + CallableOpInterface> CallableSymbol for T {}

/// An alias for [`UnsafeIntrusiveEntityRef<dyn CallableSymbol>`]
pub type CallableSymbolRef = UnsafeIntrusiveEntityRef<dyn CallableSymbol>;

#[doc(hidden)]
pub trait AsCallableSymbolRef {
    fn as_callable_symbol_ref(&self) -> SymbolRef;
}
impl AsCallableSymbolRef for CallableSymbolRef {
    #[inline]
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        *self as SymbolRef
    }
}
impl<T: CallableSymbol> AsCallableSymbolRef for T {
    #[inline(always)]
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        // SAFETY: This is safe under the assumption that all Op implementations are allocated
        // via the arena
        unsafe { SymbolRef::from_raw(self as &dyn Symbol) }
    }
}
impl<T: CallableSymbol> AsCallableSymbolRef for UnsafeIntrusiveEntityRef<T> {
    #[inline(always)]
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        *self as SymbolRef
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
    Symbol(SymbolPath),
    Value(ValueRef),
}
impl fmt::Display for Callable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Symbol(path) => fmt::Display::fmt(path, f),
            Self::Value(value) => fmt::Display::fmt(value, f),
        }
    }
}
impl From<&SymbolRefAttr> for Callable {
    fn from(value: &SymbolRefAttr) -> Self {
        Self::Symbol(value.path().clone())
    }
}
impl From<&SymbolPath> for Callable {
    fn from(value: &SymbolPath) -> Self {
        Self::Symbol(value.clone())
    }
}
impl From<SymbolPath> for Callable {
    fn from(value: SymbolPath) -> Self {
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

    pub fn as_symbol_path(&self) -> Option<&SymbolPath> {
        match self {
            Self::Symbol(name) => Some(name),
            _ => None,
        }
    }

    pub fn as_value(&self) -> Option<EntityRef<'_, dyn Value>> {
        match self {
            Self::Value(value_ref) => Some(value_ref.borrow()),
            _ => None,
        }
    }

    pub fn unwrap_symbol_path(self) -> SymbolPath {
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
