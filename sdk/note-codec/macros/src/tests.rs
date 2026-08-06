//! Tests for codec macro registration and expansion.

use proc_macro2::Span;
use quote::quote;
use syn::{ItemImpl, LitStr};

use crate::{expand, registry::reset_for_tests};

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

#[test]
fn schema_and_codec_registration_generate_native_and_wasm_dispatch() {
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
    assert!(source.contains("example:codec-schema/note-storage@1.0.0.codec-note"));
    assert!(source.contains("example:codec-schema/note-storage@1.0.0.ratio"));
    assert!(source.contains("cfg(target_family = \"wasm\")"));
    assert!(source.contains("exports::miden::note_codec::codec::Guest"));
}
