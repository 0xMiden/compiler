//! Resolved schema model.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use miden_mast_package::{Package, SectionId};
use midenc_frontend_wasm_metadata::trim_trailing_nuls;
use wit_parser::{Resolve, Type, TypeDefKind, TypeId, TypeOwner};

#[cfg(feature = "protocol")]
use crate::{CodecRegistry, DecodedValue, Felt, ValueBuilder};
use crate::{Error, FELT_FQN, Result, StandardLeaf};

/// The resource limits a schema is held to while it is read and resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaLimits {
    /// Maximum bytes accepted in an embedded schema section, including alignment padding.
    pub max_schema_bytes: usize,
    /// Maximum number of resolved WIT type definitions in a schema.
    pub max_types: usize,
    /// Maximum structural nesting depth accepted while resolving a schema.
    pub max_depth: usize,
    /// Maximum number of felts in the root layout, and in the layout of every type in it.
    pub max_felts: usize,
    /// Maximum number of nodes in the expanded schema tree.
    ///
    /// The resolved model is a DAG, but every structural walk over it expands that DAG into a
    /// tree: decoding, builder validation, and Rust code generation all visit a shared type once
    /// per reference. A schema of zero-felt records that names one type per level stays below the
    /// byte, type, depth, and felt limits while the expanded tree doubles at each level, so this
    /// budget bounds the expanded tree directly.
    pub max_nodes: usize,
}

impl SchemaLimits {
    /// Derives the limits for a schema that describes at most `items` felts.
    ///
    /// The budget allows 64 bytes of schema description and four expanded nodes per item, one
    /// resolved type per item, and a nesting depth of one eighth of the item count, which leaves
    /// ample room for legitimate models without allowing an attacker-controlled parser stack to
    /// grow to the full width.
    pub const fn for_item_count(items: usize) -> Self {
        Self {
            max_schema_bytes: items * 64,
            max_types: items,
            max_depth: items / 8,
            max_felts: items,
            max_nodes: items * 4,
        }
    }
}

/// The kind of schema a package carries: where it lives, how its WIT names its root, and the
/// limits it is held to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaProfile {
    /// The human-readable kind name used in diagnostics, for example `"note storage"`.
    pub name: &'static str,
    /// The WIT interface that defines the root alias, for example `"note-storage"`.
    pub interface: &'static str,
    /// The type alias in [`Self::interface`] that names the root record, for example `"storage"`.
    pub root_alias: &'static str,
    /// The custom package section that carries the schema WIT.
    pub section_id: &'static str,
    /// The limits the schema is held to.
    pub limits: SchemaLimits,
}

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
    fn concatenate(self, other: Self, max_felts: usize) -> Result<Self> {
        let minimum = self
            .minimum
            .checked_add(other.minimum)
            .ok_or_else(|| Error::new("schema layout minimum width is too large"))?;
        let maximum = self
            .maximum
            .checked_add(other.maximum)
            .ok_or_else(|| Error::new("schema layout maximum width is too large"))?;
        Self::bounded(minimum, maximum, max_felts)
    }

    /// Creates a layout no wider than `max_felts`.
    ///
    /// Every composed layout passes through this function, so no resolved type, and no resolved
    /// root, is wider than the schema limits allow.
    fn bounded(minimum: usize, maximum: usize, max_felts: usize) -> Result<Self> {
        if maximum > max_felts {
            return Err(Error::new(format!(
                "schema layout has maximum width {maximum} felts; the limit is {max_felts}"
            )));
        }
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
    Option(Arc<SchemaType>),
    /// A variant with declaration-ordinal cases.
    Variant(Vec<SchemaCase>),
}

/// A resolved WIT type used by a schema.
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

    /// Classifies this type as a standard protocol leaf.
    pub fn standard_leaf(&self) -> Option<StandardLeaf> {
        self.fqn.as_deref().and_then(StandardLeaf::from_fqn)
    }
}

/// A named record field in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaField {
    name: String,
    docs: Option<String>,
    ty: Arc<SchemaType>,
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
    pub fn ty(&self) -> &SchemaType {
        self.ty.as_ref()
    }
}

/// A WIT variant case in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaCase {
    name: String,
    docs: Option<String>,
    payload: Option<Arc<SchemaType>>,
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
    pub fn payload(&self) -> Option<&SchemaType> {
        self.payload.as_deref()
    }
}

