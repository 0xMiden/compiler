//! Resolved note storage schema model.

use std::collections::HashSet;

use miden_mast_package::{Package, SectionId};
use midenc_frontend_wasm_metadata::PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID;
use wit_parser::{Resolve, Type, TypeDefKind, TypeId, TypeOwner};

use crate::{
    CodecRegistry, DecodedValue, Error, NoteStorage, NoteStorageBuilder, Result, codec::FELT_FQN,
};

/// The minimum and maximum felt count for a schema type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeltLayout {
    minimum: usize,
    maximum: usize,
}

impl FeltLayout {
    /// Returns the minimum number of felts accepted by this layout.
    pub const fn minimum(self) -> usize {
        self.minimum
    }

    /// Returns the maximum number of felts accepted by this layout.
    pub const fn maximum(self) -> usize {
        self.maximum
    }

    /// Returns the fixed width, or `None` for a variable-width layout.
    pub const fn fixed_width(self) -> Option<usize> {
        if self.minimum == self.maximum {
            Some(self.minimum)
        } else {
            None
        }
    }

    /// Creates a fixed-width layout.
    const fn fixed(width: usize) -> Self {
        Self {
            minimum: width,
            maximum: width,
        }
    }

    /// Adds two layouts in declaration order.
    fn concatenate(self, other: Self) -> Result<Self> {
        let minimum = self
            .minimum
            .checked_add(other.minimum)
            .ok_or_else(|| Error::new("note storage layout minimum width is too large"))?;
        let maximum = self
            .maximum
            .checked_add(other.maximum)
            .ok_or_else(|| Error::new("note storage layout maximum width is too large"))?;
        Ok(Self { minimum, maximum })
    }
}

/// A supported primitive WIT type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveType {
    /// An unsigned 64-bit integer stored as low and high `u32` limbs.
    U64,
    /// An unsigned 32-bit integer stored in one felt.
    U32,
    /// An unsigned 8-bit integer stored in one felt.
    U8,
    /// A boolean stored as zero or one.
    Bool,
}

/// The structural kind of a resolved schema type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaTypeKind {
    /// The one-felt `miden:base/core-types.felt` bedrock type.
    Felt,
    /// A supported WIT primitive.
    Primitive(PrimitiveType),
    /// A record with fields in declaration order.
    Record(Vec<SchemaField>),
    /// An optional payload stored after a tag felt.
    Option(Box<SchemaType>),
    /// A variant with declaration-ordinal cases.
    Variant(Vec<SchemaCase>),
}

/// A resolved WIT type used by note storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaType {
    name: Option<String>,
    fqn: Option<String>,
    docs: Option<String>,
    kind: SchemaTypeKind,
    layout: FeltLayout,
}

impl SchemaType {
    /// Returns the WIT type name when this is a named type.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the canonical fully-qualified WIT type name.
    pub fn fqn(&self) -> Option<&str> {
        self.fqn.as_deref()
    }

    /// Returns the resolved WIT documentation.
    pub fn docs(&self) -> Option<&str> {
        self.docs.as_deref()
    }

    /// Returns the structural type kind.
    pub const fn kind(&self) -> &SchemaTypeKind {
        &self.kind
    }

    /// Returns the felt layout for this type.
    pub const fn layout(&self) -> FeltLayout {
        self.layout
    }
}

/// A named record field in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaField {
    name: String,
    docs: Option<String>,
    ty: SchemaType,
}

impl SchemaField {
    /// Returns the field's kebab-case WIT name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the field-level WIT documentation.
    pub fn docs(&self) -> Option<&str> {
        self.docs.as_deref()
    }

    /// Returns the field type.
    pub const fn ty(&self) -> &SchemaType {
        &self.ty
    }
}

/// A WIT variant case in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaCase {
    name: String,
    docs: Option<String>,
    payload: Option<SchemaType>,
}

impl SchemaCase {
    /// Returns the case's kebab-case WIT name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the case documentation.
    pub fn docs(&self) -> Option<&str> {
        self.docs.as_deref()
    }

    /// Returns the optional case payload.
    pub const fn payload(&self) -> Option<&SchemaType> {
        self.payload.as_ref()
    }
}

/// A resolved note storage schema with the standard codec registry.
#[derive(Clone)]
pub struct NoteStorageSchema {
    wit_text: String,
    root: SchemaType,
    codecs: CodecRegistry,
}

