//! Tests for codec macro registration and expansion.

use std::sync::{Mutex, MutexGuard};

use proc_macro2::Span;
use quote::quote;
use syn::{ItemImpl, LitStr};

use crate::{expand, registry::reset_for_tests};

static REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serializes tests that exercise the process-global procedural-macro registry.
fn lock_registry() -> MutexGuard<'static, ()> {
    REGISTRY_TEST_LOCK.lock().expect("registry test mutex is poisoned")
}

const SCHEMA: &str = r#"
package example:codec-schema@1.0.0;

interface note-storage {
    record ratio {
        numerator: u64,
        denominator: u64,
    }

    record codec-note {
        ratio: ratio,
    }

    type storage = codec-note;
}
"#;

const EMBEDDED_CORE_SCHEMA: &str = r#"
package example:embedded-core-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{digest};
    record embedded-core-note { value: digest }
    type storage = embedded-core-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record word { a: felt, b: felt, c: felt, d: felt }
        record digest { inner: word }
    }
}
"#;

#[test]
fn schema_and_codec_registration_generate_native_and_wasm_dispatch() {
    let _guard = lock_registry();
    reset_for_tests();
    let schema = LitStr::new(SCHEMA, Span::call_site());
    let generated = expand::from_wit_text(&schema).unwrap();
    let item: ItemImpl = syn::parse2(quote! {
        impl miden_note_codec::AuthorTypeCodec for Ratio {
            fn parse(_value: &str) -> Result<Self, String> { todo!() }
            fn display(&self) -> String { todo!() }
            fn validate(&self) -> Result<(), String> { todo!() }
        }
    })
    .unwrap();
    expand::note_codec(quote!(), item).unwrap();
    let root_item: ItemImpl = syn::parse2(quote! {
        impl miden_note_codec::AuthorTypeCodec for CodecNote {
            fn parse(_value: &str) -> Result<Self, String> { todo!() }
            fn display(&self) -> String { todo!() }
            fn validate(&self) -> Result<(), String> { todo!() }
        }
    })
    .unwrap();
    expand::note_codec(quote!(), root_item).unwrap();
    let exported = expand::export_codecs(quote!()).unwrap();
    let source = prettyplease::unparse(&syn::parse2(quote!(#generated #exported)).unwrap());

    assert!(source.contains("pub struct CodecNote"));
    assert!(source.contains("pub struct Ratio"));
    assert!(source.contains("::miden_note_codec::__private::miden_field_repr"));
    assert!(source.contains("felt_repr(crate_path"));
    assert!(!source.contains("extern crate"));
    assert!(source.contains("example:codec-schema/note-storage@1.0.0.codec-note"));
    assert!(source.contains("example:codec-schema/note-storage@1.0.0.ratio"));
    assert!(source.contains("cfg(target_family = \"wasm\")"));
    assert!(source.contains("exports::miden::note_codec::codec::Guest"));
}

#[test]
fn a_crate_cannot_register_two_distinct_schemas() {
    let _guard = lock_registry();
    reset_for_tests();
    let first = LitStr::new(SCHEMA, Span::call_site());
    expand::from_wit_text(&first).unwrap();

    let second_schema = SCHEMA
        .replace("example:codec-schema", "example:other-schema")
        .replace("codec-note", "other-note");
    let second = LitStr::new(&second_schema, Span::call_site());
    let error = expand::from_wit_text(&second).unwrap_err().to_string();

    assert!(error.contains("one note schema per crate"));
    assert!(error.contains("second distinct"));
}

#[test]
fn export_requires_earlier_codec_declarations() {
    let _guard = lock_registry();
    reset_for_tests();
    let error = expand::export_codecs(quote!()).unwrap_err().to_string();

    assert!(error.contains("found no registered codecs"));
    assert!(error.contains("declaration order"));
    assert!(error.contains("#[note_codec]"));
}

#[test]
fn codec_registry_uses_canonical_standard_leaf_definition() {
    let _guard = lock_registry();
    reset_for_tests();
    let schema = LitStr::new(EMBEDDED_CORE_SCHEMA, Span::call_site());
    let generated = expand::from_wit_text(&schema).unwrap();
    let digest_impl: ItemImpl = syn::parse2(quote! {
        impl miden_note_codec::AuthorTypeCodec for Digest {
            fn parse(_value: &str) -> Result<Self, String> { todo!() }
            fn display(&self) -> String { todo!() }
            fn validate(&self) -> Result<(), String> { todo!() }
        }
    })
    .unwrap();
    expand::note_codec(quote!(), digest_impl).unwrap();

    let word_impl: ItemImpl = syn::parse2(quote! {
        impl miden_note_codec::AuthorTypeCodec for Word {
            fn parse(_value: &str) -> Result<Self, String> { todo!() }
            fn display(&self) -> String { todo!() }
            fn validate(&self) -> Result<(), String> { todo!() }
        }
    })
    .unwrap();
    let error = expand::note_codec(quote!(), word_impl).unwrap_err().to_string();

    let source = prettyplease::unparse(&syn::parse2(quote!(#generated)).unwrap());
    assert!(source.contains("pub struct Digest"));
    assert!(error.contains("not part of a registered note schema"));
}
