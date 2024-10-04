use crate::{
    EntityRef, OpOperandRange, OpOperandRangeMut, RegionRef, Signature, Symbol, SymbolNameAttr,
    SymbolRef, UnsafeIntrusiveEntityRef, Value, ValueRef,
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