impl NoteStorageSchema {
    /// Reads and resolves the note storage schema section from a Miden package.
    pub fn from_package(package: &Package) -> Result<Self> {
        let section_id =
            SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).map_err(|err| {
                Error::new(format!(
                    "invalid note storage schema section id \
                     `{PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID}`: {err}"
                ))
            })?;
        let bytes = package
            .sections
            .iter()
            .find(|section| section.id == section_id)
            .ok_or_else(|| {
                Error::new(format!(
                    "package does not contain the `{PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID}` \
                     section"
                ))
            })?
            .data
            .as_ref();
        let unpadded_len = bytes.iter().rposition(|byte| *byte != 0).map_or(0, |index| index + 1);
        let text = core::str::from_utf8(&bytes[..unpadded_len]).map_err(|err| {
            Error::new(format!("note storage schema section is not valid UTF-8: {err}"))
        })?;
        Self::from_wit_text(text)
    }

    /// Resolves a note storage schema from a WIT document.
    pub fn from_wit_text(wit_text: &str) -> Result<Self> {
        let wit_text = wit_text.trim_end_matches('\0');
        let mut resolve = Resolve::default();
        let package_id = resolve.push_str("note-storage-schema.wit", wit_text).map_err(|err| {
            Error::new(format!("failed to resolve note storage schema WIT: {err:#}"))
        })?;
        let package = &resolve.packages[package_id];
        let interface_id = package.interfaces.get("note-storage").copied().ok_or_else(|| {
            Error::new(format!(
                "schema package `{}` does not define the `note-storage` interface",
                package.name
            ))
        })?;
        let interface = &resolve.interfaces[interface_id];
        let storage_id = interface.types.get("storage").copied().ok_or_else(|| {
            Error::new("the `note-storage` interface does not define the `storage` type alias")
        })?;
        let root = ModelBuilder::new(&resolve).build(Type::Id(storage_id))?;
        if !matches!(root.kind, SchemaTypeKind::Record(_)) {
            return Err(Error::new(format!(
                "the `note-storage.storage` alias must resolve to a record, found {}",
                kind_name(&root.kind)
            )));
        }

        Ok(Self {
            wit_text: wit_text.to_owned(),
            root,
            codecs: CodecRegistry::default(),
        })
    }

    /// Returns the unpadded WIT document.
    pub fn wit_text(&self) -> &str {
        &self.wit_text
    }

    /// Returns the root storage record.
    pub const fn root(&self) -> &SchemaType {
        &self.root
    }

    /// Returns the root felt layout.
    pub const fn layout(&self) -> FeltLayout {
        self.root.layout
    }

    /// Returns the schema's standard codec registry.
    pub const fn codecs(&self) -> &CodecRegistry {
        &self.codecs
    }

    /// Replaces the codec registry used by `builder` and `decode`.
    pub fn with_codec_registry(mut self, codecs: CodecRegistry) -> Self {
        self.codecs = codecs;
        self
    }

    /// Creates a string-value builder with the schema's codec registry.
    pub fn builder(&self) -> NoteStorageBuilder<'_> {
        self.builder_with_registry(&self.codecs)
    }

    /// Creates a string-value builder with a caller-provided codec registry.
    pub fn builder_with_registry<'a>(
        &'a self,
        registry: &'a CodecRegistry,
    ) -> NoteStorageBuilder<'a> {
        NoteStorageBuilder::new(self, registry)
    }

    /// Decodes note storage with the schema's codec registry.
    pub fn decode(&self, storage: &NoteStorage) -> Result<DecodedValue> {
        self.decode_with_registry(storage, &self.codecs)
    }

    /// Decodes note storage with a caller-provided codec registry.
    pub fn decode_with_registry(
        &self,
        storage: &NoteStorage,
        registry: &CodecRegistry,
    ) -> Result<DecodedValue> {
        crate::value::decode(&self.root, storage, registry)
    }
}

/// Builds an owned schema type tree from a resolved WIT graph.
struct ModelBuilder<'a> {
    resolve: &'a Resolve,
    active: HashSet<TypeId>,
}

impl<'a> ModelBuilder<'a> {
    /// Creates a model builder for one schema package.
    fn new(resolve: &'a Resolve) -> Self {
        Self {
            resolve,
            active: HashSet::new(),
        }
    }

