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

/// A reference to a symbol implementing [CallableOpInterface], with all function aliases resolved.
///
/// Equality and hashing identify the target operation (the provider of the body and signature)
/// rather than the name used to reach it.
///
/// This is a snapshot of resolution and it must be re-acquired if function aliases are retargeted
/// or symbol tables are modified. Signatures and regions are retrieved from the target operation
/// upon request.
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

/// A resolved callee that preserves both the original referencing name and the canonical target.
///
/// Use [Self::named_symbol] for operations concerning symbol visibility, emission, or usage.
/// use [Self::target] for analyses and execution.
///
/// Like [CanonicalCallableRef], this is a transient resolution snapshot and may become stale if
/// function aliases are retargeted or symbol tables are modified.
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

#[cfg(test)]
mod tests {

    use alloc::{format, string::ToString};

    use super::*;
    use crate::{
        AsCallableSymbolRef, Op, SymbolNameComponent, SymbolTable, Type, Visibility,
        diagnostics::Uri,
        dialects::builtin::{
            Module, ModuleBuilder, ModuleRef,
            attributes::{SymbolRef as SymbolRefValue, SymbolRefAttr},
        },
        parse::{ParserConfig, parse},
        testing::Test,
    };

    fn module(source: &str) -> (Test, ModuleRef) {
        let test = Test::default();
        let module = parse::<Module>(
            ParserConfig::new(test.context_rc()),
            Uri::new("resolution.hir"),
            source,
        )
        .unwrap();
        (test, module)
    }

    const ALIASED: &str = r#"
builtin.module public @test {
    builtin.function private extern("C") @body() { builtin.ret; };
    builtin.function_alias private @first -> @body;
    builtin.function_alias public @api -> @first;
};
"#;

