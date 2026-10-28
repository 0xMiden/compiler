//! The account storage layer over `miden-schema`: account component storage schemas embedded in
//! Miden packages.
//!
//! An account component declares its storage slots with `#[component_storage]`. Next to the
//! protocol storage metadata, the macro describes the slots in a WIT document, the account storage
//! schema, which the compiler embeds in the component's package. [`AccountStorageSchema`] reads
//! that document and resolves the type of every slot into the `miden-schema` model, so a host can
//! name, build, and decode slot values without the component's source. This crate re-exports
//! `miden-schema` so a host needs one import path.
//!
//! # Schema shape
//!
//! The schema package is unversioned and defines the `account-storage` interface. Its `storage`
//! record has one field per storage slot, in declaration order:
//!
//! - a value slot has the type of its value;
//! - a map slot has the type `list<tuple<K, V>>`, where `K` is the type of its keys and `V` the
//!   type of its values. `list` and `tuple` appear nowhere else in a schema.
//!
//! # Slot names
//!
//! A protocol slot name is `<component namespace>::<leaf>`. The schema carries the leaf only: a
//! field name is the leaf in kebab case, and [`StorageSlot::leaf`] returns it in snake case, as
//! it appears in the slot name. With the `protocol` feature, [`AccountStorageSchema::from_package`]
//! also reads the package's protocol component metadata, checks that its slots and the schema's
//! slots match by leaf and by kind, and records each slot's full name ([`StorageSlot::name`]).
//!
//! # Word layout
//!
//! A slot value, a map key, and a map value are each one word. A type in that position uses the
//! felt layout of `miden-schema` and is at most [`SLOT_WORD_FELTS`] felts wide; its felts come
//! first in the word, and the remaining felts are zero.
//!
//! # Features
//!
//! - `protocol` (default): slot names from the protocol metadata, building
//!   [`InitStorageData`] from text values ([`InitStorageBuilder`]), and decoding slot words. It
//!   enables the protocol features of `miden-schema` and is what makes this crate depend on
//!   `miden-protocol`.
//! - `codec-component`: loading author-supplied codec components with Wasmtime. Implies
//!   `protocol`.
//!
//! Without `protocol` the crate is the schema reader alone, which is what the compiler uses.

#![deny(missing_docs)]

use std::{collections::HashSet, sync::Arc};

pub use miden_schema::*;
use wit_parser::{Field, Resolve, Type, TypeDefKind, TypeId};

#[cfg(feature = "protocol")]
mod protocol;
#[cfg(feature = "protocol")]
pub use miden_protocol::{Word, account::component::InitStorageData};
#[cfg(feature = "protocol")]
pub use protocol::{InitStorageBuilder, felts_from_word, word_from_felts};

#[cfg(test)]
mod tests;

/// The number of felts in a storage word: the width bound of a slot value, a map key, and a map
/// value.
pub const SLOT_WORD_FELTS: usize = 4;

#[cfg(feature = "protocol")]
const _: () = assert!(SLOT_WORD_FELTS == miden_protocol::WORD_SIZE);

/// The limits an account storage schema is held to.
///
/// The budget is the one of note storage schemas. Every slot type is built separately and is at
/// most one word wide, so the felt limit is never the binding one; the byte, type, and node
/// budgets leave room for every slot an account can have.
pub const ACCOUNT_STORAGE_LIMITS: SchemaLimits = SchemaLimits::for_item_count(1024);

/// The schema profile of account storage.
pub const ACCOUNT_STORAGE_PROFILE: SchemaProfile = SchemaProfile {
    name: "account storage",
    interface: "account-storage",
    root_alias: "storage",
    section_id: midenc_frontend_wasm_metadata::PACKAGE_ACCOUNT_STORAGE_SCHEMA_SECTION_ID,
    limits: ACCOUNT_STORAGE_LIMITS,
};

/// The kind of a storage slot and the types of what it holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageSlotKind {
    /// A value slot that holds one value of the type.
    Value(Arc<SchemaType>),
    /// A map slot from keys of one type to values of another.
    Map {
        /// The type of the map keys.
        key: Arc<SchemaType>,
        /// The type of the map values.
        value: Arc<SchemaType>,
    },
}

/// One storage slot of an account storage schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageSlot {
    leaf: String,
    name: Option<String>,
    docs: Option<String>,
    kind: StorageSlotKind,
}

