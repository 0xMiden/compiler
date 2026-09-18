// TODO move this into ../callable_uses.rs
use alloc::format;

use midenc_hir::{
    Op, SymbolName,
    diagnostics::Uri,
    dialects::builtin::{Module, ModuleBuilder, ModuleRef},
    parse::{ParserConfig, parse, parse_any},
    testing::Test,
};

use super::*;

fn fixture(visibility: &str, extra_use: &str) -> (Test, ModuleRef) {
    let test = Test::default();
    test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
    let source = format!(
        r#"
builtin.module public @test {{
    builtin.function private extern("C") @body() {{ builtin.ret; }};
    builtin.function_alias private @first -> @body;
    builtin.function_alias {visibility} @api -> @first;
    builtin.function public extern("C") @caller() {{
        hir.exec @api() : extern("C") () -> ();
        hir.exec @body() : extern("C") () -> ();
        {extra_use}
        builtin.ret;
    }};
}};
"#
    );
    let module =
        parse::<Module>(ParserConfig::new(test.context_rc()), Uri::new("uses.hir"), &source)
            .unwrap();
    (test, module)
}

#[test]
fn alias_use_queries_separate_callers_exposure_and_address_taking() {
    for (visibility, extra, exposed, address_taken) in [
        ("private", "", false, false),
        ("public", "", true, false),
        ("private", "%a, %b, %c, %d = hir.procedure_root @api;", false, true),
    ] {
        let (_test, module) = fixture(visibility, extra);
        let body = ModuleBuilder::new(module).resolve_callable("body").unwrap().target();
        let uses = CallableUseAnalysis::new(module.borrow().as_operation());
        let body_uses = uses.get(body).unwrap();
        assert_eq!(body_uses.known_callers().len(), 2);
        // TODO check these known_callers are the expected ones
        assert_eq!(body_uses.has_external_name(), exposed);
        assert_eq!(body_uses.is_address_taken(), address_taken);
        assert!(!body_uses.has_out_of_scope_uses());
        assert_eq!(body_uses.has_unknown_callers(), exposed || address_taken);

        // The analysis does not rewrite the alias chain
        let alias = ModuleBuilder::new(module).get_function_alias("api").unwrap();
        assert_eq!(alias.borrow().target().path().name(), SymbolName::intern("first"));
        // TODO check first still points to body
    }
}

/// Verifies that a symbol attribute referencing a callable is treated as an escaping use (address
/// taken), whether it is attached to a call op or to an alias op, and that it does not change the
/// direct-caller set.
// TODO split this into two tests instead of looping over `[false, true]`
#[test]
fn extra_symbol_attributes_on_calls_and_aliases_are_escaping_uses() {
    for attach_to_call_op in [false, true] {
        let (_test, module) = fixture("private", "");
        let mb = ModuleBuilder::new(module);
        let target = mb.resolve_callable("body").unwrap().target();
        let alias = mb.get_function_alias("api").unwrap();
        let (mut owner, referenced, path) = if attach_to_call_op {
            // Attach attribute to `hir.exec @api` call op
            let owner = mb
                .get_function("caller")
                .unwrap()
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .iter()
                .find_map(|op| {
                    op.downcast_ref::<midenc_dialect_hir::Exec>().map(Op::as_operation_ref)
                })
                .unwrap();
            let path = owner
                .borrow()
                .downcast_ref::<midenc_dialect_hir::Exec>()
                .unwrap()
                .callee()
                .path()
                .clone();
            (owner, mb.resolve_callable("api").unwrap().named_symbol() as SymbolRef, path)
        } else {
            // Attach attribute to alias op
            (
                alias.borrow().as_operation_ref(),
                mb.resolve_callable("first").unwrap().named_symbol() as SymbolRef,
                alias.borrow().target().path().clone(),
            )
        };
        owner.borrow_mut().set_symbol_attribute("extra_address", referenced);

        let mut attr = owner
            .borrow()
            .get_attribute("extra_address")
            .unwrap()
            .try_downcast_attr::<midenc_hir::dialects::builtin::attributes::SymbolRefAttr>()
            .unwrap();
        attr.borrow_mut().set_path(path);
        let uses = CallableUseAnalysis::new(module.borrow().as_operation());
        let info = uses.get(target).unwrap();
        assert_eq!(info.known_callers().len(), 2);
        // TODO check known callers equal expected values
        assert!(info.is_address_taken());
        assert!(info.has_unknown_callers());
    }
}

