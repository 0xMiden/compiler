use super::*;

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
                path: "miden::basic_wallet::basic_wallet::receive_asset".to_string(),
            }],
            ..Default::default()
        },
        ParsedModule {
            component_frontend_metadata: vec![
                FrontendMetadata::AccountProcedure {
                    method_path: "crate::wallet::BasicWallet::move_asset_to_note".to_string(),
                    path: "miden::basic_wallet::basic_wallet::move_asset_to_note".to_string(),
                },
                FrontendMetadata::AccountProcedure {
                    method_path: "crate::wallet::BasicWallet::create_note".to_string(),
                    path: "miden::basic_wallet::basic_wallet::create_note".to_string(),
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
        path: "miden::auth::auth::auth".to_string(),
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
        err.to_string().contains("expected lifted export `miden::auth::auth::auth`"),
        "unexpected error: {err:?}"
    );
}

/// Ensures metadata validation reports a missing lifted export for an `#[account_procedure]` entry.
#[test]
fn component_frontend_metadata_reports_missing_account_procedure_export() {
    let metadata = [FrontendMetadata::AccountProcedure {
        method_path: "crate::wallet::BasicWallet::receive_asset".to_string(),
        path: "miden::basic_wallet::basic_wallet::receive_asset".to_string(),
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
        err.to_string()
            .contains("expected lifted export `miden::basic_wallet::basic_wallet::receive_asset`"),
        "unexpected error: {err:?}"
    );
}

/// Imports of every kind share one list in declaration order, so a function import is found by
/// its entity index, not by its position.
#[test]
fn function_imports_are_looked_up_past_other_imports() {
    let wasm = wat::parse_str(
        r#"
        (module
            (import "env" "memory" (memory 1))
            (import "env" "first" (func (result i32)))
            (import "env" "second" (func (result i32)))
        )
        "#,
    )
    .expect("module WAT should compile");
    let config = WasmTranslationConfig::default();
    let mut validator = Validator::new_with_features(crate::supported_features());
    let mut types = ModuleTypesBuilder::default();
    let context = midenc_hir::Context::default();
    let parsed = ModuleEnvironment::new(&config, &mut validator, &mut types)
        .parse(Parser::new(0), &wasm, context.diagnostics())
        .expect("module should parse");
    let module = &parsed.module;

    let field_of = |index: u32| {
        module
            .function_import(FuncIndex::from_u32(index))
            .map(|import| import.field.as_str())
    };
    assert_eq!(field_of(0), Some("first"));
    assert_eq!(field_of(1), Some("second"));
    assert_eq!(field_of(2), None);
}