/// A resolved schema of one [`SchemaProfile`], with the standard codec registry when the
/// `protocol` feature is enabled.
#[derive(Clone)]
pub struct Schema {
    profile: SchemaProfile,
    wit_text: String,
    root: Arc<SchemaType>,
    #[cfg(feature = "protocol")]
    codecs: CodecRegistry,
}

impl Schema {
    /// Reads and resolves the schema section of `profile` from a Miden package.
    pub fn from_package(package: &Package, profile: &SchemaProfile) -> Result<Self> {
        let section_id = SectionId::custom(profile.section_id).map_err(|err| {
            Error::new(format!(
                "`{}` is not a valid package section id for the {} schema: {err}",
                profile.section_id, profile.name
            ))
        })?;
        let bytes =
            crate::section::unique_package_section(package, section_id, profile.section_id)?;
        ensure_schema_byte_limit(bytes.len(), profile)?;
        let text = core::str::from_utf8(trim_trailing_nuls(bytes)).map_err(|err| {
            Error::new(format!("{} schema section is not valid UTF-8: {err}", profile.name))
        })?;
        Self::from_wit_text(text, profile)
    }

    /// Resolves a schema of `profile` from a WIT document.
    pub fn from_wit_text(wit_text: &str, profile: &SchemaProfile) -> Result<Self> {
        ensure_schema_byte_limit(wit_text.len(), profile)?;
        let wit_text = wit_text.trim_end_matches('\0');
        let limits = &profile.limits;
        let mut resolve = Resolve::default();
        let package_id = resolve
            .push_str(format!("{}-schema.wit", profile.interface), wit_text)
            .map_err(|err| {
                Error::new(format!("failed to resolve {} schema WIT: {err:#}", profile.name))
            })?;
        if resolve.types.len() > limits.max_types {
            return Err(Error::new(format!(
                "{} schema defines {} WIT types; the limit is {}",
                profile.name,
                resolve.types.len(),
                limits.max_types
            )));
        }
        let package = &resolve.packages[package_id];
        // Custom type FQNs embed the schema package name, and codecs and generated bindings are
        // keyed by them, so a version there would change every name on each release.
        if package.name.version.is_some() {
            return Err(Error::new(format!(
                "schema package `{}` must not carry a version; schema type names are unversioned",
                package.name
            )));
        }
        let interface_id = package.interfaces.get(profile.interface).copied().ok_or_else(|| {
            Error::new(format!(
                "schema package `{}` does not define the `{}` interface",
                package.name, profile.interface
            ))
        })?;
        let interface = &resolve.interfaces[interface_id];
        let root_id = interface.types.get(profile.root_alias).copied().ok_or_else(|| {
            Error::new(format!(
                "the `{}` interface does not define the `{}` type alias",
                profile.interface, profile.root_alias
            ))
        })?;
        validate_resolved_core_types(&resolve)?;
        // The root may be an alias of a record or the record itself; the builder follows aliases.
        let root = ModelBuilder::new(&resolve, limits).build(Type::Id(root_id))?;
        if !matches!(root.kind(), SchemaTypeKind::Record(_)) {
            return Err(Error::new(format!(
                "the `{}.{}` alias must resolve to a record, found {}",
                profile.interface,
                profile.root_alias,
                kind_name(root.kind())
            )));
        }
        let schema = Self {
            profile: *profile,
            wit_text: wit_text.to_owned(),
            root,
            #[cfg(feature = "protocol")]
            codecs: CodecRegistry::default(),
        };
        schema.validate_native_leaf_shapes()?;
        Ok(schema)
    }

    /// Returns the profile this schema was resolved with.
    pub const fn profile(&self) -> &SchemaProfile {
        &self.profile
    }

    /// Returns the unpadded WIT document.
    pub fn wit_text(&self) -> &str {
        &self.wit_text
    }

    /// Returns the root record.
    pub fn root(&self) -> &SchemaType {
        self.root.as_ref()
    }

    /// Verifies native host mappings against the pinned standard type shapes.
    pub fn validate_native_leaf_shapes(&self) -> Result<()> {
        validate_model_type_shapes(&self.root, &mut HashSet::new())
    }

