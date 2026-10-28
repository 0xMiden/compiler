//! The protocol side of account storage schemas: full slot names from the component metadata,
//! init storage data built from text values, and slot words decoded into named values.

use std::collections::{BTreeMap, HashSet};

use miden_mast_package::{Package, SectionId};
use miden_protocol::{
    Word,
    account::{
        StorageSlotName,
        component::{
            AccountComponentMetadata, InitStorageData, StorageSlotSchema, StorageValueName,
        },
    },
    utils::serde::Deserializable,
};

use crate::{
    ACCOUNT_STORAGE_PROFILE, AccountStorageSchema, CodecRegistry, DecodedValue, Error, Felt,
    Result, SLOT_WORD_FELTS, SchemaType, SchemaTypeKind, StorageSlot, StorageSlotKind,
    ValueBuilder, decode_felts, encode_text_value,
};

/// Packs the felts of one slot value, map key, or map value into a word: the felts first, zero
/// padding after.
pub fn word_from_felts(felts: &[Felt]) -> Result<Word> {
    if felts.len() > SLOT_WORD_FELTS {
        return Err(Error::new(format!(
            "a storage word holds at most {SLOT_WORD_FELTS} felts, found {}",
            felts.len()
        )));
    }
    let mut elements = [Felt::ZERO; SLOT_WORD_FELTS];
    elements[..felts.len()].copy_from_slice(felts);
    Ok(Word::new(elements))
}

/// Returns the felts of a storage word in slot layout order.
pub fn felts_from_word(word: Word) -> [Felt; SLOT_WORD_FELTS] {
    word.into_elements()
}

impl AccountStorageSchema {
    /// Resolves an account storage schema from a WIT document and names its slots after the
    /// component's protocol metadata.
    ///
    /// This is the check [`AccountStorageSchema::from_package`] applies to a package: every
    /// schema slot must match a protocol slot by leaf and by kind (value or map), and every
    /// protocol slot must have a schema slot.
    pub fn from_wit_text_and_metadata(
        wit_text: &str,
        metadata: &AccountComponentMetadata,
    ) -> Result<Self> {
        Self::from_wit_text(wit_text)?.with_protocol_metadata(metadata)
    }

    /// Names every slot after the protocol slot with the same leaf and checks that both
    /// descriptions declare the same slots.
    pub(crate) fn with_protocol_metadata(
        mut self,
        metadata: &AccountComponentMetadata,
    ) -> Result<Self> {
        let protocol_slots = metadata.storage_schema();
        let mut matched = HashSet::new();
        for slot in &mut self.slots {
            let mut candidates =
                protocol_slots.iter().filter(|(name, _)| name_leaf(name.as_str()) == slot.leaf);
            let (name, protocol_slot) = candidates.next().ok_or_else(|| {
                Error::new(format!(
                    "account storage slot `{}` has no slot named `<namespace>::{}` in the \
                     component's protocol metadata",
                    slot.leaf, slot.leaf
                ))
            })?;
            if let Some((other, _)) = candidates.next() {
                return Err(Error::new(format!(
                    "account storage slot `{}` matches both `{name}` and `{other}` in the \
                     component's protocol metadata",
                    slot.leaf
                )));
            }
            let protocol_is_map = matches!(protocol_slot, StorageSlotSchema::Map(_));
            let schema_is_map = matches!(slot.kind, StorageSlotKind::Map { .. });
            if protocol_is_map != schema_is_map {
                return Err(Error::new(format!(
                    "account storage slot `{}` is a {} slot, but the protocol metadata declares \
                     `{name}` as a {} slot",
                    slot.leaf,
                    kind_name(schema_is_map),
                    kind_name(protocol_is_map)
                )));
            }
            matched.insert(name.as_str());
            slot.name = Some(name.as_str().to_owned());
        }
        if let Some((name, _)) =
            protocol_slots.iter().find(|(name, _)| !matched.contains(name.as_str()))
        {
            return Err(Error::new(format!(
                "protocol slot `{name}` has no slot in the account storage schema"
            )));
        }
        Ok(self)
    }

    /// Returns the schema's codec registry, the standard codecs unless replaced.
    pub const fn codecs(&self) -> &CodecRegistry {
        &self.codecs
    }

