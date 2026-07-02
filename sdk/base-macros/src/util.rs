use std::{env, fs, path::PathBuf};

use midenc_frontend_wasm_metadata::{
    FrontendMetadata, WASM_COMPONENT_WIT_CUSTOM_SECTION_NAME,
    WASM_FRONTEND_METADATA_CUSTOM_SECTION_NAME,
};
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::Error;

/// Folder within a project that holds bundled WIT files
const BUNDLED_WIT_DEPS_DIR: &str = "bundled-miden-wit";

/// Rust item name used for the emitted frontend metadata bytes blob.
const FRONTEND_METADATA_BYTES_STATIC_IDENT: &str = "__miden_frontend_metadata_bytes";
/// Linker symbol used to reject multiple frontend-marked procedures in one project.
pub(crate) const FRONTEND_METADATA_UNIQUENESS_GUARD_SYMBOL: &str =
    "__MIDEN_FRONTEND_METADATA_UNIQUENESS_GUARD";

fn target_folder() -> PathBuf {
    let mut manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
    manifest_dir.push_str("/target/");
    PathBuf::from(manifest_dir)
}

pub fn bundled_wit_folder() -> Result<PathBuf, Error> {
    let out_dir = target_folder();
    let wit_deps_dir = out_dir.join(BUNDLED_WIT_DEPS_DIR);
    fs::create_dir_all(&wit_deps_dir).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!(
                "failed to create WIT dependencies directory '{}': {err}",
                wit_deps_dir.display()
            ),
        )
    })?;
    Ok(wit_deps_dir)
}

/// Emits frontend-only metadata into the shared component frontend custom section.
pub(crate) fn generate_frontend_link_section(metadata: &FrontendMetadata) -> TokenStream2 {
    let metadata_bytes = metadata.to_bytes().unwrap_or_else(|err| panic!("{err}"));
    let metadata_len = metadata_bytes.len();
    let encoded_bytes = Literal::byte_string(&metadata_bytes);
    let metadata_static_ident = format_ident!("{}", FRONTEND_METADATA_BYTES_STATIC_IDENT);

    quote! {
        const _: () = {
            // A crate may contain exactly one frontend-marked method. Reusing a fixed symbol name
            // lets the linker reject duplicates across modules or impl blocks.
            #[doc(hidden)]
            #[used]
            #[unsafe(export_name = #FRONTEND_METADATA_UNIQUENESS_GUARD_SYMBOL)]
            static __miden_frontend_metadata_uniqueness_guard: u8 = 0;
        };

        #[unsafe(
            // Keep the Mach-O-friendly `segment,section` naming scheme used by the main metadata
            // section so the linker preserves these bytes in test and release builds.
            link_section = #WASM_FRONTEND_METADATA_CUSTOM_SECTION_NAME
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static #metadata_static_ident: [u8; #metadata_len] = *#encoded_bytes;
    }
}

/// Embeds the component's public WIT source into the dedicated Wasm custom section.
///
/// No linker uniqueness guard is emitted: custom-section bytes never reach executable data, so a
/// guard export would be the only runtime cost of WIT embedding. Linking two component
/// implementations concatenates their identically named sections instead, which the Wasm frontend
/// rejects with a dedicated diagnostic when it parses the section.
pub(crate) fn generate_wit_link_section(wit_source: &str) -> TokenStream2 {
    let wit_bytes = wit_source.as_bytes();
    let wit_len = wit_bytes.len();
    let encoded_bytes = Literal::byte_string(wit_bytes);

    quote! {
        #[unsafe(
            // Keep the Mach-O-friendly `segment,section` naming scheme used by the other metadata
            // sections so the linker preserves these bytes in test and release builds.
            link_section = #WASM_COMPONENT_WIT_CUSTOM_SECTION_NAME
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_COMPONENT_WIT: [u8; #wit_len] = *#encoded_bytes;
    }
}

/// Strips line comments starting with `//` from the provided source line.
///
/// Returns the portion of the line before the comment, or the entire line if no comment exists.
///
/// **Note:** This is a simple heuristic that doesn't account for `//` appearing
/// inside string literals. Only use for WIT source parsing where this is not an issue.
pub fn strip_line_comment(line: &str) -> &str {
    match line.split_once("//") {
        Some((before, _)) => before,
        None => line,
    }
}