#[test]
fn alias_names_outside_analysis_scope_still_expose_the_target() {
    let (_test, module) = fixture("public", "");
    let target = ModuleBuilder::new(module).resolve_callable("body").unwrap().target();
    let body = target.as_operation_ref();
    let uses = CallableUseAnalysis::new(&body.borrow());
    let info = uses.get(target).unwrap();
    assert!(
        info.known_callers().is_empty(),
        "callers outside the scope are not known local callers"
    );
    assert!(
        info.has_external_name(),
        "the public alias is outside the walked operation tree"
    );
    assert!(info.has_out_of_scope_uses());
    assert!(info.has_unknown_callers());
}

#[test]
fn indirect_alias_calls_are_known_but_table_uses_still_escape() {
    let test = Test::default();
    test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
    let module = parse::<Module>(
        ParserConfig::new(test.context_rc()),
        Uri::new("table_uses.hir"),
        r#"
builtin.module public @test {
    builtin.function private extern("C") @body() { builtin.ret; };
    builtin.function_alias private @api -> @body;
    builtin.function_table private @table : 2 {
        builtin.function_table_entry 0 @api tag 1;
        builtin.function_table_entry 1 @body tag 1;
    };
    builtin.function public extern("C") @caller(%index: u32) {
        hir.exec_indirect @table[%index]() : extern("C") () -> () tag 1;
        builtin.ret;
    };
};
"#,
    )
    .unwrap();
    let target = ModuleBuilder::new(module).resolve_callable("body").unwrap().target();
    let uses = CallableUseAnalysis::new(module.borrow().as_operation());
    let info = uses.get(target).unwrap();
    assert_eq!(info.known_callers().len(), 1, "should deduplicate canonical table targets");
    assert!(info.is_address_taken());
    assert!(info.has_unknown_callers());
    assert!(!info.has_external_name());
}

// TODO add another test with function table: to entries pointing at two different functions
// then also @table[%index](). can't know which target is reached -> known caller?

#[test]
fn internal_alias_visibility_depends_on_analysis_scope() {
    let test = Test::default();
    let world = parse_any(
        ParserConfig::new(test.context_rc()),
        Uri::new("internal_alias.hir"),
        r#"
builtin.world {
    builtin.module public @test {
        builtin.function private extern("C") @body() { builtin.ret; };
        builtin.function_alias internal @api -> @body;
    };
};
"#,
    )
    .unwrap();
    let module = world
        .borrow()
        .as_symbol_table()
        .unwrap()
        .get(SymbolName::intern("test"))
        .unwrap();
    let module = module.borrow().as_operation_ref().try_downcast_op::<Module>().unwrap();
    let target = ModuleBuilder::new(module).resolve_callable("body").unwrap().target();
    let whole_world = CallableUseAnalysis::new(&world.borrow());
    let info = whole_world.get(target).unwrap();
    // The internal alias stays within the analyzed world, and there are no escaping uses.
    assert!(!info.has_external_name());
    assert!(!info.has_unknown_callers());

    let module_only = CallableUseAnalysis::new(module.borrow().as_operation());
    let info = module_only.get(target).unwrap();
    // The internal alias permits callers elsewhere in the world, outside the analyzed module.
    assert!(info.has_external_name());
    assert!(info.has_unknown_callers());
}

// TODO add following test
// fn foo
// alias alias1->foo
// alias alias2->
// fn entrypoint which call both alias1 and alias2
// then check analysis returns expected results
