// TODO move to `../interface.rs` and remove this directory

use super::*;
use crate::{
    diagnostics::Uri,
    dialects::builtin::ModuleBuilder,
    parse::{ParserConfig, parse, parse_any},
    testing::Test,
};

#[test]
fn module_interface_exports_alias_names_not_duplicate_bodies() {
    let test = Test::default();
    let module = parse::<Module>(
        ParserConfig::new(test.context_rc()),
        Uri::new("exports.hir"),
        r#"
builtin.module public @api {
    builtin.function private extern("C") @foo() { builtin.ret; };
    builtin.function_alias private @alias_private -> @foo;
    builtin.function_alias public @alias_public -> @alias_private;
    builtin.function_alias internal @alias_internal -> @foo;
};
"#,
    )
    .unwrap();

    let module = module.borrow();
    assert_eq!(module.functions().count(), 1);
    assert_eq!(module.defined_functions().count(), 1);
    assert_eq!(module.callable_symbols().count(), 4);

    // There are two exports both pointing at the same target
    let exports = module.exported_callables().collect::<Result<alloc::vec::Vec<_>, _>>().unwrap();
    assert_eq!(exports.len(), 2);
    assert_eq!(exports[0].target(), exports[1].target());

    // Exports picked up corretly
    let interface = ModuleInterface::try_new(&module).unwrap();
    assert!(!interface.is_externally_defined());
    assert!(interface.imports().is_empty());
    assert_eq!(interface.exports().len(), 2);
    assert_eq!(
        interface.exports().keys().copied().collect::<alloc::collections::BTreeSet<_>>(),
        alloc::collections::BTreeSet::from([
            SymbolName::intern("alias_public"),
            SymbolName::intern("alias_internal"),
        ])
    );

    // A component which exports the module above under the name `api`
    let provider = ComponentInterface {
        id: "test:provider".parse().unwrap(),
        visibility: Visibility::Public,
        is_externally_defined: false,
        imports: FxHashMap::default(),
        exports: FxHashMap::from_iter([(
            SymbolName::intern("api"),
            ComponentExport::Module(interface),
        )]),
    };

    // Each case is `(import name, import parameter types, whether component provides it)`
    for (name, params, expected) in [
        ("alias_public", &[][..], true),
        ("alias_internal", &[][..], true),
        ("alias_public", &[crate::Type::U32][..], false),
        ("missing", &[][..], false),
    ] {
        let requirement = Test::default().in_module("api");
        let mut builder = ModuleBuilder::new(requirement.module());
        builder
            .define_function(
                name.into(),
                Visibility::Public,
                Signature::with_convention(
                    &requirement.context_rc(),
                    crate::CallConv::C,
                    params.iter().cloned(),
                    [],
                ),
            )
            .unwrap();
        assert_eq!(
            provider.provides_module(&ModuleInterface::new(&requirement.module().borrow())),
            expected
        );
    }
}

#[test]
fn alias_to_another_module_exports_a_name_without_owning_its_body() {
    let test = Test::default();
    let world = parse_any(
        ParserConfig::new(test.context_rc()),
        Uri::new("cross_module_exports.hir"),
        r#"
builtin.world {
    builtin.module public @implementation {
        builtin.function public extern("C") @body() { builtin.ret; };
    };
    builtin.module public @api {
        builtin.function_alias public @foo -> ::@implementation::@body;
    };
};
"#,
    )
    .unwrap();

    let api = world
        .borrow()
        .as_symbol_table()
        .unwrap()
        .get(SymbolName::intern("api"))
        .unwrap();
    let api = api.borrow();
    let module = api.as_symbol_operation().downcast_ref::<Module>().unwrap();
    assert_eq!(module.defined_functions().count(), 0);
    let interface = ModuleInterface::try_new(module).unwrap();
    assert!(!interface.is_externally_defined());
    assert_eq!(
        interface.exports().keys().copied().collect::<alloc::collections::BTreeSet<_>>(),
        alloc::collections::BTreeSet::from([SymbolName::intern("foo")])
    );

    // `api` owns no function body: its `foo` alias canonicalizes to the function defined in
    // `implementation`
    let foo = module.get(SymbolName::intern("foo")).unwrap();
    let target = foo.resolve_canonical().unwrap();
    let implementation = world
        .borrow()
        .as_symbol_table()
        .unwrap()
        .get(SymbolName::intern("implementation"))
        .unwrap();
    let implementation = implementation.borrow();
    let implementation = implementation.as_symbol_operation().downcast_ref::<Module>().unwrap();
    let body = implementation.get(SymbolName::intern("body")).unwrap();
    assert_eq!(target, body);
}

#[test]
fn declarations_remain_imports_and_invalid_alias_exports_are_reported() {
    let test = Test::default();
    let mut module = parse::<Module>(
        ParserConfig::new(test.context_rc()),
        Uri::new("import_alias.hir"),
        r#"
builtin.module public @api {
    builtin.function public extern("C") @foo_1() {};
    builtin.function_alias public @foo_2 -> @foo_1;
};
"#,
    )
    .unwrap();
    let interface = ModuleInterface::new(&module.borrow());
    assert_eq!(
        interface.imports().keys().copied().collect::<alloc::collections::BTreeSet<_>>(),
        alloc::collections::BTreeSet::from([SymbolName::intern("foo_1")])
    );
    assert_eq!(
        interface.exports().keys().copied().collect::<alloc::collections::BTreeSet<_>>(),
        alloc::collections::BTreeSet::from([SymbolName::intern("foo_2")])
    );

    // Alias whose target is missing causes error
    assert_eq!(module.borrow().defined_functions().count(), 0);
    let alias = ModuleBuilder::new(module).get_function_alias("foo_2").unwrap();
    module.borrow_mut().remove(SymbolName::intern("foo_1"));
    assert!(alias.borrow().resolve_target().is_none());
    assert_eq!(
        ModuleInterface::try_new(&module.borrow()).err().unwrap(),
        crate::SymbolResolutionError::UnknownSymbol {
            path: crate::SymbolPath::from_iter([crate::SymbolNameComponent::Leaf(
                SymbolName::intern("foo_1")
            )])
        }
    );
}