    /// Replaces the codec registry used by `init_builder` and the decoders.
    pub fn with_codec_registry(mut self, codecs: CodecRegistry) -> Self {
        self.codecs = codecs;
        self
    }

    /// Creates an init storage builder with the schema's codec registry.
    pub fn init_builder(&self) -> InitStorageBuilder<'_> {
        self.init_builder_with_registry(&self.codecs)
    }

    /// Creates an init storage builder with a caller-provided codec registry.
    pub fn init_builder_with_registry<'a>(
        &'a self,
        registry: &'a CodecRegistry,
    ) -> InitStorageBuilder<'a> {
        InitStorageBuilder {
            schema: self,
            registry,
            paths: BTreeMap::new(),
            words: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }

    /// Decodes the word of the value slot `leaf` with the schema's codec registry.
    ///
    /// The felts after the value must be zero.
    pub fn decode_value(&self, leaf: &str, word: Word) -> Result<DecodedValue> {
        self.decode_value_with_registry(leaf, word, &self.codecs)
    }

    /// Decodes the word of the value slot `leaf` with a caller-provided codec registry.
    ///
    /// The felts after the value must be zero.
    pub fn decode_value_with_registry(
        &self,
        leaf: &str,
        word: Word,
        registry: &CodecRegistry,
    ) -> Result<DecodedValue> {
        let slot = self.find_slot(leaf)?;
        let ty = value_type(slot)?;
        decode_word(ty, word, registry)
            .map_err(|err| err.context(format!("account storage slot `{}`", slot.leaf)))
    }

    /// Decodes one entry of the map slot `leaf` with the schema's codec registry.
    ///
    /// Returns the key and the value. The felts after each of them must be zero.
    pub fn decode_map_entry(
        &self,
        leaf: &str,
        key: Word,
        value: Word,
    ) -> Result<(DecodedValue, DecodedValue)> {
        self.decode_map_entry_with_registry(leaf, key, value, &self.codecs)
    }

    /// Decodes one entry of the map slot `leaf` with a caller-provided codec registry.
    ///
    /// Returns the key and the value. The felts after each of them must be zero.
    pub fn decode_map_entry_with_registry(
        &self,
        leaf: &str,
        key: Word,
        value: Word,
        registry: &CodecRegistry,
    ) -> Result<(DecodedValue, DecodedValue)> {
        let slot = self.find_slot(leaf)?;
        let (key_ty, value_ty) = map_types(slot)?;
        let key = decode_word(key_ty, key, registry).map_err(|err| {
            err.context(format!("map key of account storage slot `{}`", slot.leaf))
        })?;
        let value = decode_word(value_ty, value, registry).map_err(|err| {
            err.context(format!("map value of account storage slot `{}`", slot.leaf))
        })?;
        Ok((key, value))
    }
}

/// Builds the [`InitStorageData`] of an account component from text values.
///
/// A value slot is set by its leaf name, as one text, or field by field through dotted paths
/// (`<leaf>.<field>...`) with the path rules of [`ValueBuilder::set`]. A map slot gets its
/// entries one by one. Every value slot needs a value; a map slot may stay empty.
pub struct InitStorageBuilder<'a> {
    schema: &'a AccountStorageSchema,
    registry: &'a CodecRegistry,
    /// Value slots set field by field, by leaf.
    paths: BTreeMap<String, ValueBuilder<'a>>,
    /// Value slots set as one value, by leaf.
    words: BTreeMap<String, Word>,
    /// Map slot entries in insertion order, by leaf.
    entries: BTreeMap<String, Vec<(Word, Word)>>,
}

