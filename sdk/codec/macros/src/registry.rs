//! Process-global schema and codec registration for one macro expansion process.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
};

use miden_schema::{SchemaCase, SchemaType, SchemaTypeKind};
use miden_schema_codegen::generated_type_ident;
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
    /// Generated Rust type name to WIT FQN, used by `#[codec]` lookup.
    types: BTreeMap<String, String>,
    /// Marked codecs keyed by the WIT FQN they implement.
    codecs: BTreeMap<String, RegisteredCodec>,
}

/// The registrations of every crate this macro process expanded, keyed by crate.
///
/// Procedural macros register as they expand, so `#[codec]` and `export_codecs!` read what
/// an earlier invocation in the same crate wrote.
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
/// Long-lived macro hosts such as the rust-analyzer proc-macro server expand many crates in one
/// process; the key keeps their registrations apart. One Cargo package builds many crates, such
/// as a library and its integration tests, so the key names the crate as well as the package
/// directory.
fn macro_invocation_crate_key() -> String {
    let package = std::env::var("CARGO_MANIFEST_DIR")
        .or_else(|_| std::env::var("CARGO_PKG_NAME"))
        .unwrap_or_default();
    let crate_name = std::env::var("CARGO_CRATE_NAME").unwrap_or_default();
    format!("{package}\u{1f}{crate_name}")
}

/// Returns the shared registry map keyed by expanding crate.
fn registry() -> &'static Mutex<BTreeMap<String, Registry>> {
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Records every named type generated from `roots` under the schema `wit_text`.
///
/// The roots are the schema root of a note storage schema, or the slot types of an account
/// storage schema; the WIT text identifies the schema.
pub(crate) fn register_schema(
    wit_text: &str,
    roots: &[&SchemaType],
    span: Span,
) -> syn::Result<()> {
    let mut bindings = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        collect_type_bindings(root, &mut seen, &mut bindings)?;
    }
    let bindings = index_by_rust_type_name(&bindings, span)?;

    let mut registries = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    let location = expansion_location(span);
    match &registry.schema {
        Some((existing, _)) if existing == wit_text => return Ok(()),
        Some((_, existing_location)) if *existing_location == location => {
            // A long-lived macro host re-expanded an edited invocation; replace the stale
            // schema and drop the codecs that were registered against it.
            registry.codecs.clear();
        }
        Some(_) => {
            return Err(syn::Error::new(
                span,
                "miden-codec supports one schema per crate; remove the second distinct \
                 from_project!, from_package!, or from_wit_text! invocation. If this error \
                 appears in your IDE after an edit, restart the rust-analyzer proc-macro server",
            ));
        }
        None => {}
    }
    registry.schema = Some((wit_text.to_owned(), location));
    registry.types = bindings;
    Ok(())
}

/// Resolves and records a marked codec implementation by generated Rust type name.
pub(crate) fn register_codec(rust_name: &str, rust_type: String, span: Span) -> syn::Result<()> {
    let mut registries = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    let fqn = registry.types.get(rust_name).ok_or_else(|| {
        syn::Error::new(
            span,
            format!(
                "type `{rust_name}` is not part of a registered schema; invoke \
                 miden_codec::from_project! or from_package! before #[codec]"
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
                "WIT type `{fqn}` already has a different #[codec] implementation. If this error \
                 appears in your IDE after an edit, restart the rust-analyzer proc-macro server"
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
        .map_err(|_| syn::Error::new(span, "codec registry mutex is poisoned"))?;
    let registry = registries.entry(macro_invocation_crate_key()).or_default();
    if registry.codecs.is_empty() {
        return Err(syn::Error::new(
            span,
            "export_codecs! found no registered codecs; place it after from_project! or \
             from_package! and after every #[codec] implementation because procedural macros \
             register as they expand; supported types are exported in canonical FQN order",
        ));
    }
    Ok(registry.codecs.values().map(|codec| codec.registration.clone()).collect())
}

/// Indexes WIT FQNs by the Rust type name the generator gives them.
///
/// Two WIT types of different interfaces may share a local name. The generator rejects that
/// collision, and this index rejects it the same way instead of dropping one binding.
fn index_by_rust_type_name(
    bindings: &BTreeMap<String, String>,
    span: Span,
) -> syn::Result<BTreeMap<String, String>> {
    let mut index = BTreeMap::new();
    for (fqn, rust_name) in bindings {
        if let Some(existing) = index.insert(rust_name.clone(), fqn.clone()) {
            return Err(syn::Error::new(
                span,
                format!("WIT types `{existing}` and `{fqn}` both map to Rust type `{rust_name}`"),
            ));
        }
    }
    Ok(index)
}

/// Collects reachable generated record and variant bindings, keyed by WIT FQN.
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
            syn::Error::new(Span::call_site(), "a generated codec type has no WIT FQN")
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
        bindings.insert(fqn.to_owned(), generated_type_ident(name));
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

/// Clears the process-wide macro registry between unit tests.
///
/// The registry lives for the whole proc-macro process, so a test that expands macros must start
/// from an empty registry. Tests serialize on the registry test lock before they call this.
#[cfg(test)]
pub(crate) fn reset_for_tests() {
    if let Some(registry) = REGISTRY.get() {
        registry.lock().expect("mutex poisoned").clear();
    }
}
