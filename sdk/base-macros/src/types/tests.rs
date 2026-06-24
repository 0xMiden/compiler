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
    assert!(type_ref.requires_core_type_import());
    let exported_names = HashSet::new();
    ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect("core types require no export");
}

#[test]
fn allows_wit_primitive_type_without_export_attribute() {
    reset_export_type_registry_for_tests();
    let ty: Type = syn::parse_str("u64").unwrap();
    let exported = HashMap::new();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("u64 should resolve");
    assert_eq!(type_ref.wit_name, "u64");
    assert!(!type_ref.is_custom);
    assert!(!type_ref.requires_core_type_import());
    let exported_names = HashSet::new();
    ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect("primitive types require no export");
}

#[test]
fn struct_fields_allow_wit_primitive_types() {
    reset_export_type_registry_for_tests();
    let item: syn::ItemStruct = parse_quote! {
        struct Foo {
            first: u64,
            second: u32,
            third: u8,
        }
    };
    let def = exported_type_from_struct(&item).expect("struct definition should parse");
    let exported_names = HashSet::from([def.wit_name.clone()]);

    let ExportedTypeKind::Record { fields } = &def.kind else {
        panic!("expected record kind");
    };
    for field in fields {
        assert!(!field.ty.is_custom);
        assert!(!field.ty.requires_core_type_import());
        ensure_custom_type_defined(&field.ty, &exported_names, Span::call_site())
            .expect("primitive fields should not need #[export_type]");
    }
}

#[test]
fn maps_rust_primitive_types_to_wit_types() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let exported_names = HashSet::new();
    for (rust_type, wit_type) in [
        ("bool", "bool"),
        ("i8", "s8"),
        ("u8", "u8"),
        ("i16", "s16"),
        ("u16", "u16"),
        ("i32", "s32"),
        ("u32", "u32"),
        ("i64", "s64"),
        ("u64", "u64"),
    ] {
        let ty: Type = syn::parse_str(rust_type).unwrap();
        let type_ref = map_type_to_type_ref(&ty, &exported).expect("primitive should resolve");
        assert_eq!(type_ref.wit_name, wit_type);
        assert!(!type_ref.is_custom, "{rust_type} should not be custom");
        assert!(
            !type_ref.requires_core_type_import(),
            "{rust_type} should not require a core type import"
        );
        ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
            .expect("primitive types require no export");
    }
}

#[test]
fn rejects_unsupported_component_primitives() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();

    for rust_type in ["f32", "f64", "char"] {
        let ty: Type = syn::parse_str(rust_type).unwrap();
        let err = map_type_to_type_ref(&ty, &exported)
            .expect_err("unsupported primitive should be rejected");

        assert!(
            err.to_string().contains("is not supported in component interfaces yet"),
            "error message should explain unsupported primitive: {err}"
        );
    }
}

#[test]
fn rejects_unsupported_component_primitives_nested_in_option_or_result() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();

    for rust_type in ["Option<f32>", "Option<char>", "Result<f64, u32>", "Result<u32, char>"] {
        let ty: Type = syn::parse_str(rust_type).unwrap();
        let err = map_type_to_type_ref(&ty, &exported)
            .expect_err("nested unsupported primitive should be rejected");

        assert!(
            err.to_string().contains("is not supported in component interfaces yet"),
            "error message should explain nested unsupported primitive: {err}"
        );
    }
}

#[test]
fn maps_rust_option_type_to_wit_option() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let exported_names = HashSet::new();
    let ty: Type = syn::parse_str("Option<u64>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("option should resolve");

    assert_eq!(type_ref.wit_name, "option<u64>");
    assert!(!type_ref.is_custom);
    assert!(!type_ref.requires_core_type_import());
    ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect("primitive option should require no export");
}

#[test]
fn option_type_tracks_nested_core_type_imports() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let ty: Type = syn::parse_str("Option<Word>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("option should resolve");
    let mut imports = Vec::new();

    type_ref.add_required_core_type_imports(&mut imports);

    assert_eq!(type_ref.wit_name, "option<word>");
    assert_eq!(imports, vec!["word"]);
}

#[test]
fn option_type_validates_nested_custom_type() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let ty: Type = syn::parse_str("Option<LocalType>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("option should resolve");
    let exported_names = HashSet::new();
    let err = ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect_err("expected unresolved type error");

    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}

#[test]
fn maps_rust_result_type_to_wit_result() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let exported_names = HashSet::new();
    let ty: Type = syn::parse_str("Result<u64, Felt>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("result should resolve");

    assert_eq!(type_ref.wit_name, "result<u64, felt>");
    assert!(!type_ref.is_custom);
    assert!(!type_ref.requires_core_type_import());
    ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect("result should require no unresolved custom export");
}

#[test]
fn result_type_tracks_nested_core_type_imports() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let ty: Type = syn::parse_str("Result<Word, Felt>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("result should resolve");
    let mut imports = Vec::new();

    type_ref.add_required_core_type_imports(&mut imports);

    assert_eq!(type_ref.wit_name, "result<word, felt>");
    assert_eq!(imports, vec!["word", "felt"]);
}

#[test]
fn result_type_validates_nested_custom_type() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let ty: Type = syn::parse_str("Result<u64, LocalType>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("result should resolve");
    let exported_names = HashSet::new();
    let err = ensure_custom_type_defined(&type_ref, &exported_names, Span::call_site())
        .expect_err("expected unresolved type error");

    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}

#[test]
fn result_type_maps_unit_argument_to_wit_placeholder() {
    reset_export_type_registry_for_tests();
    let exported = HashMap::new();
    let ty: Type = syn::parse_str("Result<(), Felt>").unwrap();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("result should resolve");

    assert_eq!(type_ref.wit_name, "result<_, felt>");
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
