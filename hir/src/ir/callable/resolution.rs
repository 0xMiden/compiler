#[cfg(test)]
mod tests;

use crate::{
    CallableOpInterface, CallableSymbol, CallableSymbolRef, EntityRef, FxHashSet, OperationRef,
    RegionRef, Symbol, SymbolName, SymbolPath, SymbolRef, UnsafeIntrusiveEntityRef,
    dialects::builtin::{Function, FunctionAlias, FunctionRef, attributes::Signature},
};

/// A failure to resolve a named symbol or its canonical callable.
///
/// Errors own their identifying information, so diagnostics do not borrow the IR arena.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SymbolResolutionError {
    #[error("cannot resolve symbol outside a symbol table")]
    NoSymbolTable,
    #[error("symbol '{path}' does not resolve")]
    UnknownSymbol { path: SymbolPath },
    #[error("alias cycle at '{symbol}'")]
    Cycle { symbol: SymbolName },
    #[error("symbol '{symbol}' is not callable")]
    NotCallable { symbol: SymbolName },
    #[error("symbol '{symbol}' is not a builtin.function")]
    NotFunction { symbol: SymbolName },
    #[error("callee is not a symbol")]
    NonSymbolCallee,
    #[error("symbol reference '{path}' has no tracked owner")]
    UntrackedSymbol { path: SymbolPath },
}

/// A symbol known to implement [CallableOpInterface], with all function aliases resolved.
///
/// Equality and hashing describe the body/signature owner, not the name used to reach it.
/// This is a snapshot of resolution, not a cache: resolve again after retargeting aliases or
/// changing symbol tables. Signatures and regions are always read from the current target.
// TODO edit comment
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalCallableRef {
    symbol: SymbolRef,
}

impl CanonicalCallableRef {
    /// The canonical symbol, for symbol-oriented APIs.
    pub fn as_symbol_ref(self) -> SymbolRef {
        self.symbol
    }

    pub fn as_operation_ref(self) -> OperationRef {
        self.symbol.borrow().as_operation_ref()
    }

    /// Borrow the [CallableOpInterface].
    pub fn borrow(&self) -> EntityRef<'_, dyn CallableOpInterface> {
        EntityRef::map(self.symbol.borrow(), |symbol| {
            symbol
                .as_symbol_operation()
                .as_trait::<dyn CallableOpInterface>()
                .expect("canonical callable handles must refer to callable operations")
        })
    }

    pub fn signature(self) -> Signature {
        self.borrow().signature()
    }

    /// The region owned by the callable op, or `None` for a declaration.
    pub fn callable_region(self) -> Option<RegionRef> {
        self.borrow().get_callable_region()
    }

    pub fn as_function(self) -> Option<FunctionRef> {
        self.as_operation_ref().try_downcast_op::<Function>().ok()
    }
}

/// A resolved symbolic callee, retaining both its referenced name and its canonical callable.
///
/// Use [Self::named_symbol] for visibility, emission and symbol uses; use [Self::target] for
/// analyses and execution. Construct through [SymbolRef::resolve_callable]. Like
/// [CanonicalCallableRef], this is a short-lived resolution snapshot, not an IR mutation cache.
// TODO edit doc comment
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct ResolvedSymbolCallee {
    named: CallableSymbolRef,
    target: CanonicalCallableRef,
}

impl core::fmt::Debug for ResolvedSymbolCallee {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResolvedSymbolCallee")
            .field("named", &(self.named as SymbolRef))
            .field("target", &self.target)
            .finish()
    }
}

impl ResolvedSymbolCallee {
    /// The name used to reference the callable.
    ///
    /// In case of an alias the named symbol differs from the symbol of the callable.
    pub fn named_symbol(self) -> CallableSymbolRef {
        self.named
    }

    pub fn target(self) -> CanonicalCallableRef {
        self.target
    }

    pub fn signature(self) -> Signature {
        self.target.signature()
    }
}

impl crate::AsCallableSymbolRef for ResolvedSymbolCallee {
    fn as_callable_symbol_ref(&self) -> SymbolRef {
        // TODO check if this cast is safe
        self.named as SymbolRef
    }
}

impl UnsafeIntrusiveEntityRef<dyn Symbol> {
    /// Follow aliases in each alias's own symbol table, preserving the stored symbol uses.
    ///
    /// Non-alias symbols are returned unchanged. Cycles are detected by identity and acyclic
    /// chains have no depth limit.
    ///
    // TODO consider add depth limit (top level CONST) with variant in `SymbolResolutionError`
    pub fn resolve_canonical(self) -> Result<SymbolRef, SymbolResolutionError> {
        let mut current = self;
        let mut visited = FxHashSet::default();
        loop {
            let symbol = current.borrow();
            let op = symbol.as_symbol_operation();
            let Some(alias) = op.downcast_ref::<FunctionAlias>() else {
                return Ok(current);
            };
            if !visited.insert(current) {
                return Err(SymbolResolutionError::Cycle {
                    symbol: symbol.name(),
                });
            }
            let table = op.nearest_symbol_table().ok_or(SymbolResolutionError::NoSymbolTable)?;
            let path = alias.target().path().clone();
            let next = table
                .borrow()
                .as_symbol_table()
                .ok_or(SymbolResolutionError::NoSymbolTable)?
                .resolve(&path)
                .ok_or(SymbolResolutionError::UnknownSymbol { path })?;
            current = next;
        }
    }

    /// Helper resolving a callable symbol without requiring to distinguish functions and aliases.
    pub fn resolve_callable(self) -> Result<ResolvedSymbolCallee, SymbolResolutionError> {
        let named = self.as_trait_ref::<dyn CallableSymbol>().ok_or_else(|| {
            SymbolResolutionError::NotCallable {
                symbol: self.borrow().name(),
            }
        })?;
        let symbol = self.resolve_canonical()?;
        if !symbol.borrow().as_symbol_operation().implements::<dyn CallableOpInterface>() {
            return Err(SymbolResolutionError::NotCallable {
                symbol: symbol.borrow().name(),
            });
        }
        Ok(ResolvedSymbolCallee {
            named,
            target: CanonicalCallableRef { symbol },
        })
    }

    /// Resolve a callable and require its canonical target to be a builtin function.
    pub fn resolve_function(self) -> Result<FunctionRef, SymbolResolutionError> {
        let target = self.resolve_callable()?.target();
        target.as_function().ok_or_else(|| SymbolResolutionError::NotFunction {
            symbol: target.as_symbol_ref().borrow().name(),
        })
    }
}
