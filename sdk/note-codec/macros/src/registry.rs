//! Process-global schema and codec registration for one macro expansion process.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
};

use heck::ToUpperCamelCase;
use miden_note_schema::{NoteStorageSchema, SchemaCase, SchemaType, SchemaTypeKind};
use proc_macro2::Span;

/// One marked author codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodecRegistration {
    /// Fully qualified WIT name implemented by the codec.
    pub(crate) fqn: String,
    /// Rust type path that implements the codec.
    pub(crate) rust_type: String,
}

/// One marked codec together with the source location that produced it.
#[derive(Clone, Debug)]
struct RegisteredCodec {
    registration: CodecRegistration,
    location: ExpansionLocation,
}

/// Schema types and marked codecs registered by earlier macro expansions.
#[derive(Default)]
struct Registry {
    /// Registered schema source and the expansion that supplied it.
    schema: Option<(String, ExpansionLocation)>,
    /// Generated Rust upper-camel type name to WIT FQN, used by `#[note_codec]` lookup.
    types: BTreeMap<String, String>,
    /// Marked codecs keyed by the WIT FQN they implement.
    codecs: BTreeMap<String, RegisteredCodec>,
}

static REGISTRY: OnceLock<Mutex<BTreeMap<String, Registry>>> = OnceLock::new();

/// Source location of one macro expansion.
///
/// The location tells a stale re-expansion of an edited invocation (same location) from a
/// real conflict between two invocations (different locations).
// Keep this registry identity/replacement policy aligned with sdk/base-macros/src/types.rs;
// changes must land in both.
type ExpansionLocation = (String, usize, usize);

/// Returns the (file, line, column) location of one expansion span.
fn expansion_location(span: Span) -> ExpansionLocation {
    let start = span.start();
    (span.file(), start.line, start.column)
}

/// Returns the key of the crate whose macro expansion is running.
///
/// Long-lived macro hosts such as the rust-analyzer proc-macro server expand many crates in
/// one process; the key keeps their registrations apart.
fn macro_invocation_crate_key() -> String {
    std::env::var("CARGO_MANIFEST_DIR")
        .or_else(|_| std::env::var("CARGO_PKG_NAME"))
        .unwrap_or_default()
}

/// Returns the shared registry map keyed by expanding crate.
fn registry() -> &'static Mutex<BTreeMap<String, Registry>> {
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Records every generated named type, including the storage root.
pub(crate) fn register_schema(schema: &NoteStorageSchema, span: Span) -> syn::Result<()> {
    let mut bindings = BTreeMap::new();
    collect_type_bindings(schema.root(), &mut BTreeSet::new(), &mut bindings)?;

    let mut registries = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    let location = expansion_location(span);
    match &registry.schema {
        Some((existing, _)) if existing == schema.wit_text() => return Ok(()),
        Some((_, existing_location)) if *existing_location == location => {
            // A long-lived macro host re-expanded an edited invocation; replace the stale
            // schema and drop the codecs that were registered against it.
            registry.codecs.clear();
        }
        Some(_) => {
            return Err(syn::Error::new(
                span,
                "miden-note-codec supports one note schema per crate; remove the second distinct \
                 from_project!, from_package!, or from_wit_text! invocation. If this error \
                 appears in your IDE after an edit, restart the rust-analyzer proc-macro server",
            ));
        }
        None => {}
    }
    registry.schema = Some((schema.wit_text().to_owned(), location));
    registry.types = bindings;
    Ok(())
}

/// Resolves and records a marked codec implementation by generated Rust type name.
pub(crate) fn register_codec(rust_name: &str, rust_type: String, span: Span) -> syn::Result<()> {
    let mut registries = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    let fqn = registry.types.get(rust_name).ok_or_else(|| {
        syn::Error::new(
            span,
            format!(
                "type `{rust_name}` is not part of a registered note schema; invoke \
                 miden_note_codec::from_project! or from_package! before #[note_codec]"
            ),
        )
    })?;
    let fqn = fqn.clone();
    let location = expansion_location(span);
    let registration = CodecRegistration {
        fqn: fqn.clone(),
        rust_type,
    };
    if let Some(existing) = registry.codecs.get(&fqn)
        && existing.registration != registration
        // A re-registration from the same source location replaces a stale entry.
        && existing.location != location
    {
        return Err(syn::Error::new(
            span,
            format!(
                "WIT type `{fqn}` already has a different #[note_codec] implementation. If this \
                 error appears in your IDE after an edit, restart the rust-analyzer proc-macro \
                 server"
            ),
        ));
    }
    registry.codecs.insert(
        fqn,
        RegisteredCodec {
            registration,
            location,
        },
    );
    Ok(())
}

/// Returns marked codecs in canonical FQN order.
pub(crate) fn registered_codecs(span: Span) -> syn::Result<Vec<CodecRegistration>> {
    let mut registries = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    if registry.codecs.is_empty() {
        return Err(syn::Error::new(
            span,
            "export_codecs! found no registered codecs; place it after from_project! or \
             from_package! and after every #[note_codec] implementation because procedural macros \
             register in declaration order",
        ));
    }
    Ok(registry.codecs.values().map(|codec| codec.registration.clone()).collect())
}

/// Collects reachable generated record and variant bindings.
fn collect_type_bindings(
    ty: &SchemaType,
    seen: &mut BTreeSet<String>,
    bindings: &mut BTreeMap<String, String>,
) -> syn::Result<()> {
    if ty.standard_leaf().is_some() {
        return Ok(());
    }
    if matches!(ty.kind(), SchemaTypeKind::Record(_) | SchemaTypeKind::Variant(_)) {
        let fqn = ty.fqn().ok_or_else(|| {
            syn::Error::new(Span::call_site(), "a generated note codec type has no WIT FQN")
        })?;
        let name = ty.name().ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                format!("generated WIT type `{fqn}` has no local name"),
            )
        })?;
        if !seen.insert(fqn.to_owned()) {
            return Ok(());
        }
        bindings.insert(name.to_upper_camel_case(), fqn.to_owned());
    }

    match ty.kind() {
        SchemaTypeKind::Record(fields) => {
            for field in fields {
                collect_type_bindings(field.ty(), seen, bindings)?;
            }
        }
        SchemaTypeKind::Option(payload) => collect_type_bindings(payload, seen, bindings)?,
        SchemaTypeKind::Variant(cases) => {
            for payload in cases.iter().filter_map(SchemaCase::payload) {
                collect_type_bindings(payload, seen, bindings)?;
            }
        }
        SchemaTypeKind::Felt | SchemaTypeKind::Primitive(_) => {}
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    if let Some(registry) = REGISTRY.get() {
        registry.lock().expect("mutex poisoned").clear();
    }
}
