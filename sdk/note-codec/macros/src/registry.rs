//! Process-global schema and codec registration for one macro expansion process.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
};

use heck::ToUpperCamelCase;
use miden_note_schema::{
    ACCOUNT_ID_FQN, ASSET_AMOUNT_FQN, FELT_FQN, NoteStorageSchema, SchemaCase, SchemaType,
    SchemaTypeKind, WORD_FQN,
};
use proc_macro2::Span;

/// One marked author codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodecRegistration {
    pub(crate) fqn: String,
    pub(crate) rust_type: String,
}

/// Schema types and marked codecs registered by earlier macro expansions.
#[derive(Default)]
struct Registry {
    schemas: BTreeSet<String>,
    types: BTreeMap<String, BTreeSet<String>>,
    codecs: BTreeMap<String, CodecRegistration>,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

/// Returns the shared registry.
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// Records every generated named type, including the storage root.
pub(crate) fn register_schema(schema: &NoteStorageSchema, span: Span) -> syn::Result<()> {
    let mut bindings = BTreeMap::new();
    collect_type_bindings(schema.root(), &mut BTreeSet::new(), &mut bindings)?;

    let mut registry = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    if !registry.schemas.insert(schema.wit_text().to_owned()) {
        return Ok(());
    }
    for (rust_name, fqn) in bindings {
        registry.types.entry(rust_name).or_default().insert(fqn);
    }
    Ok(())
}

/// Resolves and records a marked codec implementation by generated Rust type name.
pub(crate) fn register_codec(rust_name: &str, rust_type: String, span: Span) -> syn::Result<()> {
    let mut registry = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    let fqns = registry.types.get(rust_name).ok_or_else(|| {
        syn::Error::new(
            span,
            format!(
                "type `{rust_name}` is not part of a registered note schema; invoke \
                 miden_note_codec::from_project! or from_package! before #[note_codec]"
            ),
        )
    })?;
    if fqns.len() != 1 {
        return Err(syn::Error::new(
            span,
            format!(
                "generated type `{rust_name}` is ambiguous across note schemas: {}",
                fqns.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    let fqn = fqns.first().expect("one FQN was checked above").clone();
    let registration = CodecRegistration {
        fqn: fqn.clone(),
        rust_type,
    };
    if let Some(existing) = registry.codecs.get(&fqn)
        && existing != &registration
    {
        return Err(syn::Error::new(
            span,
            format!("WIT type `{fqn}` already has a different #[note_codec] implementation"),
        ));
    }
    registry.codecs.insert(fqn, registration);
    Ok(())
}

/// Returns marked codecs in canonical FQN order.
pub(crate) fn registered_codecs(span: Span) -> syn::Result<Vec<CodecRegistration>> {
    let registry = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
    Ok(registry.codecs.values().cloned().collect())
}

/// Collects reachable generated record and variant bindings.
fn collect_type_bindings(
    ty: &SchemaType,
    seen: &mut BTreeSet<String>,
    bindings: &mut BTreeMap<String, String>,
) -> syn::Result<()> {
    if is_protocol_leaf(ty) {
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

/// Returns true for protocol leaves mapped to existing host types.
fn is_protocol_leaf(ty: &SchemaType) -> bool {
    matches!(ty.fqn(), Some(FELT_FQN | WORD_FQN | ACCOUNT_ID_FQN | ASSET_AMOUNT_FQN))
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    if let Some(registry) = REGISTRY.get() {
        *registry.lock().expect("mutex poisoned") = Registry::default();
    }
}