    /// Resolves one WIT type.
    fn build(mut self, ty: Type) -> Result<SchemaType> {
        self.build_type(ty)
    }

    /// Resolves a primitive or named type.
    fn build_type(&mut self, ty: Type) -> Result<SchemaType> {
        match ty {
            Type::Id(id) => self.build_type_id(id),
            Type::U64 => self.primitive(PrimitiveType::U64, None, None, None),
            Type::U32 => self.primitive(PrimitiveType::U32, None, None, None),
            Type::U8 => self.primitive(PrimitiveType::U8, None, None, None),
            Type::Bool => self.primitive(PrimitiveType::Bool, None, None, None),
            unsupported => Err(Error::new(format!(
                "WIT primitive `{unsupported:?}` is not supported in note storage schemas"
            ))),
        }
    }

    /// Resolves aliases to the type definition that owns the structural type.
    fn build_type_id(&mut self, id: TypeId) -> Result<SchemaType> {
        let id = self.follow_aliases(id)?;
        if !self.active.insert(id) {
            return Err(Error::new(
                "recursive WIT types are not supported in note storage schemas",
            ));
        }

        let definition = self.resolve.types[id].clone();
        let name = definition.name.clone();
        let docs = definition.docs.contents.clone();
        let fqn = self.type_fqn(id)?;
        let result = if fqn.as_deref() == Some(FELT_FQN) {
            Ok(SchemaType {
                name,
                fqn,
                docs,
                kind: SchemaTypeKind::Felt,
                layout: FeltLayout::fixed(1),
            })
        } else {
            match definition.kind {
                TypeDefKind::Type(ty) => self.build_named_alias(ty, name, fqn, docs),
                TypeDefKind::Record(record) => {
                    let mut fields = Vec::with_capacity(record.fields.len());
                    let mut layout = FeltLayout::fixed(0);
                    for field in record.fields {
                        let ty = self.build_type(field.ty)?;
                        layout = layout.concatenate(ty.layout)?;
                        fields.push(SchemaField {
                            name: field.name,
                            docs: field.docs.contents,
                            ty,
                        });
                    }
                    Ok(SchemaType {
                        name,
                        fqn,
                        docs,
                        kind: SchemaTypeKind::Record(fields),
                        layout,
                    })
                }
                TypeDefKind::Option(payload) => {
                    let payload = Box::new(self.build_type(payload)?);
                    let layout = FeltLayout {
                        minimum: 1,
                        maximum: 1usize.checked_add(payload.layout.maximum).ok_or_else(|| {
                            Error::new("option layout maximum width is too large")
                        })?,
                    };
                    Ok(SchemaType {
                        name,
                        fqn,
                        docs,
                        kind: SchemaTypeKind::Option(payload),
                        layout,
                    })
                }
                TypeDefKind::Variant(variant) => {
                    let mut cases = Vec::with_capacity(variant.cases.len());
                    for case in variant.cases {
                        cases.push(SchemaCase {
                            name: case.name,
                            docs: case.docs.contents,
                            payload: case.ty.map(|ty| self.build_type(ty)).transpose()?,
                        });
                    }
                    let layout = variant_layout(&cases)?;
                    Ok(SchemaType {
                        name,
                        fqn,
                        docs,
                        kind: SchemaTypeKind::Variant(cases),
                        layout,
                    })
                }
                TypeDefKind::Enum(enum_) => {
                    let cases = enum_
                        .cases
                        .into_iter()
                        .map(|case| SchemaCase {
                            name: case.name,
                            docs: case.docs.contents,
                            payload: None,
                        })
                        .collect::<Vec<_>>();
                    let layout = variant_layout(&cases)?;
                    Ok(SchemaType {
                        name,
                        fqn,
                        docs,
                        kind: SchemaTypeKind::Variant(cases),
                        layout,
                    })
                }
                unsupported => Err(Error::new(format!(
                    "WIT {} `{}` is not supported in note storage schemas",
                    unsupported.as_str(),
                    fqn.as_deref().or(name.as_deref()).unwrap_or("<anonymous>")
                ))),
            }
        };
        self.active.remove(&id);
        result
    }