    #[test]
    fn resolved_alias_retains_name_and_reads_current_target_signature() {
        let (test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let alias = mb.resolve_callable("api").unwrap();
        let direct = mb.resolve_callable("body").unwrap();
        assert_eq!(alias.named_symbol().borrow().name(), SymbolName::intern("api"));
        assert_eq!(alias.named_symbol().borrow().visibility(), Visibility::Public);
        assert_eq!(alias.target(), direct.target());
        assert_eq!(
            alias.as_callable_symbol_ref(),
            module.borrow().get(SymbolName::intern("api")).unwrap()
        );
        assert!(alias.target().callable_region().is_some());
        let mut function = alias.target().as_function().unwrap();
        assert_eq!(function.borrow().visibility(), Visibility::Private);
        let updated = Signature::new(&test.context_rc(), [Type::U32], [Type::U64]);
        *function.borrow_mut().get_signature_mut() = updated.clone();
        assert_eq!(alias.signature(), updated);
        assert_eq!(alias.target().borrow().signature(), updated);
    }

    #[test]
    fn canonical_alias_resolution_and_verification_agree_for_long_chains() {
        for length in [0, 1, 128] {
            let mut source = "builtin.module public @test {\n".to_string();
            for i in 0..length {
                source.push_str(&format!("builtin.function_alias public @a{i} -> @a{};\n", i + 1));
            }
            source.push_str(&format!(
                "builtin.function private extern(\"C\") @a{length}() {{ builtin.ret; }};\n}};"
            ));
            let (_test, module) = module(&source);
            let mb = ModuleBuilder::new(module);
            let callee = mb.resolve_callable("a0").unwrap();
            assert!(callee.target().as_function() == mb.get_function(&format!("a{length}")));
            module.borrow().as_operation().recursively_verify().unwrap();
        }
    }

    #[test]
    fn alias_resolution_reports_direct_self_reference_cycles() {
        let (_test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let mut first = mb.get_function_alias("first").unwrap();
        // Builders reject self-uses. Emulate malformed IR to exercise resolution.
        let path = first.borrow().path();
        first.borrow_mut().target_mut().set_path(path);
        assert!(matches!(mb.resolve_callable("api"), Err(SymbolResolutionError::Cycle { .. })));
        let error = module.borrow().as_operation().recursively_verify().unwrap_err();
        assert!(format!("{error}").contains("cycle"));
    }

    #[test]
    fn alias_resolution_reports_mutual_alias_cycles() {
        let (_test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let api = mb.get_function_alias("api").unwrap();
        let mut first = mb.get_function_alias("first").unwrap();
        first.borrow_mut().set_target(api).unwrap();
        assert!(matches!(mb.resolve_callable("api"), Err(SymbolResolutionError::Cycle { .. })));
        let error = module.borrow().as_operation().recursively_verify().unwrap_err();
        assert!(format!("{error}").contains("cycle"));
    }

    #[test]
    fn resolving_an_unknown_symbol_name_reports_unknown_symbol() {
        let (_test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        assert!(matches!(
            mb.resolve_callable("missing"),
            Err(SymbolResolutionError::UnknownSymbol { .. })
        ));
    }

    #[test]
    fn alias_chain_with_removed_target_reports_unknown_symbol() {
        let (_test, mut module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        module.borrow_mut().remove(SymbolName::intern("body"));
        assert!(matches!(
            mb.resolve_callable("api"),
            Err(SymbolResolutionError::UnknownSymbol { .. })
        ));
    }

    #[test]
    fn alias_chain_with_non_callable_target_reports_not_callable() {
        let (_test, mut module) = module(ALIASED);
        let mut mb = ModuleBuilder::new(module);
        module.borrow_mut().remove(SymbolName::intern("body"));
        mb.define_global_variable("body".into(), Visibility::Private, Type::U32)
            .unwrap();
        assert!(matches!(
            mb.resolve_callable("api"),
            Err(SymbolResolutionError::NotCallable { .. })
        ));
    }

    #[test]
    fn resolving_a_non_callable_symbol_reports_not_callable() {
        let (_test, module) = module(ALIASED);
        let mut mb = ModuleBuilder::new(module);
        mb.define_global_variable("data".into(), Visibility::Private, Type::U32)
            .unwrap();
        assert!(matches!(
            mb.resolve_callable("data"),
            Err(SymbolResolutionError::NotCallable { .. })
        ));
    }

    #[test]
    fn canonical_resolution_accepts_non_callable_symbols() {
        let (_test, module) = module(ALIASED);
        let mut mb = ModuleBuilder::new(module);
        let global = mb
            .define_global_variable("data".into(), Visibility::Private, Type::U32)
            .unwrap();
        let symbol = global.borrow().as_operation().as_symbol_ref().unwrap();
        assert_eq!(symbol.resolve_canonical().unwrap(), symbol);
    }

    #[test]
    fn detached_alias_reports_no_symbol_table() {
        let (_test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let mut first = mb.get_function_alias("first").unwrap();
        let symbol = first.borrow().as_operation().as_symbol_ref().unwrap();
        first.borrow_mut().as_operation_mut().remove();
        assert_eq!(symbol.resolve_callable().unwrap_err(), SymbolResolutionError::NoSymbolTable);
    }

    #[test]
    fn resolving_a_detached_alias_through_its_user_reports_unknown_symbol() {
        let (_test, mut module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let api = mb.get_function_alias("api").unwrap();
        let mut first = mb.get_function_alias("first").unwrap();
        module.borrow_mut().remove(SymbolName::intern("first"));
        first.borrow_mut().as_operation_mut().remove();
        assert!(matches!(
            api.borrow().target().resolve_callable(),
            Err(SymbolResolutionError::UnknownSymbol { .. })
        ));
    }

    #[test]
    fn untracked_symbol_attribute_returns_a_structured_error() {
        let test = Test::default();
        let path = SymbolPath::new([
            SymbolNameComponent::Root,
            SymbolNameComponent::Leaf(SymbolName::intern("untracked")),
        ])
        .unwrap();
        let attr = test
            .context_rc()
            .create_attribute::<SymbolRefAttr, _>(SymbolRefValue::new(path.clone(), None));
        assert_eq!(
            attr.borrow().resolve_callable(),
            Err(SymbolResolutionError::UntrackedSymbol { path })
        );
    }

    #[test]
    fn resolved_callable_is_a_snapshot_of_the_alias_target() {
        let mut test = Test::default().in_module("test");
        let old_target = test.define_function("old_target", &[], &[]);
        let new_target = test.define_function("new_target", &[], &[]);
        let mut mb = ModuleBuilder::new(test.module());
        let mut alias = mb
            .define_function_alias("entry".into(), Visibility::Public, old_target)
            .unwrap();
        let old = mb.resolve_callable("entry").unwrap();
        alias.borrow_mut().set_target(new_target).unwrap();
        let new = mb.resolve_callable("entry").unwrap();
        assert!(old.named_symbol() == new.named_symbol());
        assert!(old.target().as_function() == Some(old_target));
        assert!(new.target().as_function() == Some(new_target));
    }

    #[test]
    fn callable_declaration_has_signature_but_no_body() {
        let mut test = Test::default().in_module("test");
        let decl = test.define_function("decl", &[Type::U32], &[Type::U32]);
        let mut module = ModuleBuilder::new(test.module());
        module.define_function_alias("entry".into(), Visibility::Public, decl).unwrap();
        let callee = module.resolve_callable("entry").unwrap();
        assert!(callee.target().callable_region().is_none());
        assert!(callee.target().as_function().unwrap().borrow().is_declaration());
        assert_eq!(callee.signature().arity(), 1);
    }
}