impl InitStorageBuilder<'_> {
    /// Sets a value slot, or one field of it, by kebab-case or snake_case path.
    ///
    /// `path` is the slot's leaf name to set the whole value, or `<leaf>.<field>...` to set one
    /// field of a record value. A value accepts the syntax of [`ValueBuilder::set`]; a record set
    /// as a whole needs a codec.
    pub fn set(mut self, path: &str, value: impl AsRef<str>) -> Result<Self> {
        let schema = self.schema;
        let (leaf, fields) = match path.split_once('.') {
            Some((leaf, fields)) => (leaf, Some(fields)),
            None => (path, None),
        };
        let slot = schema.find_slot(leaf)?;
        let ty = value_type(slot)?;
        let context = |err: Error| err.context(format!("account storage slot `{}`", slot.leaf));
        if self.words.contains_key(&slot.leaf)
            || (fields.is_none() && self.paths.contains_key(&slot.leaf))
        {
            return Err(Error::new(format!(
                "account storage slot `{}` already has a value",
                slot.leaf
            )));
        }
        match fields {
            None => {
                let felts =
                    encode_text_value(ty, value.as_ref(), self.registry).map_err(context)?;
                self.words.insert(slot.leaf.clone(), word_from_felts(&felts)?);
            }
            Some(fields) => {
                let builder = self.paths.remove(&slot.leaf).unwrap_or_else(|| {
                    ValueBuilder::for_type(ty, ACCOUNT_STORAGE_PROFILE.name, self.registry)
                });
                let builder = builder.set(fields, value).map_err(context)?;
                self.paths.insert(slot.leaf.clone(), builder);
            }
        }
        Ok(self)
    }

    /// Sets a value slot to a word encoded elsewhere, such as by typed bindings.
    ///
    /// The word must decode as the slot type with the builder's codec registry.
    pub fn set_word(mut self, leaf: &str, word: Word) -> Result<Self> {
        let slot = self.schema.find_slot(leaf)?;
        self.schema.decode_value_with_registry(&slot.leaf, word, self.registry)?;
        if self.words.contains_key(&slot.leaf) || self.paths.contains_key(&slot.leaf) {
            return Err(Error::new(format!(
                "account storage slot `{}` already has a value",
                slot.leaf
            )));
        }
        self.words.insert(slot.leaf.clone(), word);
        Ok(self)
    }

    /// Adds one entry to a map slot from the texts of its key and value.
    ///
    /// Each text accepts the syntax of [`ValueBuilder::set`]. A record key or value has no text
    /// form without a codec; encode it with the typed bindings and use
    /// [`InitStorageBuilder::set_entry_words`].
    pub fn set_entry(mut self, leaf: &str, key: &str, value: &str) -> Result<Self> {
        let slot = self.schema.find_slot(leaf)?;
        let (key_ty, value_ty) = map_types(slot)?;
        let key = self.encode_entry_word(slot, key_ty, "key", key)?;
        let value = self.encode_entry_word(slot, value_ty, "value", value)?;
        self.entries.entry(slot.leaf.clone()).or_default().push((key, value));
        Ok(self)
    }

    /// Adds one entry to a map slot from words encoded elsewhere, such as by typed bindings.
    ///
    /// The key and the value must decode as the slot's types with the builder's codec registry.
    pub fn set_entry_words(mut self, leaf: &str, key: Word, value: Word) -> Result<Self> {
        let slot = self.schema.find_slot(leaf)?;
        self.schema
            .decode_map_entry_with_registry(&slot.leaf, key, value, self.registry)?;
        self.entries.entry(slot.leaf.clone()).or_default().push((key, value));
        Ok(self)
    }

    /// Checks that every value slot has a value and returns the init storage data.
    ///
    /// Every slot must have its full protocol name, so the schema must come from
    /// [`AccountStorageSchema::from_package`] or
    /// [`AccountStorageSchema::from_wit_text_and_metadata`]. A map key given twice is an error.
    pub fn build(mut self) -> Result<InitStorageData> {
        let mut data = InitStorageData::default();
        for slot in &self.schema.slots {
            let name = slot.name.as_deref().ok_or_else(|| {
                Error::new(format!(
                    "account storage slot `{}` has no protocol slot name; read the schema with \
                     `AccountStorageSchema::from_package`",
                    slot.leaf
                ))
            })?;
            let slot_name = StorageSlotName::new(name)
                .map_err(|err| Error::new(format!("invalid protocol slot name `{name}`: {err}")))?;
            let init_error = |err: miden_protocol::account::component::InitStorageDataError| {
                Error::new(format!("failed to add account storage slot `{name}`: {err}"))
            };
            match &slot.kind {
                StorageSlotKind::Value(_) => {
                    let word = match self.words.remove(&slot.leaf) {
                        Some(word) => word,
                        None => {
                            let builder = self.paths.remove(&slot.leaf).ok_or_else(|| {
                                Error::new(format!(
                                    "missing a value for account storage slot `{}`",
                                    slot.leaf
                                ))
                            })?;
                            let felts = builder.build().map_err(|err| {
                                err.context(format!("account storage slot `{}`", slot.leaf))
                            })?;
                            word_from_felts(&felts)?
                        }
                    };
                    data.insert_value(StorageValueName::from_slot_name(&slot_name), word)
                        .map_err(init_error)?;
                }
                StorageSlotKind::Map { .. } => {
                    for (key, value) in self.entries.remove(&slot.leaf).unwrap_or_default() {
                        data.insert_map_entry(slot_name.clone(), key, value).map_err(init_error)?;
                    }
                }
            }
        }
        Ok(data)
    }

    /// Encodes the text of a map key or value into its word.
    fn encode_entry_word(
        &self,
        slot: &StorageSlot,
        ty: &SchemaType,
        role: &str,
        text: &str,
    ) -> Result<Word> {
        if matches!(ty.kind(), SchemaTypeKind::Record(_))
            && !ty.fqn().is_some_and(|fqn| self.registry.contains(fqn))
        {
            return Err(Error::new(format!(
                "the map {role} type `{}` of account storage slot `{}` is a record without a \
                 codec, so it has no text form; encode the entry with the typed bindings and use \
                 `set_entry_words`",
                ty.fqn().or(ty.name()).unwrap_or("<anonymous record>"),
                slot.leaf
            )));
        }
        let felts = encode_text_value(ty, text, self.registry).map_err(|err| {
            err.context(format!("map {role} of account storage slot `{}`", slot.leaf))
        })?;
        word_from_felts(&felts)
    }
}

