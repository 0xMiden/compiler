use std::collections::HashSet;

use syn::parse_quote;

use super::*;

#[test]
fn emits_hint_for_missing_export_type() {
    reset_export_type_registry_for_tests();
    let ty: Type = syn::parse_str("LocalType").unwrap();
    let exported = HashMap::new();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("type resolution should succeed");
    let exported_names = HashSet::new();
    let err = ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect_err("expected failure");
    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}

#[test]
fn allows_sdk_type_without_export_attribute() {
    reset_export_type_registry_for_tests();
    let ty: Type = syn::parse_str("Asset").unwrap();
    let exported = HashMap::new();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("asset should resolve");
    assert_eq!(type_ref.wit_name, "asset");
    assert!(!type_ref.is_custom);
    let exported_names = HashSet::new();
    ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect("core types require no export");
}

#[test]
fn struct_field_missing_export_type_hint() {
    reset_export_type_registry_for_tests();
    let item: syn::ItemStruct = parse_quote! {
        struct Foo {
            value: LocalType,
        }
    };
    let def = exported_type_from_struct(&item).expect("struct definition should parse");
    let exported_names = HashSet::from([def.wit_name.clone()]);
    if let ExportedTypeKind::Record { fields } = &def.kind {
        let err = ensure_custom_type_defined(&fields[0].ty, &exported_names, Span::call_site())
            .expect_err("expected unresolved type error");
        assert!(
            err.to_string().contains("add #[export_type]"),
            "error message missing hint: {err}"
        );
    } else {
        panic!("expected record kind");
    }
}

#[test]
fn enum_payload_missing_export_type_hint() {
    reset_export_type_registry_for_tests();
    let item: syn::ItemEnum = parse_quote! {
        enum Foo {
            Variant(LocalType),
        }
    };
    let def = exported_type_from_enum(&item).expect("enum definition should parse");
    let exported_names = HashSet::from([def.wit_name.clone()]);
    if let ExportedTypeKind::Variant { variants } = &def.kind {
        if let Some(type_ref) = &variants[0].payload {
            let err = ensure_custom_type_defined(type_ref, &exported_names, Span::call_site())
                .expect_err("expected unresolved type error");
            assert!(
                err.to_string().contains("add #[export_type]"),
                "error message missing hint: {err}"
            );
        } else {
            panic!("expected payload");
        }
    } else {
        panic!("expected variant kind");
    }
}

#[test]
fn forward_reference_between_export_types_is_allowed() {
    reset_export_type_registry_for_tests();

    let first: syn::ItemStruct = parse_quote! {
        struct First {
            next: Second,
        }
    };
    let first_def = exported_type_from_struct(&first).expect("first struct should parse");

    let second: syn::ItemStruct = parse_quote! {
        struct Second {
            value: Felt,
        }
    };
    let second_def = exported_type_from_struct(&second).expect("second struct should parse");

    let exported_names = HashSet::from([first_def.wit_name.clone(), second_def.wit_name.clone()]);

    if let ExportedTypeKind::Record { fields } = &first_def.kind {
        ensure_custom_type_defined(&fields[0].ty, &exported_names, Span::call_site())
            .expect("forward reference should resolve once type is exported");
    } else {
        panic!("expected record kind");
    }
}
