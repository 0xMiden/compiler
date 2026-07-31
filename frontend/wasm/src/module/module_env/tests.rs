use super::*;

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
