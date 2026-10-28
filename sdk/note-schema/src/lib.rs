//! The note layer over `miden-schema`: note storage schemas embedded in Miden packages.
//!
//! A note storage schema is a [`Schema`] of the [`NOTE_STORAGE_PROFILE`]. This crate binds that
//! profile to [`NoteStorage`]: [`NoteStorageSchema`] reads the schema from a package, builds
//! note storage from string values, and decodes note storage into named values. The felt layout,
//! the schema model, and the codec registry are those of `miden-schema`, which this crate
//! re-exports so a host needs one import path.
//!
//! # Features
//!
//! - `protocol` (default): everything that produces or consumes `NoteStorage`, the
//!   [`NoteStorageBuilder`], decoding, and the standard codecs. It is what makes this crate
//!   depend on `miden-protocol`.
//! - `codec-component`: loading author-supplied codec components with Wasmtime. Implies
//!   `protocol`.
//!
//! Without `protocol` the crate is the schema model alone, which is what the compiler uses.

#![deny(missing_docs)]

#[cfg(feature = "protocol")]
pub use miden_protocol::note::NoteStorage;
pub use miden_schema::*;

/// The protocol's limit on the number of items in a note's storage, from which the note storage
/// schema limits are derived.
///
/// Stated here, rather than read from `miden-protocol`, so that the schema model does not need
/// the protocol crate; when the `protocol` feature links it, the two are checked to agree.
pub const MAX_NOTE_STORAGE_ITEMS: usize = 1024;

#[cfg(feature = "protocol")]
const _: () = assert!(MAX_NOTE_STORAGE_ITEMS == miden_protocol::MAX_NOTE_STORAGE_ITEMS);

/// The limits a note storage schema is held to, derived from [`MAX_NOTE_STORAGE_ITEMS`].
pub const NOTE_STORAGE_LIMITS: SchemaLimits = SchemaLimits::for_item_count(MAX_NOTE_STORAGE_ITEMS);

const _: () = assert!(NOTE_STORAGE_LIMITS.max_depth > 0);

/// The schema profile of note storage.
pub const NOTE_STORAGE_PROFILE: SchemaProfile = SchemaProfile {
    name: "note storage",
    interface: "note-storage",
    root_alias: "storage",
    section_id: midenc_frontend_wasm_metadata::PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID,
    limits: NOTE_STORAGE_LIMITS,
};

/// Maximum bytes accepted in an embedded note storage schema section, including alignment padding.
pub const MAX_NOTE_STORAGE_SCHEMA_BYTES: usize = NOTE_STORAGE_LIMITS.max_schema_bytes;

/// Maximum number of resolved WIT type definitions in a note storage schema.
pub const MAX_NOTE_STORAGE_SCHEMA_TYPES: usize = NOTE_STORAGE_LIMITS.max_types;

/// Maximum structural nesting depth accepted while resolving a note storage schema.
pub const MAX_NOTE_STORAGE_SCHEMA_DEPTH: usize = NOTE_STORAGE_LIMITS.max_depth;

/// Maximum number of felts in the root note storage layout.
pub const MAX_NOTE_STORAGE_SCHEMA_FELTS: usize = NOTE_STORAGE_LIMITS.max_felts;

/// Maximum number of nodes in the expanded note storage schema tree.
pub const MAX_NOTE_STORAGE_SCHEMA_NODES: usize = NOTE_STORAGE_LIMITS.max_nodes;

/// A resolved note storage schema, with the standard codec registry when the `protocol` feature
/// is enabled.
#[derive(Clone)]
pub struct NoteStorageSchema(Schema);

impl NoteStorageSchema {
    /// Reads and resolves the note storage schema section from a Miden package.
    pub fn from_package(package: &miden_mast_package::Package) -> Result<Self> {
        Schema::from_package(package, &NOTE_STORAGE_PROFILE).map(Self)
    }

    /// Resolves a note storage schema from a WIT document.
    pub fn from_wit_text(wit_text: &str) -> Result<Self> {
        Schema::from_wit_text(wit_text, &NOTE_STORAGE_PROFILE).map(Self)
    }

    /// Returns the kind-agnostic schema.
    pub const fn schema(&self) -> &Schema {
        &self.0
    }

    /// Returns the unpadded WIT document.
    pub fn wit_text(&self) -> &str {
        self.0.wit_text()
    }

    /// Returns the root storage record.
    pub fn root(&self) -> &SchemaType {
        self.0.root()
    }

    /// Returns the root felt layout.
    pub fn layout(&self) -> FeltLayout {
        self.0.layout()
    }

    /// Verifies native host mappings against the pinned standard type shapes.
    pub fn validate_native_leaf_shapes(&self) -> Result<()> {
        self.0.validate_native_leaf_shapes()
    }

    /// Returns the schema's standard codec registry.
    #[cfg(feature = "protocol")]
    pub const fn codecs(&self) -> &CodecRegistry {
        self.0.codecs()
    }

    /// Replaces the codec registry used by `builder` and `decode`.
    #[cfg(feature = "protocol")]
    pub fn with_codec_registry(self, codecs: CodecRegistry) -> Self {
        Self(self.0.with_codec_registry(codecs))
    }

    /// Creates a string-value builder with the schema's codec registry.
    #[cfg(feature = "protocol")]
    pub fn builder(&self) -> NoteStorageBuilder<'_> {
        NoteStorageBuilder(self.0.builder())
    }

    /// Creates a string-value builder with a caller-provided codec registry.
    #[cfg(feature = "protocol")]
    pub fn builder_with_registry<'a>(
        &'a self,
        registry: &'a CodecRegistry,
    ) -> NoteStorageBuilder<'a> {
        NoteStorageBuilder(self.0.builder_with_registry(registry))
    }

    /// Decodes note storage with the schema's codec registry.
    #[cfg(feature = "protocol")]
    pub fn decode(&self, storage: &NoteStorage) -> Result<DecodedValue> {
        self.0.decode(storage.items())
    }

    /// Decodes note storage with a caller-provided codec registry.
    #[cfg(feature = "protocol")]
    pub fn decode_with_registry(
        &self,
        storage: &NoteStorage,
        registry: &CodecRegistry,
    ) -> Result<DecodedValue> {
        self.0.decode_with_registry(storage.items(), registry)
    }
}

/// Builds note storage from normalized dotted string paths.
#[cfg(feature = "protocol")]
pub struct NoteStorageBuilder<'a>(ValueBuilder<'a>);

#[cfg(feature = "protocol")]
impl NoteStorageBuilder<'_> {
    /// Sets a leaf value by kebab-case or snake_case dotted path.
    ///
    /// Options accept `none` or `some(<value>)`. Variants accept a case name or
    /// `case-name(<value>)` when the selected case has a payload.
    pub fn set(self, path: &str, value: impl AsRef<str>) -> Result<Self> {
        self.0.set(path, value).map(Self)
    }

    /// Checks completeness and returns note storage in declaration order.
    pub fn build(self) -> Result<NoteStorage> {
        let felts = self.0.build()?;
        NoteStorage::new(felts)
            .map_err(|err| Error::new(format!("failed to create note storage: {err}")))
    }
}
