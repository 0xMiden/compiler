//! Naming rules shared by the macros that derive WIT functions from Rust items.
//!
//! A Rust identifier maps to a canonical WIT name, which is written in explicit (`%`) form in the
//! generated WIT, and which wit-bindgen maps back to the Rust identifier of the generated guest
//! trait method.

use heck::ToKebabCase;
use midenc_frontend_wasm_metadata::namespace::RUST_KEYWORDS;
use syn::ext::IdentExt;

/// Converts a Rust identifier to its canonical WIT spelling before WIT escaping is applied.
///
/// Returns an error at the identifier's span when the derived name is not a valid WIT name.
pub(crate) fn rust_ident_to_wit_name(ident: &syn::Ident) -> syn::Result<String> {
    let wit_name = ident.unraw().to_string().to_kebab_case();
    if wit_bindgen_core::wit_parser::validate_id(&wit_name).is_err() {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "`{ident}` has no valid WIT name (derived `{wit_name}`): WIT names are ASCII \
                 kebab-case words of lowercase letters and digits, the first starting with a \
                 letter; rename it"
            ),
        ));
    }
    Ok(wit_name)
}

/// Returns the Rust identifier wit-bindgen generates for the canonical WIT name `wit_name` of the
/// Rust item `ident` (a guest trait method, parameter or record field).
///
/// `wit_name` must come from [`rust_ident_to_wit_name`]. Returns an error at `ident`'s span when
/// the generated identifier is a Rust keyword wit-bindgen does not escape (e.g. the edition-2024
/// `gen`), since the generated bindings would not compile.
pub(crate) fn wit_bindgen_guest_ident(
    wit_name: &str,
    ident: &syn::Ident,
) -> syn::Result<syn::Ident> {
    let guest_name = wit_bindgen_rust::to_rust_ident(wit_name);
    // wit-bindgen appends `_` to the keywords it knows, so a generated keyword is one it misses.
    if RUST_KEYWORDS.contains(&guest_name.as_str()) {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "`{ident}` would be named `{guest_name}` in the generated bindings, which is a \
                 Rust keyword wit-bindgen does not escape; rename it"
            ),
        ));
    }
    debug_assert!(
        syn::parse_str::<syn::Ident>(&guest_name).is_ok(),
        "`{wit_name}` was validated as a WIT name, so its guest identifier `{guest_name}` is valid"
    );
    Ok(syn::Ident::new(&guest_name, ident.span()))
}

/// Renders WIT's explicit identifier form.
///
/// Explicit identifiers are valid for both keywords and ordinary identifiers. Using this form for
/// every Rust-derived function, parameter, field and case name keeps them valid without checking
/// them against WIT's keyword list; type, package and interface names are rendered bare and are
/// still checked against the keyword list.
pub(crate) fn explicit_wit_identifier(name: &str) -> String {
    format!("%{name}")
}
