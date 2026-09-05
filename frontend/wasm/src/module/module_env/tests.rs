use cranelift_entity::EntityRef;

use super::*;
use crate::module::types::Global;

#[test]
fn standalone_dwarf_offsets_are_code_section_relative() {
    let info = WasmFileInfo {
        code_section_offset: 12,
        ..Default::default()
    };

    assert_eq!(info.dwarf_offset(20), 8);
}

/// Ensures the frontend metadata entries emitted across a component's core modules are collected
/// into one list — in particular the several `#[account_procedure]` entries of an account
/// component.
#[test]
fn component_frontend_metadata_collects_account_procedures() {
    let modules = [
        ParsedModule {
            component_frontend_metadata: vec![FrontendMetadata::AccountProcedure {
                method_path: "crate::wallet::BasicWallet::receive_asset".to_string(),
                export_name: "receive-asset".to_string(),
            }],
            ..Default::default()
        },
        ParsedModule {
            component_frontend_metadata: vec![
                FrontendMetadata::AccountProcedure {
                    method_path: "crate::wallet::BasicWallet::move_asset_to_note".to_string(),
                    export_name: "move-asset-to-note".to_string(),
                },
                FrontendMetadata::AccountProcedure {
                    method_path: "crate::wallet::BasicWallet::create_note".to_string(),
                    export_name: "create-note".to_string(),
                },
            ],
            ..Default::default()
        },
    ];

    let merged = merge_frontend_metadata(modules.iter());

    assert_eq!(merged.len(), 3);
}

/// Ensures metadata validation reports when an `#[auth_script]` export was not lifted into the
/// component.
#[test]
fn component_frontend_metadata_reports_missing_lifted_exports() {
    let metadata = [FrontendMetadata::AuthScript {
        method_path: "crate::auth::AuthComponent::authenticate".to_string(),
        export_name: "auth".to_string(),
    }];
    let lifted_exports = FxHashSet::default();

    let err = validate_lifted_frontend_metadata_exports(&metadata, &lifted_exports).unwrap_err();

    assert!(
        err.to_string().contains(
            "failed to find the component export marked with `#[auth_script]`: \
             `crate::auth::AuthComponent::authenticate`"
        ),
        "unexpected error: {err:?}"
    );
    assert!(
        err.to_string().contains("expected lifted export `auth`"),
        "unexpected error: {err:?}"
    );
}

/// Ensures metadata validation reports a missing lifted export for an `#[account_procedure]` entry.
#[test]
fn component_frontend_metadata_reports_missing_account_procedure_export() {
    let metadata = [FrontendMetadata::AccountProcedure {
        method_path: "crate::wallet::BasicWallet::receive_asset".to_string(),
        export_name: "receive-asset".to_string(),
    }];
    let lifted_exports = FxHashSet::default();

    let err = validate_lifted_frontend_metadata_exports(&metadata, &lifted_exports).unwrap_err();

    assert!(
        err.to_string().contains(
            "failed to find the component export marked with `#[account_procedure]`: \
             `crate::wallet::BasicWallet::receive_asset`"
        ),
        "unexpected error: {err:?}"
    );
    assert!(
        err.to_string().contains("expected lifted export `receive-asset`"),
        "unexpected error: {err:?}"
    );
}

fn module_with_func_names(names: &[(u32, &str)]) -> Module {
    let mut module = Module::default();
    let max = names.iter().map(|(index, _)| *index).max();
    if let Some(max) = max {
        // Dummy signature since here we only care about function names
        let sig = SignatureIndex::from_u32(0);
        for _ in 0..=max {
            module.push_function(sig);
        }
    }
    for (index, name) in names {
        module
            .name_section
            .func_names
            .insert(FuncIndex::new(*index as usize), Symbol::intern(*name));
    }
    module
}