/// Reads the protocol component metadata section of a package.
pub(crate) fn read_component_metadata(package: &Package) -> Result<AccountComponentMetadata> {
    let mut sections = package
        .sections
        .iter()
        .filter(|section| section.id == SectionId::ACCOUNT_COMPONENT_METADATA);
    let section = sections.next().ok_or_else(|| {
        Error::new(format!(
            "the package has an account storage schema but no `{}` section",
            SectionId::ACCOUNT_COMPONENT_METADATA
        ))
    })?;
    if sections.next().is_some() {
        return Err(Error::new(format!(
            "the package has more than one `{}` section",
            SectionId::ACCOUNT_COMPONENT_METADATA
        )));
    }
    AccountComponentMetadata::read_from_bytes(&section.data).map_err(|err| {
        Error::new(format!("failed to decode the account component metadata: {err}"))
    })
}

/// Decodes one slot word and checks that the felts after the value are zero.
fn decode_word(ty: &SchemaType, word: Word, registry: &CodecRegistry) -> Result<DecodedValue> {
    let felts = felts_from_word(word);
    let (value, consumed) = decode_felts(ty, &felts, registry)?;
    if let Some(index) = felts[consumed..].iter().position(|felt| *felt != Felt::ZERO) {
        return Err(Error::new(format!(
            "the word has a nonzero padding felt at index {}",
            consumed + index
        )));
    }
    Ok(value)
}

/// Returns the type of a value slot, or an error for a map slot.
fn value_type(slot: &StorageSlot) -> Result<&SchemaType> {
    match &slot.kind {
        StorageSlotKind::Value(ty) => Ok(ty),
        StorageSlotKind::Map { .. } => Err(Error::new(format!(
            "account storage slot `{}` is a map slot, not a value slot",
            slot.leaf
        ))),
    }
}

/// Returns the key and value types of a map slot, or an error for a value slot.
fn map_types(slot: &StorageSlot) -> Result<(&SchemaType, &SchemaType)> {
    match &slot.kind {
        StorageSlotKind::Map { key, value } => Ok((key, value)),
        StorageSlotKind::Value(_) => Err(Error::new(format!(
            "account storage slot `{}` is a value slot, not a map slot",
            slot.leaf
        ))),
    }
}

/// Returns the last `::` segment of a protocol slot name.
fn name_leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// Names a slot kind in diagnostics.
fn kind_name(is_map: bool) -> &'static str {
    if is_map { "map" } else { "value" }
}
