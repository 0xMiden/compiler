// TODO move these tests into `../resolution.rs`, then remove this file here and its directory

use alloc::{format, string::ToString};

use super::*;
use crate::{
    AsCallableSymbolRef, Op, SymbolTable, Type, Visibility,
    diagnostics::Uri,
    dialects::builtin::{Module, ModuleBuilder, ModuleRef},
    parse::{ParserConfig, parse},
    testing::Test,
};

fn module(source: &str) -> (Test, ModuleRef) {
    let test = Test::default();
    let module =
        parse::<Module>(ParserConfig::new(test.context_rc()), Uri::new("resolution.hir"), source)
            .unwrap();
    (test, module)
}

// TODO rename:
// - body -> foo
// - first -> foo_alias
// - api -> foo_alias_alias
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

// TODO split into two tests: 1 for cycle with self, one for cycle without self
#[test]
fn alias_resolution_reports_cycles_including_direct_self_reference() {
    for self_reference in [false, true] {
        let (_test, module) = module(ALIASED);
        let mb = ModuleBuilder::new(module);
        let api = mb.get_function_alias("api").unwrap();
        let mut first = mb.get_function_alias("first").unwrap();
        if self_reference {
            // Builders reject self-uses. Emulate malformed IR to exercise resolution.
            let path = first.borrow().path();
            first.borrow_mut().target_mut().set_path(path);
        } else {
            first.borrow_mut().set_target(api).unwrap();
        }
        assert!(matches!(mb.resolve_callable("api"), Err(SymbolResolutionError::Cycle { .. })));
        let error = module.borrow().as_operation().recursively_verify().unwrap_err();
        assert!(format!("{error}").contains("cycle"));
    }
}

// TODO this test is too overloaded. split into multiple simpler tests
#[test]
fn alias_resolution_errors_distinguish_missing_non_callable_and_detached_targets() {
    let (_test, mut module) = module(ALIASED);
    let mut mb = ModuleBuilder::new(module);
    let api = mb.get_function_alias("api").unwrap();
    let mut first = mb.get_function_alias("first").unwrap();
    assert!(matches!(
        mb.resolve_callable("missing"),
        Err(SymbolResolutionError::UnknownSymbol { .. })
    ));

    module.borrow_mut().remove(SymbolName::intern("body"));
    assert!(matches!(
        mb.resolve_callable("api"),
        Err(SymbolResolutionError::UnknownSymbol { .. })
    ));
    let global = mb
        .define_global_variable("body".into(), Visibility::Private, Type::U32)
        .unwrap();
    assert!(matches!(
        mb.resolve_callable("api"),
        Err(SymbolResolutionError::NotCallable { .. })
    ));
    assert!(matches!(
        mb.resolve_callable("body"),
        Err(SymbolResolutionError::NotCallable { .. })
    ));
    // Generic symbol resolution still accepts non-callable symbols.
    let symbol = global.borrow().as_operation().as_symbol_ref().unwrap();
    assert_eq!(symbol.resolve_canonical().unwrap(), symbol);

    module.borrow_mut().remove(SymbolName::intern("first"));
    let symbol = first.borrow().as_operation().as_symbol_ref().unwrap();
    first.borrow_mut().as_operation_mut().remove();
    assert_eq!(symbol.resolve_callable().unwrap_err(), SymbolResolutionError::NoSymbolTable);
    assert!(matches!(
        api.borrow().target().resolve_callable(),
        Err(SymbolResolutionError::UnknownSymbol { .. })
    ));
}

#[test]
fn untracked_symbol_attribute_returns_a_structured_error() {
    use crate::dialects::builtin::attributes::{SymbolRef as SymbolRefValue, SymbolRefAttr};

    let (test, module) = module(ALIASED);
    let path = module.borrow().get(SymbolName::intern("api")).unwrap().borrow().path();
    let attr = test
        .context_rc()
        .create_attribute::<SymbolRefAttr, _>(SymbolRefValue::new(path, None));
    assert!(matches!(
        attr.borrow().resolve_callable(),
        Err(SymbolResolutionError::UntrackedSymbol { .. })
    ));
}

// TODO use names different from those in ALIASED, otherwise it's confusing
#[test]
fn retargeting_an_alias_requires_resolving_a_new_snapshot() {
    let mut test = Test::default().in_module("test");
    let first = test.define_function("first", &[], &[]);
    let second = test.define_function("second", &[], &[]);
    let mut mb = ModuleBuilder::new(test.module());
    let mut alias = mb.define_function_alias("api".into(), Visibility::Public, first).unwrap();
    let old = mb.resolve_callable("api").unwrap();
    alias.borrow_mut().set_target(second).unwrap();
    let new = mb.resolve_callable("api").unwrap();
    assert!(old.named_symbol() == new.named_symbol());
    assert!(old.target().as_function() == Some(first));
    assert!(new.target().as_function() == Some(second));
}

// TODO use names different from those in ALIASED, otherwise it's confusing
#[test]
fn callable_declaration_has_signature_but_no_body() {
    let mut test = Test::default().in_module("test");
    let external = test.define_function("external", &[Type::U32], &[Type::U32]);
    let mut module = ModuleBuilder::new(test.module());
    module
        .define_function_alias("api".into(), Visibility::Public, external)
        .unwrap();
    let callee = module.resolve_callable("api").unwrap();
    assert!(callee.target().callable_region().is_none());
    assert!(callee.target().as_function().unwrap().borrow().is_declaration());
    assert_eq!(callee.signature().arity(), 1);
}