impl StorageSlot {
    /// Returns the slot's leaf name in snake case, the last segment of its protocol slot name.
    pub fn leaf(&self) -> &str {
        &self.leaf
    }

    /// Returns the slot's full protocol slot name, `<component namespace>::<leaf>`.
    ///
    /// The name comes from the package's protocol metadata, so it is `None` for a schema read
    /// from WIT text alone or without the `protocol` feature.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the slot's WIT documentation.
    pub fn docs(&self) -> Option<&str> {
        self.docs.as_deref()
    }

    /// Returns the slot's kind and types.
    pub const fn kind(&self) -> &StorageSlotKind {
        &self.kind
    }

    /// Returns the types in the slot: the value type, or the key and value types of a map.
    fn types(&self) -> impl Iterator<Item = &SchemaType> {
        let (first, second) = match &self.kind {
            StorageSlotKind::Value(value) => (value, None),
            StorageSlotKind::Map { key, value } => (key, Some(value)),
        };
        core::iter::once(first.as_ref()).chain(second.map(AsRef::as_ref))
    }
}

/// A resolved account storage schema, with the standard codec registry when the `protocol`
/// feature is enabled.
#[derive(Clone)]
pub struct AccountStorageSchema {
    wit_text: String,
    slots: Vec<StorageSlot>,
    #[cfg(feature = "protocol")]
    codecs: CodecRegistry,
}

impl core::fmt::Debug for AccountStorageSchema {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AccountStorageSchema")
            .field("wit_text", &self.wit_text)
            .field("slots", &self.slots)
            .finish_non_exhaustive()
    }
}

impl AccountStorageSchema {
    /// Reads and resolves the account storage schema section from a Miden package.
    ///
    /// With the `protocol` feature, the package's protocol component metadata is read as well:
    /// every schema slot must match a protocol slot by leaf and by kind (value or map), every
    /// protocol slot must have a schema slot, and each slot gets its full protocol name.
    pub fn from_package(package: &miden_mast_package::Package) -> Result<Self> {
        let schema = Self::from_wit_text(read_schema_section(package, &ACCOUNT_STORAGE_PROFILE)?)?;
        #[cfg(feature = "protocol")]
        let schema = schema.with_protocol_metadata(&protocol::read_component_metadata(package)?)?;
        Ok(schema)
    }

    /// Resolves an account storage schema from a WIT document.
    pub fn from_wit_text(wit_text: &str) -> Result<Self> {
        let profile = &ACCOUNT_STORAGE_PROFILE;
        let resolved = resolve_schema_package(wit_text, profile)?;
        let resolve = &resolved.resolve;
        let root_id = follow_aliases(resolve, resolved.root_type_id)?;
        let TypeDefKind::Record(record) = &resolve.types[root_id].kind else {
            return Err(Error::new(format!(
                "the `{}.{}` type must resolve to a record, found {}",
                profile.interface,
                profile.root_alias,
                resolve.types[root_id].kind.as_str()
            )));
        };
        // wit-parser accepts an empty record; `#[component_storage]` emits no schema without slots.
        if record.fields.is_empty() {
            return Err(Error::new(format!(
                "the `{}.{}` record has no fields; an account storage schema declares at least \
                 one slot",
                profile.interface, profile.root_alias
            )));
        }
        let slots = record
            .fields
            .iter()
            .map(|field| resolve_slot(resolve, field))
            .collect::<Result<Vec<_>>>()?;
        let schema = Self {
            wit_text: wit_text.trim_end_matches('\0').to_owned(),
            slots,
            #[cfg(feature = "protocol")]
            codecs: CodecRegistry::default(),
        };
        schema.validate_native_leaf_shapes()?;
        Ok(schema)
    }

    /// Returns the unpadded WIT document.
    pub fn wit_text(&self) -> &str {
        &self.wit_text
    }

    /// Returns the storage slots in declaration order.
    pub fn slots(&self) -> &[StorageSlot] {
        &self.slots
    }

    /// Returns the slot with the snake-case `leaf` name.
    pub fn slot(&self, leaf: &str) -> Option<&StorageSlot> {
        self.slots.iter().find(|slot| slot.leaf == leaf)
    }

