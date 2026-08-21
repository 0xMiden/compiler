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

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

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

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();
    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "bar");

    // Nothing was renamed
    assert!(module.linkage_renames.is_empty());
    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "bar");
}

#[test]
fn func_name_sanitation_does_not_modify_name_section() {
    let mut module = module_with_func_names(&[(0, "foo"), (2, "foo")]);

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

    // Name section is unmodified
    assert_eq!(
        module.name_section.func_names.get(&FuncIndex::new(0)),
        Some(&Symbol::intern("foo"))
    );
    assert_eq!(
        module.name_section.func_names.get(&FuncIndex::new(2)),
        Some(&Symbol::intern("foo"))
    );

    // Sanitized names are recorded separately
    assert_eq!(
        module.linkage_renames.get(&FuncIndex::new(0)),
        Some(&Symbol::intern("foo_func0"))
    );
    assert_eq!(
        module.linkage_renames.get(&FuncIndex::new(2)),
        Some(&Symbol::intern("foo_func2"))
    );
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

    let err = module
        .sanitize_duplicate_func_names(&DiagnosticsHandler::default())
        .unwrap_err();

    assert!(
        err.to_string().contains("identifies an intrinsic or Miden ABI linker stub"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn renamed_func_name_colliding_with_a_survivor_gets_trailing_underscore() {
    let mut module = module_with_func_names(&[(0, "foo"), (1, "foo"), (2, "foo_func1")]);

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

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

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

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

    module.sanitize_duplicate_func_names(&DiagnosticsHandler::default()).unwrap();

    assert_eq!(module.func_name(FuncIndex::new(0)).as_str(), "foo_func0");
    assert_eq!(module.func_name(FuncIndex::new(1)).as_str(), "foo_func1_");
    assert_eq!(module.global_name(global_idx).as_str(), "foo_func1");

    assert_eq!(module.source_func_name(FuncIndex::new(0)).as_str(), "foo");
    assert_eq!(module.source_func_name(FuncIndex::new(1)).as_str(), "foo");
}