    /// Returns all custom named types reachable from the root.
    ///
    /// A custom type is a named type that is not a [`StandardLeaf`]; these are the types an
    /// author codec may handle.
    pub fn custom_type_fqns(&self) -> HashSet<String> {
        let mut fqns = HashSet::new();
        collect_custom_type_fqns(&self.root, &mut HashSet::new(), &mut fqns);
        fqns
    }

    /// Returns the root felt layout.
    pub fn layout(&self) -> FeltLayout {
        self.root.layout
    }

    /// Returns the schema's standard codec registry.
    #[cfg(feature = "protocol")]
    pub const fn codecs(&self) -> &CodecRegistry {
        &self.codecs
    }

    /// Replaces the codec registry used by `builder` and `decode`.
    #[cfg(feature = "protocol")]
    pub fn with_codec_registry(mut self, codecs: CodecRegistry) -> Self {
        self.codecs = codecs;
        self
    }

    /// Creates a string-value builder with the schema's codec registry.
    #[cfg(feature = "protocol")]
    pub fn builder(&self) -> ValueBuilder<'_> {
        self.builder_with_registry(&self.codecs)
    }

    /// Creates a string-value builder with a caller-provided codec registry.
    #[cfg(feature = "protocol")]
    pub fn builder_with_registry<'a>(&'a self, registry: &'a CodecRegistry) -> ValueBuilder<'a> {
        ValueBuilder::new(self, registry)
    }

    /// Decodes a complete felt encoding of the root with the schema's codec registry.
    #[cfg(feature = "protocol")]
    pub fn decode(&self, felts: &[Felt]) -> Result<DecodedValue> {
        self.decode_with_registry(felts, &self.codecs)
    }

    /// Decodes a complete felt encoding of the root with a caller-provided codec registry.
    #[cfg(feature = "protocol")]
    pub fn decode_with_registry(
        &self,
        felts: &[Felt],
        registry: &CodecRegistry,
    ) -> Result<DecodedValue> {
        crate::value::decode(&self.root, felts, registry, self.profile.name)
    }
}

/// Enforces the parser input budget before WIT resolution or component work begins.
fn ensure_schema_byte_limit(byte_len: usize, profile: &SchemaProfile) -> Result<()> {
    let limit = profile.limits.max_schema_bytes;
    if byte_len > limit {
        return Err(Error::new(format!(
            "{} schema section is {byte_len} bytes; the limit is {limit}",
            profile.name
        )));
    }
    Ok(())
}