    /// Returns all custom named types in the slot types.
    ///
    /// A custom type is a named type that is not a [`StandardLeaf`]; these are the types an
    /// author codec may handle.
    pub fn custom_type_fqns(&self) -> HashSet<String> {
        self.slots
            .iter()
            .flat_map(StorageSlot::types)
            .flat_map(SchemaType::custom_type_fqns)
            .collect()
    }

    /// Verifies native host mappings in every slot type against the pinned standard type shapes.
    pub fn validate_native_leaf_shapes(&self) -> Result<()> {
        self.slots
            .iter()
            .flat_map(StorageSlot::types)
            .try_for_each(SchemaType::validate_native_leaf_shapes)
    }

    /// Returns the slot with a snake-case or kebab-case `leaf` name, or an error naming it.
    #[cfg(feature = "protocol")]
    fn find_slot(&self, leaf: &str) -> Result<&StorageSlot> {
        let normalized = leaf.trim().replace('-', "_");
        self.slot(&normalized).ok_or_else(|| {
            Error::new(format!("the account storage schema has no slot named `{normalized}`"))
        })
    }
}

impl CustomTypes for AccountStorageSchema {
    fn custom_type_fqns(&self) -> HashSet<String> {
        AccountStorageSchema::custom_type_fqns(self)
    }
}

/// Resolves one root record field into a storage slot.
fn resolve_slot(resolve: &Resolve, field: &Field) -> Result<StorageSlot> {
    // wit-parser already strips the `%` escape from a field name.
    let leaf = field.name.replace('-', "_");
    let kind = match map_entry_types(resolve, field.ty, &leaf)? {
        Some((key, value)) => StorageSlotKind::Map {
            key: slot_word_type(resolve, key, &leaf, "map key")?,
            value: slot_word_type(resolve, value, &leaf, "map value")?,
        },
        None => StorageSlotKind::Value(slot_word_type(resolve, field.ty, &leaf, "value")?),
    };
    Ok(StorageSlot {
        leaf,
        name: None,
        docs: field.docs.contents.clone(),
        kind,
    })
}

/// Returns the key and value types of a map slot's `list<tuple<K, V>>`, or `None` when the field
/// is not a list and so is a value slot.
fn map_entry_types(resolve: &Resolve, ty: Type, leaf: &str) -> Result<Option<(Type, Type)>> {
    let Type::Id(id) = ty else {
        return Ok(None);
    };
    let TypeDefKind::List(element) = resolve.types[follow_aliases(resolve, id)?].kind else {
        return Ok(None);
    };
    let tuple = match element {
        Type::Id(element) => match &resolve.types[follow_aliases(resolve, element)?].kind {
            TypeDefKind::Tuple(tuple) => Some(tuple),
            _ => None,
        },
        _ => None,
    };
    let Some(tuple) = tuple else {
        return Err(Error::new(format!(
            "account storage slot `{leaf}` is a list, but not a `list<tuple<K, V>>` map"
        )));
    };
    match tuple.types.as_slice() {
        [key, value] => Ok(Some((*key, *value))),
        types => Err(Error::new(format!(
            "account storage map slot `{leaf}` has a tuple of {} types; a map entry is a \
             `tuple<K, V>` of a key and a value",
            types.len()
        ))),
    }
}

/// Builds the type of one word in a slot and checks that it fits the word.
fn slot_word_type(resolve: &Resolve, ty: Type, leaf: &str, role: &str) -> Result<Arc<SchemaType>> {
    let ty = build_model_type(resolve, ty, &ACCOUNT_STORAGE_LIMITS).map_err(|error| {
        Error::new(format!("the {role} type of account storage slot `{leaf}` is invalid: {error}"))
    })?;
    let width = ty.layout().maximum();
    if width > SLOT_WORD_FELTS {
        let name = ty.fqn().or(ty.name()).unwrap_or("<anonymous>");
        return Err(Error::new(format!(
            "the {role} type `{name}` of account storage slot `{leaf}` is up to {width} felts \
             wide; a slot word holds at most {SLOT_WORD_FELTS}"
        )));
    }
    Ok(ty)
}

/// Follows `type = id` aliases to their defining type.
fn follow_aliases(resolve: &Resolve, mut id: TypeId) -> Result<TypeId> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(id) {
            return Err(Error::new("cyclic WIT type aliases are not supported"));
        }
        match resolve.types[id].kind {
            TypeDefKind::Type(Type::Id(next)) => id = next,
            _ => return Ok(id),
        }
    }
}