    /// Resolves a named alias whose target is a primitive.
    fn build_named_alias(
        &mut self,
        ty: Type,
        name: Option<String>,
        fqn: Option<String>,
        docs: Option<String>,
    ) -> Result<SchemaType> {
        match ty {
            Type::Id(id) => self.build_type_id(id),
            Type::U64 => self.primitive(PrimitiveType::U64, name, fqn, docs),
            Type::U32 => self.primitive(PrimitiveType::U32, name, fqn, docs),
            Type::U8 => self.primitive(PrimitiveType::U8, name, fqn, docs),
            Type::Bool => self.primitive(PrimitiveType::Bool, name, fqn, docs),
            unsupported => Err(Error::new(format!(
                "WIT primitive alias `{unsupported:?}` is not supported in note storage schemas"
            ))),
        }
    }

    /// Creates a supported primitive type.
    fn primitive(
        &self,
        primitive: PrimitiveType,
        name: Option<String>,
        fqn: Option<String>,
        docs: Option<String>,
    ) -> Result<SchemaType> {
        let width = match primitive {
            PrimitiveType::U64 => 2,
            PrimitiveType::U32 | PrimitiveType::U8 | PrimitiveType::Bool => 1,
        };
        Ok(SchemaType {
            name,
            fqn,
            docs,
            kind: SchemaTypeKind::Primitive(primitive),
            layout: FeltLayout::fixed(width),
        })
    }

    /// Follows `type = id` aliases to their defining type.
    fn follow_aliases(&self, mut id: TypeId) -> Result<TypeId> {
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(id) {
                return Err(Error::new("cyclic WIT type aliases are not supported"));
            }
            match self.resolve.types[id].kind {
                TypeDefKind::Type(Type::Id(next)) => id = next,
                _ => return Ok(id),
            }
        }
    }

    /// Reconstructs the canonical FQN for a named interface type.
    fn type_fqn(&self, id: TypeId) -> Result<Option<String>> {
        let definition = &self.resolve.types[id];
        let Some(type_name) = definition.name.as_deref() else {
            return Ok(None);
        };
        let TypeOwner::Interface(interface_id) = definition.owner else {
            return Err(Error::new(format!(
                "named WIT type `{type_name}` is not owned by an interface"
            )));
        };
        let interface = &self.resolve.interfaces[interface_id];
        let interface_name = interface.name.as_deref().ok_or_else(|| {
            Error::new(format!("type `{type_name}` belongs to an unnamed interface"))
        })?;
        let package_id = interface.package.ok_or_else(|| {
            Error::new(format!("interface `{interface_name}` does not belong to a package"))
        })?;
        let package_name = &self.resolve.packages[package_id].name;
        let mut fqn =
            format!("{}:{}/{}", package_name.namespace, package_name.name, interface_name);
        if let Some(version) = &package_name.version {
            fqn.push('@');
            fqn.push_str(&version.to_string());
        }
        fqn.push('.');
        fqn.push_str(type_name);
        Ok(Some(fqn))
    }
}

/// Returns a variable layout for declaration-ordinal cases.
fn variant_layout(cases: &[SchemaCase]) -> Result<FeltLayout> {
    if cases.is_empty() {
        return Err(Error::new("a note storage variant must define at least one case"));
    }
    let minimum_payload = cases
        .iter()
        .map(|case| case.payload.as_ref().map_or(0, |ty| ty.layout.minimum))
        .min()
        .unwrap_or(0);
    let maximum_payload = cases
        .iter()
        .map(|case| case.payload.as_ref().map_or(0, |ty| ty.layout.maximum))
        .max()
        .unwrap_or(0);
    Ok(FeltLayout {
        minimum: 1usize
            .checked_add(minimum_payload)
            .ok_or_else(|| Error::new("variant layout minimum width is too large"))?,
        maximum: 1usize
            .checked_add(maximum_payload)
            .ok_or_else(|| Error::new("variant layout maximum width is too large"))?,
    })
}

/// Returns a stable name for a model kind.
fn kind_name(kind: &SchemaTypeKind) -> &'static str {
    match kind {
        SchemaTypeKind::Felt => "felt",
        SchemaTypeKind::Primitive(_) => "primitive",
        SchemaTypeKind::Record(_) => "record",
        SchemaTypeKind::Option(_) => "option",
        SchemaTypeKind::Variant(_) => "variant",
    }
}

/// Normalizes one WIT path segment from snake case to kebab case.
pub(crate) fn normalize_name(name: &str) -> String {
    name.trim().replace('_', "-")
}
