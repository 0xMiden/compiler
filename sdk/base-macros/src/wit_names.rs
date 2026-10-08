//! Naming rules shared by the macros that derive WIT functions from Rust items.
//!
//! A Rust identifier maps to a canonical WIT name, which is written in explicit (`%`) form in the
//! generated WIT, and which wit-bindgen maps back to the Rust identifier of the generated guest
//! trait method. The module also derives the Rust identifier wit-bindgen generates for any WIT
//! name, so code referring to a generated binding spells it the same way, and rejects items whose
//! WIT names collide: two sibling functions or parameters, or a function and a type of the same
//! generated interface.

use std::collections::BTreeSet;

use heck::ToKebabCase;
use midenc_frontend_wasm_metadata::namespace::RUST_KEYWORDS;
use proc_macro2::Span;
use syn::ext::IdentExt;

/// Converts a Rust identifier to its canonical WIT spelling before WIT escaping is applied.
///
/// A raw identifier loses its `r#`, so `r#type` maps to `type`.
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

/// Returns the Rust identifier wit-bindgen generates for the WIT name `wit_name`, at `span`.
///
/// Code that refers to a generated binding must spell it exactly as wit-bindgen does, so it
/// derives the identifier from the WIT name rather than from a Rust identifier the name came from.
pub(crate) fn wit_bindgen_rust_ident(wit_name: &str, span: Span) -> syn::Ident {
    syn::Ident::new(&wit_bindgen_rust_name(wit_name), span)
}

/// Returns the spelling of the Rust identifier wit-bindgen generates for the WIT name `wit_name`,
/// for callers that compare or key by the name rather than emit it.
pub(crate) fn wit_bindgen_rust_name(wit_name: &str) -> String {
    wit_bindgen_rust::to_rust_ident(wit_name)
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
    let guest_name = wit_bindgen_rust_name(wit_name);
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

/// Rejects the first exported function whose WIT name is also the name of a type of the same
/// generated WIT interface, `type_names`.
///
/// Each function is given as `(item kind, Rust identifier, WIT name)`, e.g.
/// `("component method", ident, "get-count")`. WIT interfaces share one namespace between types
/// and functions, so a collision would otherwise surface as a WIT parse error inside the
/// generated bindings; the error points at the Rust identifier instead.
pub(crate) fn reject_function_type_name_collisions<'a>(
    functions: impl IntoIterator<Item = (&'a str, &'a syn::Ident, &'a str)>,
    type_names: impl IntoIterator<Item = &'a String>,
) -> syn::Result<()> {
    let type_names = type_names.into_iter().map(String::as_str).collect::<BTreeSet<_>>();
    match functions.into_iter().find(|(_, _, wit_name)| type_names.contains(wit_name)) {
        Some((item_kind, ident, wit_name)) => Err(syn::Error::new(
            ident.span(),
            format!(
                "{item_kind} `{ident}` produces the WIT name `{wit_name}`, which collides with \
                 the type `{wit_name}` of the generated WIT interface; rename it"
            ),
        )),
        None => Ok(()),
    }
}

/// Rejects the item `ident` when its WIT name `wit_name` is already used by one of the sibling
/// items declared before it, `previous`.
///
/// Items are given as `(item kind, Rust identifier, WIT name)`, e.g.
/// `("component method", ident, "get-count")`; siblings are the functions of one generated WIT
/// interface or the parameters of one function. Distinct Rust identifiers can normalize to one
/// WIT name (`foo_bar`, `fooBar`), which would otherwise surface as a WIT parse error inside the
/// generated bindings; the error points at both Rust declarations instead.
pub(crate) fn reject_duplicate_wit_name<'a>(
    item_kind: &str,
    ident: &syn::Ident,
    wit_name: &str,
    previous: impl IntoIterator<Item = (&'a str, &'a syn::Ident, &'a str)>,
) -> syn::Result<()> {
    let Some((previous_kind, previous, _)) =
        previous.into_iter().find(|(_, _, previous_name)| *previous_name == wit_name)
    else {
        return Ok(());
    };
    let mut error = syn::Error::new(
        ident.span(),
        format!(
            "{item_kind} `{ident}` produces the WIT name `{wit_name}`, which is already used by \
             {previous_kind} `{previous}`; rename one of them"
        ),
    );
    error.combine(syn::Error::new(
        previous.span(),
        format!("{previous_kind} `{previous}` first uses the WIT name `{wit_name}`"),
    ));
    Err(error)
}

/// Renders WIT's explicit identifier form.
///
/// Explicit identifiers are valid for both keywords and ordinary identifiers. Using this form for
/// every Rust-derived function, parameter, field and case name, and for the stored-procedure
/// interface named after the storage struct, keeps them valid without checking them against
/// WIT's keyword list. Type names, and the package and interface names derived from the
/// namespace, are rendered bare and are checked against the keyword list instead.
pub(crate) fn explicit_wit_identifier(name: &str) -> String {
    format!("%{name}")
}
