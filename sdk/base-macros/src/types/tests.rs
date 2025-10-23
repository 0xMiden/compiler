use syn::parse_quote;

use super::*;

#[test]
fn emits_hint_for_missing_export_type() {
    let ty: Type = syn::parse_str("LocalType").unwrap();
    let exported = HashMap::new();
    let err = map_type_to_type_ref(&ty, &exported).expect_err("expected failure");
    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}

#[test]
fn allows_sdk_type_without_export_attribute() {
    let ty: Type = syn::parse_str("Asset").unwrap();
    let exported = HashMap::new();
    let type_ref = map_type_to_type_ref(&ty, &exported).expect("asset should resolve");
    assert_eq!(type_ref.wit_name, "asset");
    assert!(!type_ref.is_custom);
}

#[test]
fn struct_field_missing_export_type_hint() {
    let item: syn::ItemStruct = parse_quote! {
        struct Foo {
            value: LocalType,
        }
    };
    let err = exported_type_from_struct(&item).expect_err("expected failure");
    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}

#[test]
fn enum_payload_missing_export_type_hint() {
    let item: syn::ItemEnum = parse_quote! {
        enum Foo {
            Variant(LocalType),
        }
    };
    let err = exported_type_from_enum(&item).expect_err("expected failure");
    assert!(
        err.to_string().contains("add #[export_type]"),
        "error message missing hint: {err}"
    );
}