/// Enforces the expanded-tree budget on one resolved schema type.
///
/// The builder memoizes shared types, so resolution stays linear. Every consumer of the model
/// walks it as a tree, so the budget is applied to the expanded node count of each type as it is
/// resolved. The check therefore protects decoding, builder validation, and code generation.
fn ensure_expanded_node_limit(
    ty: &SchemaType,
    expanded_nodes: usize,
    limits: &SchemaLimits,
) -> Result<()> {
    let limit = limits.max_nodes;
    if expanded_nodes > limit {
        let name = ty.fqn().or_else(|| ty.name()).unwrap_or("<anonymous>");
        return Err(Error::new(format!(
            "schema type `{name}` expands to {expanded_nodes} nodes; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Verifies the raw embedded core-types definitions before the model applies native mappings.
fn validate_resolved_core_types(resolve: &Resolve) -> Result<()> {
    let Some((_, package_id)) = resolve.package_names.iter().find(|(name, _)| {
        name.namespace == "miden"
            && name.name == "base"
            && name.version.as_ref().is_some_and(|version| version.to_string() == "1.0.0")
    }) else {
        return Ok(());
    };
    let package = &resolve.packages[*package_id];
    let Some(interface_id) = package.interfaces.get("core-types").copied() else {
        return Ok(());
    };
    let interface = &resolve.interfaces[interface_id];

    for leaf in StandardLeaf::ALL {
        let name = standard_leaf_name(leaf);
        let fields = standard_leaf_fields(leaf);
        let Some(type_id) = interface.types.get(name).copied() else {
            continue;
        };
        let type_id = follow_resolved_aliases(resolve, type_id)?;
        let TypeDefKind::Record(record) = &resolve.types[type_id].kind else {
            return Err(core_shape_error(name, fields));
        };
        if record.fields.len() != fields.len()
            || record
                .fields
                .iter()
                .zip(fields)
                .any(|(field, expected)| field.name != *expected)
        {
            return Err(core_shape_error(name, fields));
        }
        if leaf == StandardLeaf::Felt {
            // Miden's canonical felt uses `f32` as its WIT-level placeholder representation.
            if !resolves_to_primitive(resolve, record.fields[0].ty, Type::F32)? {
                return Err(core_shape_error(name, fields));
            }
        } else {
            for field in &record.fields {
                if !resolves_to_fqn(resolve, field.ty, crate::FELT_FQN)? {
                    return Err(core_shape_error(name, fields));
                }
            }
        }
    }
    Ok(())
}

/// Follows raw WIT aliases to their structural definition.
fn follow_resolved_aliases(resolve: &Resolve, mut id: TypeId) -> Result<TypeId> {
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

/// Returns true when a raw WIT type resolves to one primitive.
fn resolves_to_primitive(resolve: &Resolve, mut ty: Type, expected: Type) -> Result<bool> {
    let mut visited = HashSet::new();
    loop {
        match ty {
            Type::Id(id) => {
                if !visited.insert(id) {
                    return Err(Error::new("cyclic WIT type aliases are not supported"));
                }
                let TypeDefKind::Type(next) = resolve.types[id].kind else {
                    return Ok(false);
                };
                ty = next;
            }
            primitive => return Ok(primitive == expected),
        }
    }
}

/// Returns true when a raw WIT type resolves to one canonical FQN.
fn resolves_to_fqn(resolve: &Resolve, ty: Type, expected: &str) -> Result<bool> {
    let Type::Id(id) = ty else {
        return Ok(false);
    };
    let id = follow_resolved_aliases(resolve, id)?;
    Ok(type_fqn(resolve, id)?.as_deref() == Some(expected))
}

/// Verifies mapped type shapes in the owned schema model.
fn validate_model_type_shapes(ty: &SchemaType, seen: &mut HashSet<String>) -> Result<()> {
    if let Some(fqn) = ty.fqn()
        && !seen.insert(fqn.to_owned())
    {
        return Ok(());
    }

    match ty.standard_leaf() {
        Some(StandardLeaf::Felt) if !matches!(ty.kind(), SchemaTypeKind::Felt) => {
            return Err(core_shape_error(
                standard_leaf_name(StandardLeaf::Felt),
                standard_leaf_fields(StandardLeaf::Felt),
            ));
        }
        Some(StandardLeaf::Felt) => {}
        Some(leaf) => {
            validate_model_record(
                ty,
                standard_leaf_name(leaf),
                standard_leaf_fields(leaf),
                crate::FELT_FQN,
            )?;
        }
        None => {}
    }

    match ty.kind() {
        SchemaTypeKind::Record(fields) => {
            for field in fields {
                validate_model_type_shapes(field.ty(), seen)?;
            }
        }
        SchemaTypeKind::Option(payload) => validate_model_type_shapes(payload, seen)?,
        SchemaTypeKind::Variant(cases) => {
            for payload in cases.iter().filter_map(SchemaCase::payload) {
                validate_model_type_shapes(payload, seen)?;
            }
        }
        SchemaTypeKind::Felt | SchemaTypeKind::Primitive(_) => {}
    }
    Ok(())
}

/// Returns the terminal WIT name from a canonical standard-leaf FQN.
fn standard_leaf_name(leaf: StandardLeaf) -> &'static str {
    leaf.fqn()
        .rsplit_once('.')
        .expect("standard-leaf FQNs always contain an interface separator")
        .1
}

/// Returns the canonical record field order for a standard leaf.
fn standard_leaf_fields(leaf: StandardLeaf) -> &'static [&'static str] {
    match leaf {
        StandardLeaf::Felt | StandardLeaf::AssetAmount => &["inner"],
        StandardLeaf::Word => &["a", "b", "c", "d"],
        StandardLeaf::AccountId => &["prefix", "suffix"],
    }
}

/// Verifies one mapped record in the owned schema model.
fn validate_model_record(
    ty: &SchemaType,
    name: &str,
    expected_fields: &[&str],
    expected_field_fqn: &str,
) -> Result<()> {
    let SchemaTypeKind::Record(fields) = ty.kind() else {
        return Err(core_shape_error(name, expected_fields));
    };
    if fields.len() != expected_fields.len()
        || fields.iter().zip(expected_fields).any(|(field, expected)| {
            field.name() != *expected || field.ty().fqn() != Some(expected_field_fqn)
        })
    {
        return Err(core_shape_error(name, expected_fields));
    }
    Ok(())
}

/// Creates the canonical core-type shape diagnostic.
fn core_shape_error(name: &str, fields: &[&str]) -> Error {
    let field_shape = if name == "felt" {
        "inner: f32".to_owned()
    } else {
        fields
            .iter()
            .map(|field| format!("{field}: felt"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    Error::new(format!(
        "embedded WIT type `miden:base/core-types@1.0.0.{name}` does not match the pinned \
         canonical shape `record {name} {{ {field_shape} }}`"
    ))
}

/// Collects schema-owned types and excludes only the canonical standard leaves.
fn collect_custom_type_fqns(
    ty: &SchemaType,
    seen: &mut HashSet<*const SchemaType>,
    fqns: &mut HashSet<String>,
) {
    // Pointer identity is sufficient because ModelBuilder memoizes exactly one Arc per TypeId.
    if !seen.insert(core::ptr::from_ref(ty)) {
        return;
    }
    if let Some(fqn) = ty.fqn()
        && ty.standard_leaf().is_none()
    {
        fqns.insert(fqn.to_owned());
    }
    match ty.kind() {
        SchemaTypeKind::Record(fields) => {
            for field in fields {
                collect_custom_type_fqns(field.ty(), seen, fqns);
            }
        }
        SchemaTypeKind::Option(payload) => collect_custom_type_fqns(payload, seen, fqns),
        SchemaTypeKind::Variant(cases) => {
            for payload in cases.iter().filter_map(SchemaCase::payload) {
                collect_custom_type_fqns(payload, seen, fqns);
            }
        }
        SchemaTypeKind::Felt | SchemaTypeKind::Primitive(_) => {}
    }
}

/// One memoized schema node, its maximum depth, and the size of its expanded subtree.
#[derive(Clone)]
struct MemoizedSchemaType {
    /// The resolved node.
    ty: Arc<SchemaType>,
    /// Levels of nesting below this node.
    ///
    /// A memoized node is reused at a deeper position than the one it was resolved at, so the
    /// depth limit is checked again on every reuse with this value added to the new depth.
    maximum_subtree_depth: usize,
    /// Number of nodes a structural walk visits below and including this node.
    expanded_nodes: usize,
}

/// Builds a memoized schema graph from a resolved WIT graph.
struct ModelBuilder<'a> {
    /// The resolved WIT document the schema comes from.
    resolve: &'a Resolve,
    /// The limits the resolved model is held to.
    limits: &'a SchemaLimits,
    /// The types the walk is inside, which reports a recursive type.
    active: HashSet<TypeId>,
    /// The nodes already resolved, keyed by WIT type.
    memo: HashMap<TypeId, MemoizedSchemaType>,
}

impl<'a> ModelBuilder<'a> {
    /// Creates a model builder for one schema package.
    fn new(resolve: &'a Resolve, limits: &'a SchemaLimits) -> Self {
        Self {
            resolve,
            limits,
            active: HashSet::new(),
            memo: HashMap::new(),
        }
    }

    /// Resolves one WIT type.
    fn build(mut self, ty: Type) -> Result<Arc<SchemaType>> {
        self.build_type(ty, 0).map(|memoized| memoized.ty)
    }

    /// Resolves a primitive or named type.
    fn build_type(&mut self, ty: Type, depth: usize) -> Result<MemoizedSchemaType> {
        let max_depth = self.limits.max_depth;
        if depth > max_depth {
            return Err(Error::new(format!(
                "schema nesting depth {depth} exceeds the limit of {max_depth}"
            )));
        }
        match ty {
            Type::Id(id) => self.build_type_id(id, depth),
            Type::U64 => self.primitive(PrimitiveType::U64, None, None, None),
            Type::U32 => self.primitive(PrimitiveType::U32, None, None, None),
            Type::U8 => self.primitive(PrimitiveType::U8, None, None, None),
            Type::Bool => self.primitive(PrimitiveType::Bool, None, None, None),
            unsupported => Err(Error::new(format!(
                "WIT primitive `{unsupported:?}` is not supported in schemas"
            ))),
        }
    }

    /// Resolves aliases to the type definition that owns the structural type.
    fn build_type_id(&mut self, id: TypeId, depth: usize) -> Result<MemoizedSchemaType> {
        let id = self.follow_aliases(id)?;
        if let Some(memoized) = self.memo.get(&id) {
            let maximum_depth = depth.saturating_add(memoized.maximum_subtree_depth);
            let max_depth = self.limits.max_depth;
            if maximum_depth > max_depth {
                return Err(Error::new(format!(
                    "schema nesting depth {maximum_depth} exceeds the limit of {max_depth}"
                )));
            }
            return Ok(memoized.clone());
        }
        if !self.active.insert(id) {
            return Err(Error::new("recursive WIT types are not supported in schemas"));
        }

        let definition = self.resolve.types[id].clone();
        let name = definition.name.clone();
        let docs = definition.docs.contents.clone();
        let fqn = type_fqn(self.resolve, id)?;
        let max_felts = self.limits.max_felts;
        let result = if fqn.as_deref() == Some(FELT_FQN) {
            Ok(MemoizedSchemaType {
                ty: Arc::new(SchemaType {
                    name,
                    fqn,
                    docs,
                    kind: SchemaTypeKind::Felt,
                    layout: FeltLayout::fixed(1),
                }),
                maximum_subtree_depth: 0,
                expanded_nodes: 1,
            })
        } else {
            match definition.kind {
                TypeDefKind::Type(ty) => self.build_named_alias(ty, name, fqn, docs),
                TypeDefKind::Record(record) => {
                    let mut fields = Vec::with_capacity(record.fields.len());
                    let mut layout = FeltLayout::fixed(0);
                    let mut maximum_subtree_depth = 0;
                    let mut expanded_nodes = 1usize;
                    for field in record.fields {
                        let memoized = self.build_type(field.ty, depth + 1)?;
                        maximum_subtree_depth =
                            maximum_subtree_depth.max(1 + memoized.maximum_subtree_depth);
                        expanded_nodes = expanded_nodes.saturating_add(memoized.expanded_nodes);
                        layout = layout.concatenate(memoized.ty.layout, max_felts)?;
                        fields.push(SchemaField {
                            name: field.name,
                            docs: field.docs.contents,
                            ty: memoized.ty,
                        });
                    }
                    Ok(MemoizedSchemaType {
                        ty: Arc::new(SchemaType {
                            name,
                            fqn,
                            docs,
                            kind: SchemaTypeKind::Record(fields),
                            layout,
                        }),
                        maximum_subtree_depth,
                        expanded_nodes,
                    })
                }
                TypeDefKind::Option(payload) => {
                    let payload = self.build_type(payload, depth + 1)?;
                    let maximum = 1usize
                        .checked_add(payload.ty.layout.maximum)
                        .ok_or_else(|| Error::new("option layout maximum width is too large"))?;
                    let layout = FeltLayout::bounded(1, maximum, max_felts)?;
                    Ok(MemoizedSchemaType {
                        maximum_subtree_depth: 1 + payload.maximum_subtree_depth,
                        expanded_nodes: payload.expanded_nodes.saturating_add(1),
                        ty: Arc::new(SchemaType {
                            name,
                            fqn,
                            docs,
                            kind: SchemaTypeKind::Option(payload.ty),
                            layout,
                        }),
                    })
                }
                TypeDefKind::Variant(variant) => {
                    let mut cases = Vec::with_capacity(variant.cases.len());
                    let mut maximum_subtree_depth = 0;
                    let mut expanded_nodes = 1usize;
                    for case in variant.cases {
                        let payload = match case.ty {
                            Some(ty) => {
                                let memoized = self.build_type(ty, depth + 1)?;
                                maximum_subtree_depth =
                                    maximum_subtree_depth.max(1 + memoized.maximum_subtree_depth);
                                expanded_nodes =
                                    expanded_nodes.saturating_add(memoized.expanded_nodes);
                                Some(memoized.ty)
                            }
                            None => None,
                        };
                        cases.push(SchemaCase {
                            name: case.name,
                            docs: case.docs.contents,
                            payload,
                        });
                    }
                    let layout = variant_layout(&cases, max_felts)?;
                    Ok(MemoizedSchemaType {
                        ty: Arc::new(SchemaType {
                            name,
                            fqn,
                            docs,
                            kind: SchemaTypeKind::Variant(cases),
                            layout,
                        }),
                        maximum_subtree_depth,
                        expanded_nodes,
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
                    let layout = variant_layout(&cases, max_felts)?;
                    Ok(MemoizedSchemaType {
                        ty: Arc::new(SchemaType {
                            name,
                            fqn,
                            docs,
                            kind: SchemaTypeKind::Variant(cases),
                            layout,
                        }),
                        maximum_subtree_depth: 0,
                        expanded_nodes: 1,
                    })
                }
                unsupported => Err(Error::new(format!(
                    "WIT {} `{}` is not supported in schemas",
                    unsupported.as_str(),
                    fqn.as_deref().or(name.as_deref()).unwrap_or("<anonymous>")
                ))),
            }
        };
        self.active.remove(&id);
        if let Ok(memoized) = &result {
            ensure_expanded_node_limit(&memoized.ty, memoized.expanded_nodes, self.limits)?;
            self.memo.insert(id, memoized.clone());
        }
        result
    }

    /// Resolves a primitive alias while preserving its name, FQN, and documentation.
    ///
    /// [`Self::build_type_id`] follows ID aliases before dispatching here.
    fn build_named_alias(
        &mut self,
        ty: Type,
        name: Option<String>,
        fqn: Option<String>,
        docs: Option<String>,
    ) -> Result<MemoizedSchemaType> {
        match ty {
            Type::Id(_) => unreachable!("build_type_id must follow ID aliases first"),
            Type::U64 => self.primitive(PrimitiveType::U64, name, fqn, docs),
            Type::U32 => self.primitive(PrimitiveType::U32, name, fqn, docs),
            Type::U8 => self.primitive(PrimitiveType::U8, name, fqn, docs),
            Type::Bool => self.primitive(PrimitiveType::Bool, name, fqn, docs),
            unsupported => Err(Error::new(format!(
                "WIT primitive alias `{unsupported:?}` is not supported in schemas"
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
    ) -> Result<MemoizedSchemaType> {
        let width = match primitive {
            PrimitiveType::U64 => 2,
            PrimitiveType::U32 | PrimitiveType::U8 | PrimitiveType::Bool => 1,
        };
        Ok(MemoizedSchemaType {
            ty: Arc::new(SchemaType {
                name,
                fqn,
                docs,
                kind: SchemaTypeKind::Primitive(primitive),
                layout: FeltLayout::fixed(width),
            }),
            maximum_subtree_depth: 0,
            expanded_nodes: 1,
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
}

/// Reconstructs the canonical FQN for a named interface type.
fn type_fqn(resolve: &Resolve, id: TypeId) -> Result<Option<String>> {
    let definition = &resolve.types[id];
    let Some(type_name) = definition.name.as_deref() else {
        return Ok(None);
    };
    let TypeOwner::Interface(interface_id) = definition.owner else {
        return Err(Error::new(format!(
            "named WIT type `{type_name}` is not owned by an interface"
        )));
    };
    let interface = &resolve.interfaces[interface_id];
    let interface_name = interface
        .name
        .as_deref()
        .ok_or_else(|| Error::new(format!("type `{type_name}` belongs to an unnamed interface")))?;
    let package_id = interface.package.ok_or_else(|| {
        Error::new(format!("interface `{interface_name}` does not belong to a package"))
    })?;
    let package_name = &resolve.packages[package_id].name;
    let mut fqn = format!("{}:{}/{}", package_name.namespace, package_name.name, interface_name);
    if let Some(version) = &package_name.version {
        fqn.push('@');
        fqn.push_str(&version.to_string());
    }
    fqn.push('.');
    fqn.push_str(type_name);
    Ok(Some(fqn))
}

/// Returns a variable layout for declaration-ordinal cases.
fn variant_layout(cases: &[SchemaCase], max_felts: usize) -> Result<FeltLayout> {
    if cases.is_empty() {
        return Err(Error::new("a schema variant must define at least one case"));
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
    let minimum = 1usize
        .checked_add(minimum_payload)
        .ok_or_else(|| Error::new("variant layout minimum width is too large"))?;
    let maximum = 1usize
        .checked_add(maximum_payload)
        .ok_or_else(|| Error::new("variant layout maximum width is too large"))?;
    FeltLayout::bounded(minimum, maximum, max_felts)
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
#[cfg(feature = "protocol")]
pub(crate) fn normalize_name(name: &str) -> String {
    name.trim().replace('_', "-")
}