#[test]
fn duplicate_func_names_are_renamed() {
    let mut module = module_with_func_names(&[(0, "foo"), (2, "foo"), (1, "bar")]);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "bar");
    assert_eq!(module.func_name(FuncIndex::new(2)).as_str(), "foo_func2");

    // The name section is not modified
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "bar");
    assert_eq!(module.source_func_name(FuncIndex::new(2)).as_str(), "foo");
}

#[test]
fn unique_func_names_are_kept() {
    let mut module = module_with_func_names(&[(0, "foo"), (1, "bar")]);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();
    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "bar");

    // Nothing was renamed
    assert_eq!(module.func_name(FuncIndex::new(0)), module.source_func_name(FuncIndex::new(0)));
    assert_eq!(module.func_name(FuncIndex::new(1)), module.source_func_name(FuncIndex::new(1)));
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "bar");
}

#[test]
fn linkage_names_do_not_modify_name_section() {
    let mut module = module_with_func_names(&[(0, "foo"), (2, "foo")]);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    // Name section is unmodified
    assert_eq!(
        module.name_section.func_names.get(&FuncIndex::new(0)),
        Some(&Symbol::intern("foo"))
    );
    assert_eq!(
        module.name_section.func_names.get(&FuncIndex::new(2)),
        Some(&Symbol::intern("foo"))
    );

    // Sanitized names are recorded in the linkage map
    assert_eq!(
        module.func_linkages.get(FuncIndex::new(0)).copied(),
        Some(Symbol::intern("foo_func0"))
    );
    assert_eq!(
        module.func_linkages.get(FuncIndex::new(2)).copied(),
        Some(Symbol::intern("foo_func2"))
    );
}

/// Functions without a name-section entry (e.g. stripped binaries) use `func{index}` as both source
/// and linkage name. They must not be renamed to `func{index}_func{index}`.
#[test]
fn unnamed_functions_keep_fallback_name() {
    let mut module = Module::default();
    let sig = SignatureIndex::from_u32(0);
    module.push_function(sig);
    module.push_function(sig);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "func0");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "func1");
    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "func1");
    assert_eq!(module.func_name(FuncIndex::new(0)), module.source_func_name(FuncIndex::new(0)));
    assert_eq!(module.func_name(FuncIndex::new(1)), module.source_func_name(FuncIndex::new(1)));
}

#[test]
fn source_func_name_falls_back_to_the_synthesized_name() {
    // No name-section entry for the function: both accessors fall back to `func{index}`
    let module = module_with_func_names(&[]);

    assert_eq!(module.func_name(FuncIndex::new(3)).as_str(), "func3");
    assert_eq!(module.source_func_name(FuncIndex::new(3)).as_str(), "func3");
}

#[test]
fn duplicated_intrinsic_stub_name_is_an_error() {
    let mut module =
        module_with_func_names(&[(0, "intrinsics::felt::add"), (1, "intrinsics::felt::add")]);

    let err = module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap_err();

    assert!(
        err.to_string().contains("identifies an intrinsic or Miden ABI linker stub"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn renamed_func_name_colliding_with_a_survivor_gets_trailing_underscore() {
    let mut module = module_with_func_names(&[(0, "foo"), (1, "foo"), (2, "foo_func1")]);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "foo_func1_");
    assert_eq!(module.func_name(FuncIndex::new(2)).as_str(), "foo_func1");

    // Deduplication affects the linkage name only; the source names are unchanged
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(2)).as_str(), "foo_func1");
}

#[test]
fn renamed_func_name_appends_underscores_until_free() {
    let mut module =
        module_with_func_names(&[(0, "foo"), (1, "foo"), (2, "foo_func1"), (3, "foo_func1_")]);

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "foo_func1__");
    assert_eq!(module.func_name(FuncIndex::new(2)).as_str(), "foo_func1");
    assert_eq!(module.func_name(FuncIndex::new(3)).as_str(), "foo_func1_");
}

