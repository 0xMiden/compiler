use std::{
    env, fs,
    path::{Path, PathBuf},
};

use midenc_frontend_wasm_metadata::{
    FrontendMetadata, WASM_COMPONENT_WIT_CUSTOM_SECTION_NAME,
    WASM_FRONTEND_METADATA_CUSTOM_SECTION_NAME, encode_section,
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

/// Returns true if a function's return type is unit.
pub(crate) fn is_unit_return_type(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), syn::Type::Tuple(tuple) if tuple.elems.is_empty())
        }
    }
}

/// Checks if a type's final path segment matches `name` (allowing module-qualified paths like
/// `miden::Word`).
pub(crate) fn is_type_named(ty: &syn::Type, name: &str) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    type_path
        .path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == name && matches!(seg.arguments, syn::PathArguments::None))
}

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
///
/// A component may need several entries (an optional `#[auth_script]` entry plus one entry per
/// `#[account_procedure]` method), so the whole set is encoded as a single section payload.
pub(crate) fn generate_frontend_link_section(entries: &[FrontendMetadata]) -> TokenStream2 {
    let metadata_bytes = encode_section(entries).unwrap_or_else(|err| panic!("{err}"));
    let metadata_len = metadata_bytes.len();
    let encoded_bytes = Literal::byte_string(&metadata_bytes);
    let metadata_static_ident = format_ident!("{}", FRONTEND_METADATA_BYTES_STATIC_IDENT);

    quote! {
        const _: () = {
            // A crate may contain exactly one frontend metadata section. Reusing a fixed symbol
            // name lets the linker reject duplicates across modules or impl blocks.
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
pub(crate) fn generate_wit_link_section(wit_source: &str) -> Result<TokenStream2, Error> {
    let wit_source = normalize_embedded_wit(wit_source);
    // The Wasm frontend rejects a section whose top-level package count is not one; enforce
    // the producing half of that cross-crate contract here, where the offending source can
    // still be named. User-written WIT reaches this point, so this is a diagnostic, not an
    // assertion.
    let package_count = midenc_frontend_wasm_metadata::count_top_level_wit_packages(&wit_source);
    if package_count != 1 {
        return Err(Error::new(
            Span::call_site(),
            format!(
                "embedded component WIT must contain exactly one top-level `package <id>;` \
                 declaration, found {package_count}"
            ),
        ));
    }
    let wit_bytes = wit_source.as_bytes();
    let wit_len = wit_bytes.len();
    let encoded_bytes = Literal::byte_string(wit_bytes);
    // The wrapper module's name is derived from the content, so two WIT emitters in one
    // module — two components, or a note next to a bare `generate!()` — do not collide on a
    // fixed static name with an opaque rustc E0428. Their sections concatenate instead, and
    // the duplicate reaches the Wasm frontend's dedicated diagnostic.
    let module_ident = format_ident!("__miden_component_wit_{:016x}", content_hash(wit_bytes));

    Ok(quote! {
        #[doc(hidden)]
        mod #module_ident {
            #[unsafe(
                // Keep the Mach-O-friendly `segment,section` naming scheme used by the other
                // metadata sections so the linker preserves these bytes in test and release
                // builds.
                link_section = #WASM_COMPONENT_WIT_CUSTOM_SECTION_NAME
            )]
            #[doc(hidden)]
            #[allow(clippy::octal_escapes)]
            #[used]
            pub static __MIDEN_COMPONENT_WIT: [u8; #wit_len] = *#encoded_bytes;
        }
    })
}

/// A stable content fingerprint for naming per-expansion items.
///
/// `DefaultHasher::new()` uses fixed keys, so the name is deterministic across builds.
fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Wraps embedded WIT in newlines so section boundaries stay line boundaries.
///
/// The linker concatenates identically named custom sections byte-wise. The frontend's
/// duplicate-implementation detector (`count_top_level_wit_packages` in
/// `midenc-frontend-wasm-metadata`) scans token-wise and no longer needs the boundary, but the
/// padding keeps glued payloads readable and remains cheap insurance for other consumers.
fn normalize_embedded_wit(wit_source: &str) -> String {
    let mut normalized =
        String::with_capacity(wit_source.len() + 2 - usize::from(wit_source.starts_with('\n')));
    if !wit_source.starts_with('\n') {
        normalized.push('\n');
    }
    normalized.push_str(wit_source);
    if !wit_source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

/// Emits the rebuild-tracking constant for one file a macro expansion consumed.
///
/// Everything here lands in consumer scope, so the macro path is fully qualified: a
/// consumer-defined `include_bytes` must not shadow the rebuild tracking.
pub(crate) fn package_include_tokens(path: &Path) -> Result<TokenStream2, Error> {
    let utf8_path = path.to_str().ok_or_else(|| {
        Error::new(Span::call_site(), format!("path '{}' contains invalid UTF-8", path.display()))
    })?;
    Ok(quote! {
        const _: &[u8] = ::core::include_bytes!(#utf8_path);
    })
}

/// Emits the dep-info record of the package cache location.
///
/// Every midenc-driven build exchanges packages through its own unique directory, so Cargo
/// re-expands the consumer whenever the cache path rotates — the correctness leg of the
/// per-build cache design. The `include_bytes!` constants cover content changes at an
/// unchanged path, which is how consumers of a stable exported directory re-expand. Both the
/// type and the macro are fully qualified because this lands in consumer scope, where a
/// user-defined `Option` or `option_env` must not shadow the tracking.
pub(crate) fn package_cache_tracking_tokens() -> TokenStream2 {
    let env_name = midenc_frontend_wasm_metadata::package_cache::PACKAGE_CACHE_ENV;
    quote! {
        const _: ::core::option::Option<&str> = ::core::option_env!(#env_name);
    }
}

/// Lists the non-directory `.wit` files directly inside `dir`, sorted by name.
pub(crate) fn wit_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut wit_files = fs::read_dir(dir)
        .map_err(|err| {
            Error::new(Span::call_site(), format!("failed to read '{}': {err}", dir.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            Error::new(Span::call_site(), format!("failed to iterate '{}': {err}", dir.display()))
        })?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            !path.is_dir() && path.extension().and_then(|ext| ext.to_str()) == Some("wit")
        })
        .collect::<Vec<_>>();
    wit_files.sort();
    Ok(wit_files)
}

#[cfg(test)]
mod tests {
    use super::normalize_embedded_wit;

    #[test]
    fn embedded_wit_gains_boundary_newlines() {
        assert_eq!(normalize_embedded_wit("package miden:a@0.1.0;"), "\npackage miden:a@0.1.0;\n");
    }

    #[test]
    fn embedded_wit_with_boundary_newlines_is_unchanged() {
        assert_eq!(
            normalize_embedded_wit("\npackage miden:a@0.1.0;\n"),
            "\npackage miden:a@0.1.0;\n"
        );
    }
}
