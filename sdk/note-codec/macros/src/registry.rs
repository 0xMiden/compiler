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
    pub(crate) fqn: String,
    pub(crate) rust_type: String,
}

/// Schema types and marked codecs registered by earlier macro expansions.
#[derive(Default)]
struct Registry {
    schema: Option<String>,
    types: BTreeMap<String, String>,
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
    match registry.schema.as_deref() {
        Some(existing) if existing == schema.wit_text() => return Ok(()),
        Some(_) => {
            return Err(syn::Error::new(
                span,
                "miden-note-codec supports one note schema per crate; remove the second distinct \
                 from_project!, from_package!, or from_wit_text! invocation",
            ));
        }
        None => {}
    }
    registry.schema = Some(schema.wit_text().to_owned());
    registry.types = bindings;
    Ok(())
}

/// Resolves and records a marked codec implementation by generated Rust type name.
pub(crate) fn register_codec(rust_name: &str, rust_type: String, span: Span) -> syn::Result<()> {
    let mut registry = registry()
        .lock()
        .map_err(|_| syn::Error::new(span, "note codec registry mutex is poisoned"))?;
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
    if registry.codecs.is_empty() {
        return Err(syn::Error::new(
            span,
            "export_codecs! found no registered codecs; place it after from_project! or \
             from_package! and after every #[note_codec] implementation because procedural macros \
             register in declaration order",
        ));
    }
    Ok(registry.codecs.values().cloned().collect())
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
        *registry.lock().expect("mutex poisoned") = Registry::default();
    }
}
