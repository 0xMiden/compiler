//! Procedural macros for author-side codec components.

#![deny(missing_docs)]

extern crate proc_macro;

mod expand;
mod registry;

use proc_macro::TokenStream;
use syn::{ItemImpl, LitStr, parse_macro_input};

/// Generates host-profile note types from the package built by a Miden project.
///
/// The project path is relative to `CARGO_MANIFEST_DIR`. Build the note project with
/// `cargo miden build` before compiling the codec crate.
#[proc_macro]
pub fn from_project(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand::from_project(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generates host-profile note types from one exact Miden package path.
///
/// A relative path is resolved against `CARGO_MANIFEST_DIR`.
#[proc_macro]
pub fn from_package(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand::from_package(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generates host-profile note types from WIT text for internal tests.
#[doc(hidden)]
#[proc_macro]
pub fn from_wit_text(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand::from_wit_text(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Marks an [`miden_codec::AuthorTypeCodec`] implementation for component export.
#[proc_macro_attribute]
pub fn codec(args: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as ItemImpl);
    expand::codec(args.into(), item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Exports all codecs marked earlier in the crate through the `miden:codec` component world.
///
/// Place this macro after the generated schema types and every `#[codec]` implementation.
/// Procedural macros register codecs as they expand; supported types are exported in canonical FQN
/// order.
#[proc_macro]
pub fn export_codecs(input: TokenStream) -> TokenStream {
    expand::export_codecs(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[cfg(test)]
mod tests;
