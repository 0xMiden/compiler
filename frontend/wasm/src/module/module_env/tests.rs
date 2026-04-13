use super::*;

/// Ensures duplicate `#[auth_script]` metadata across modules is rejected at merge time.
#[test]
fn component_frontend_metadata_rejects_duplicate_auth_exports() {
    let modules = [
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::AuthScript {
                export_name: "auth_a".to_string(),
            }),
            ..Default::default()
        },
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::AuthScript {
                export_name: "auth_b".to_string(),
            }),
            ..Default::default()
        },
    ];

    let err = merge_frontend_metadata(modules.iter()).unwrap_err();

    assert!(
        err.to_string().contains("multiple `#[auth_script]` procedures were found"),
        "unexpected error: {err:?}"
    );
}

/// Ensures duplicate `#[note_script]` metadata across modules is rejected at merge time.
#[test]
fn component_frontend_metadata_rejects_duplicate_note_script_exports() {
    let modules = [
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::NoteScript {
                export_name: "note_a".to_string(),
            }),
            ..Default::default()
        },
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::NoteScript {
                export_name: "note_b".to_string(),
            }),
            ..Default::default()
        },
    ];

    let err = merge_frontend_metadata(modules.iter()).unwrap_err();

    assert!(
        err.to_string().contains("multiple `#[note_script]` procedures were found"),
        "unexpected error: {err:?}"
    );
}

/// Ensures mixed frontend metadata kinds across modules are rejected at merge time.
#[test]
fn component_frontend_metadata_rejects_mixed_export_kinds() {
    let modules = [
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::AuthScript {
                export_name: "auth".to_string(),
            }),
            ..Default::default()
        },
        ParsedModule {
            component_frontend_metadata: Some(FrontendMetadata::NoteScript {
                export_name: "note".to_string(),
            }),
            ..Default::default()
        },
    ];

    let err = merge_frontend_metadata(modules.iter()).unwrap_err();

    assert!(
        err.to_string()
            .contains("both `#[auth_script]` and `#[note_script]` procedures were found"),
        "unexpected error: {err:?}"
    );
}

/// Ensures metadata validation reports when a marked export was not lifted into the component.
#[test]
fn component_frontend_metadata_reports_missing_lifted_exports() {
    let metadata = FrontendMetadata::AuthScript {
        export_name: "auth".to_string(),
    };
    let lifted_exports = FxHashSet::default();

    let err =
        validate_lifted_frontend_metadata_exports(Some(&metadata), &lifted_exports).unwrap_err();

    assert!(
        err.to_string()
            .contains("failed to find the component export marked with `#[auth_script]`: `auth`"),
        "unexpected error: {err:?}"
    );
}