#[test]
fn renamed_func_name_colliding_with_a_global_gets_trailing_underscore() {
    let mut module = module_with_func_names(&[(0, "foo"), (1, "foo")]);
    let global_idx = module.globals.push(Global {
        ty: WasmType::I32,
        mutability: false,
    });
    module
        .name_section
        .globals_names
        .insert(global_idx, Symbol::intern("foo_func1"));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "foo_func1_");
    assert_eq!(module.global_name(global_idx).as_str(), "foo_func1");

    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "foo");
}

#[test]
fn export_name_becomes_linkage_name_while_source_name_is_kept() {
    let mut module = module_with_func_names(&[(0, "foo_src")]);
    module
        .exports
        .insert("foo_ex".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_ex");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo_src");
    assert_eq!(
        module.func_linkages.get(FuncIndex::new(0)).copied(),
        Some(Symbol::intern("foo_ex"))
    );
}

#[test]
fn export_name_without_name_section_becomes_linkage_name() {
    let mut module = module_with_func_names(&[]);
    // No name-section entries, but the exported function must still exist in `Module::functions`
    // for the dense maps to cover it.
    module.push_function(SignatureIndex::from_u32(0));
    module
        .exports
        .insert("foo".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "func0");
    assert_eq!(
        module.func_linkages.get(FuncIndex::new(0)).copied(),
        Some(Symbol::intern("foo"))
    );
}

#[test]
fn export_name_identical_to_source_name_records_no_linkage_override() {
    let mut module = module_with_func_names(&[(0, "foo")]);
    module
        .exports
        .insert("foo".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
}

#[test]
fn multiple_exports_for_single_function_is_an_error() {
    let mut module = module_with_func_names(&[(0, "foo")]);
    module
        .exports
        .insert("export_1".to_string(), EntityIndex::Function(FuncIndex::new(0)));
    module
        .exports
        .insert("export_2".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    let err = module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap_err();

    assert!(
        err.to_string()
            .contains("exporting a function under multiple names is not supported"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn export_name_colliding_with_global_name_is_an_error() {
    let mut module = module_with_func_names(&[(0, "my_func")]);
    let global_idx = module.globals.push(Global {
        ty: WasmType::I32,
        mutability: false,
    });
    module
        .name_section
        .globals_names
        .insert(global_idx, Symbol::intern("colliding_name"));
    module
        .exports
        .insert("colliding_name".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    let err = module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap_err();

    assert!(
        err.to_string().contains("conflicts with a global variable name"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn export_name_identifying_intrinsic_is_an_error() {
    let mut module = module_with_func_names(&[(0, "my_func")]);
    module
        .exports
        .insert("intrinsics::felt::add".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    let err = module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap_err();

    assert!(
        err.to_string().contains("identifies an intrinsic or Miden ABI linker stub"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn export_name_takes_precedence_over_unexported_source_name_collision() {
    let mut module = module_with_func_names(&[(0, "foo_src"), (1, "bar")]);
    module
        .exports
        .insert("bar".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "bar");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "bar_func1");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo_src");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "bar");
}

#[test]
fn export_name_colliding_with_fallback_renames_fallback() {
    let mut module = module_with_func_names(&[(0, "foo_src")]);
    // Second function without a name-section entry: its fallback would be `func1`.
    module.push_function(SignatureIndex::from_u32(0));
    module
        .exports
        .insert("func1".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    // Export wins, the unnamed function's fallback is renamed
    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "func1");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "func1_func1");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo_src");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "func1");
}

#[test]
fn duplicate_source_names_with_one_member_exported() {
    let mut module = module_with_func_names(&[(0, "foo"), (1, "foo")]);
    module
        .exports
        .insert("foo".to_string(), EntityIndex::Function(FuncIndex::new(0)));

    module.resolve_func_symbols(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "foo_func1");
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "foo");

    // Both functions share the raw name-section name "foo", so DWARF subprogram resolution
    // considers both duplicate (requiring low_pc fallback)
    assert!(module.is_duplicate_source_func_name(FuncIndex::new(0)));
    assert!(module.is_duplicate_source_func_name(FuncIndex::new(1)));
}
