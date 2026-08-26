use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

static EXPORTED_TYPES: OnceLock<Mutex<HashMap<String, Vec<RegisteredExportType>>>> =
    OnceLock::new();

use heck::{ToKebabCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::{Attribute, ItemStruct, Type, spanned::Spanned};
use wit_bindgen_core::wit_parser::Type as WitType;

use crate::manifest_paths::SDK_WIT_SOURCE;

#[derive(Clone, Debug)]
pub(crate) struct TypeRef {
    pub(crate) wit_name: String,
    pub(crate) is_custom: bool,
    pub(crate) path: Vec<String>,
    pub(crate) dependencies: Vec<TypeRef>,
}

impl TypeRef {
    /// Returns true when this type must be imported from the SDK core-types WIT interface.
    pub(crate) fn requires_core_type_import(&self) -> bool {
        !self.is_custom && sdk_core_type_names().contains(&self.wit_name)
    }

    /// Appends all SDK core-types imports referenced by this type.
    pub(crate) fn add_required_core_type_imports(&self, imports: &mut impl Extend<String>) {
        if self.requires_core_type_import() {
            imports.extend([self.wit_name.clone()]);
        }
        for dependency in &self.dependencies {
            dependency.add_required_core_type_imports(imports);
        }
    }

    /// Returns true when this type is an SDK core-type record.
    pub(crate) fn is_sdk_core_record(&self) -> bool {
        !self.is_custom && sdk_core_record_names().contains(&self.wit_name)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedField {
    pub(crate) docs: Vec<String>,
    pub(crate) name: String,
    pub(crate) ty: TypeRef,
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedVariant {
    pub(crate) docs: Vec<String>,
    pub(crate) wit_name: String,
    pub(crate) payload: Option<TypeRef>,
}

#[derive(Clone, Debug)]
pub(crate) enum ExportedTypeKind {
    Record { fields: Vec<ExportedField> },
    Variant { variants: Vec<ExportedVariant> },
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedTypeDef {
    pub(crate) docs: Vec<String>,
    pub(crate) rust_name: String,
    pub(crate) wit_name: String,
    pub(crate) kind: ExportedTypeKind,
}

/// Returns the text stored in `#[doc = "..."]` attributes.
pub(crate) fn doc_comments(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            let syn::Meta::NameValue(meta) = &attr.meta else {
                return None;
            };
            let syn::Expr::Lit(expr) = &meta.value else {
                return None;
            };
            let syn::Lit::Str(value) = &expr.lit else {
                return None;
            };
            Some(value.value())
        })
        .collect()
}

/// Represents the types that can be used as storage fields.
///
/// During macro expansion struct field types correspond to strings, as types haven't been
/// resolved yet. After validating a field type, use this enum instead of strings.
#[derive(Clone, Debug)]
pub(crate) enum StorageFieldType {
    StorageMap,
    StorageValue,
}

/// One exported-type registration together with the source location that produced it.
#[derive(Clone, Debug)]
struct RegisteredExportType {
    def: ExportedTypeDef,
    location: ExpansionLocation,
}

/// Source location of one macro expansion.
///
/// The location tells a stale re-expansion of an edited item (same location) from a real
/// conflict between two items (different locations).
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

/// Registers one exported type while preserving the first definition seen by the macro process.
pub(crate) fn register_export_type(def: ExportedTypeDef, span: Span) -> Result<(), syn::Error> {
    let registry = EXPORTED_TYPES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry.lock().expect("mutex poisoned");
    let entries = registry.entry(macro_invocation_crate_key()).or_default();
    register_export_type_in(entries, def, span, expansion_location(span))
}

/// Applies exported-type identity rules to one registry snapshot.
fn register_export_type_in(
    registry: &mut Vec<RegisteredExportType>,
    def: ExportedTypeDef,
    span: Span,
    location: ExpansionLocation,
) -> Result<(), syn::Error> {
    if let Some(existing) =
        registry.iter_mut().find(|existing| existing.def.wit_name == def.wit_name)
    {
        if existing.def.rust_name == def.rust_name
            && exported_type_shapes_match(&existing.def, &def)
        {
            // rust-analyzer can expand the same attribute more than once in one macro process.
            return Ok(());
        }

        if existing.location == location {
            // A long-lived macro host re-expanded an edited item; replace the stale shape.
            existing.def = def;
            return Ok(());
        }

        let identity = if existing.def.rust_name == def.rust_name {
            format!("Rust type `{}`", def.rust_name)
        } else {
            format!("Rust types `{}` and `{}` both map to", existing.def.rust_name, def.rust_name)
        };
        return Err(syn::Error::new(
            span,
            format!(
                "conflicting #[export_type] registration: {identity} WIT type `{}` with different \
                 identity or shape; the earlier registration is `{}`, while this registration is \
                 `{}`. Rename one type or make both registrations structurally identical. If this \
                 error appears in your IDE after an edit, restart the rust-analyzer proc-macro \
                 server",
                def.wit_name,
                describe_exported_type_shape(&existing.def),
                describe_exported_type_shape(&def),
            ),
        ));
    }
    registry.push(RegisteredExportType { def, location });
    Ok(())
}

/// Returns true when two definitions render the same structural WIT type.
fn exported_type_shapes_match(left: &ExportedTypeDef, right: &ExportedTypeDef) -> bool {
    match (&left.kind, &right.kind) {
        (
            ExportedTypeKind::Record {
                fields: left_fields,
            },
            ExportedTypeKind::Record {
                fields: right_fields,
            },
        ) => {
            left_fields.len() == right_fields.len()
                && left_fields.iter().zip(right_fields).all(|(left, right)| {
                    left.name.to_kebab_case() == right.name.to_kebab_case()
                        && type_ref_shapes_match(&left.ty, &right.ty)
                })
        }
        (
            ExportedTypeKind::Variant {
                variants: left_variants,
            },
            ExportedTypeKind::Variant {
                variants: right_variants,
            },
        ) => {
            left_variants.len() == right_variants.len()
                && left_variants.iter().zip(right_variants).all(|(left, right)| {
                    left.wit_name == right.wit_name
                        && match (&left.payload, &right.payload) {
                            (Some(left), Some(right)) => type_ref_shapes_match(left, right),
                            (None, None) => true,
                            _ => false,
                        }
                })
        }
        _ => false,
    }
}

/// Returns true when two references resolve to the same WIT identity and generic shape.
fn type_ref_shapes_match(left: &TypeRef, right: &TypeRef) -> bool {
    left.wit_name == right.wit_name
        && left.is_custom == right.is_custom
        && left.dependencies.len() == right.dependencies.len()
        && left
            .dependencies
            .iter()
            .zip(&right.dependencies)
            .all(|(left, right)| type_ref_shapes_match(left, right))
}

/// Formats the canonical structural shape of one exported definition.
///
/// The text serves conflict diagnostics and the compile-time shape checks that pin a written
/// type to its `#[export_type]` registration, so it must stay stable and structural.
pub(crate) fn describe_exported_type_shape(def: &ExportedTypeDef) -> String {
    match &def.kind {
        ExportedTypeKind::Record { fields } => format!(
            "record {} {{ {} }}",
            def.wit_name,
            fields
                .iter()
                .map(|field| format!("{}: {}", field.name.to_kebab_case(), field.ty.wit_name))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExportedTypeKind::Variant { variants } => format!(
            "variant {} {{ {} }}",
            def.wit_name,
            variants
                .iter()
                .map(|variant| match &variant.payload {
                    Some(payload) => format!("{}({})", variant.wit_name, payload.wit_name),
                    None => variant.wit_name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Emits nominal identity checks for references classified as SDK core types by their Rust name.
///
/// Procedural macros cannot resolve a bare identifier such as `Word`. The generated check permits
/// a genuine `miden::Word` import but rejects a local same-named type unless it was registered with
/// `#[export_type]`, preventing the emitted WIT shape from drifting from the encoded Rust type.
pub(crate) fn sdk_core_type_identity_guards(
    definition: &ExportedTypeDef,
    span: Span,
) -> Result<TokenStream, syn::Error> {
    let mut guarded = HashSet::new();
    let mut guards = TokenStream::new();
    visit_exported_type_refs(definition, &mut |type_ref| {
        collect_sdk_core_type_identity_guard(type_ref, span, &mut guarded, &mut guards)
    })?;
    Ok(guards)
}

/// Visits every type reference contained in one exported definition.
fn visit_exported_type_refs(
    definition: &ExportedTypeDef,
    visitor: &mut impl FnMut(&TypeRef) -> Result<(), syn::Error>,
) -> Result<(), syn::Error> {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                visit_type_ref_dependencies(&field.ty, visitor)?;
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for payload in variants.iter().filter_map(|variant| variant.payload.as_ref()) {
                visit_type_ref_dependencies(payload, visitor)?;
            }
        }
    }
    Ok(())
}

/// Visits one type reference and every nested generic dependency.
fn visit_type_ref_dependencies(
    type_ref: &TypeRef,
    visitor: &mut impl FnMut(&TypeRef) -> Result<(), syn::Error>,
) -> Result<(), syn::Error> {
    visitor(type_ref)?;
    for dependency in &type_ref.dependencies {
        visit_type_ref_dependencies(dependency, visitor)?;
    }
    Ok(())
}

/// Appends one nominal SDK identity check when a core-type path has not already been guarded.
fn collect_sdk_core_type_identity_guard(
    type_ref: &TypeRef,
    span: Span,
    guarded: &mut HashSet<(String, String)>,
    guards: &mut TokenStream,
) -> Result<(), syn::Error> {
    if !type_ref.requires_core_type_import() {
        return Ok(());
    }

    let rust_path = type_ref.path.join("::");
    if !guarded.insert((rust_path.clone(), type_ref.wit_name.clone())) {
        return Ok(());
    }
    let rust_path = syn::parse_str::<syn::Path>(&rust_path).map_err(|error| {
        syn::Error::new(
            span,
            format!("failed to reconstruct SDK core-type path for an identity check: {error}"),
        )
    })?;
    let sdk_ident = syn::Ident::new(&type_ref.wit_name.to_upper_camel_case(), span);
    guards.extend(quote_spanned! {span=>
        const _: fn() = || {
            fn __miden_core_type_name_collision_use_sdk_type_or_add_export_type<T>(
                _: ::core::marker::PhantomData<T>,
                _: ::core::marker::PhantomData<T>,
            ) {}
            __miden_core_type_name_collision_use_sdk_type_or_add_export_type(
                ::core::marker::PhantomData::<#rust_path>,
                ::core::marker::PhantomData::<::miden::#sdk_ident>,
            );
        };
    });
    Ok(())
}

/// Emits the hidden constant that records the structural shape of an exported type.
///
/// Compile-time checks read this constant through a written type path, so a type that only
/// shares the registered name cannot pass for the registered type.
pub(crate) fn export_type_shape_const(
    def: &ExportedTypeDef,
    generics: &syn::Generics,
    span: Span,
) -> TokenStream {
    let ident = syn::Ident::new(&def.rust_name, span);
    let shape = describe_exported_type_shape(def);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote_spanned! {span=>
        impl #impl_generics #ident #ty_generics #where_clause {
            #[doc(hidden)]
            pub const __MIDEN_EXPORT_TYPE_SHAPE: &'static str = #shape;
        }
    }
}

/// Emits compile-time checks that pin written custom types to their registrations.
///
/// Each check reads the shape constant through the type path as it is written at the
/// expansion site. A type that is not the registered type fails to compile, either because
/// it has no shape constant or because its shape text differs.
pub(crate) fn custom_type_shape_assertions(
    definition: &ExportedTypeDef,
    registry: &HashMap<String, ExportedTypeDef>,
    span: Span,
) -> Result<TokenStream, syn::Error> {
    let mut asserted = HashSet::new();
    let mut checks = TokenStream::new();
    visit_exported_type_refs(definition, &mut |type_ref| {
        collect_custom_type_shape_assertion(type_ref, registry, span, &mut asserted, &mut checks)
    })?;
    Ok(checks)
}

/// Appends one shape check when a custom type path has not already been checked.
fn collect_custom_type_shape_assertion(
    type_ref: &TypeRef,
    registry: &HashMap<String, ExportedTypeDef>,
    span: Span,
    asserted: &mut HashSet<String>,
    checks: &mut TokenStream,
) -> Result<(), syn::Error> {
    if !type_ref.is_custom {
        return Ok(());
    }
    let written_path = type_ref.path.join("::");
    if !asserted.insert(written_path.clone()) {
        return Ok(());
    }
    let Some(rust_name) = type_ref.path.last() else {
        return Ok(());
    };
    let Some(registered) = registry.get(rust_name) else {
        // The schema resolution path reports unregistered custom types with full context.
        return Ok(());
    };
    let expected = describe_exported_type_shape(registered);
    let path = syn::parse_str::<syn::Path>(&written_path).map_err(|error| {
        syn::Error::new(
            span,
            format!("failed to reconstruct the path of custom type `{written_path}`: {error}"),
        )
    })?;
    let message = format!(
        "type `{written_path}` does not match the #[export_type] registration named `{}`; write \
         the registered type here or rename one of the types",
        registered.rust_name
    );
    checks.extend(quote_spanned! {span=>
        const _: () = {
            const fn __miden_shape_text_eq(left: &str, right: &str) -> bool {
                let (left, right) = (left.as_bytes(), right.as_bytes());
                if left.len() != right.len() {
                    return false;
                }
                let mut index = 0;
                while index < left.len() {
                    if left[index] != right[index] {
                        return false;
                    }
                    index += 1;
                }
                true
            }
            assert!(
                __miden_shape_text_eq(<#path>::__MIDEN_EXPORT_TYPE_SHAPE, #expected),
                #message
            );
        };
    });
    Ok(())
}

pub(crate) fn registered_export_types() -> Vec<ExportedTypeDef> {
    let registry = EXPORTED_TYPES.get_or_init(|| Mutex::new(HashMap::new()));
    let registry = registry.lock().expect("mutex poisoned");
    registry
        .get(&macro_invocation_crate_key())
        .map(|entries| entries.iter().map(|entry| entry.def.clone()).collect())
        .unwrap_or_default()
}

pub(crate) fn registered_export_type_map() -> HashMap<String, ExportedTypeDef> {
    registered_export_types()
        .into_iter()
        .map(|def| (def.rust_name.clone(), def))
        .collect()
}

pub(crate) fn map_type_to_type_ref(
    ty: &Type,
    exported_types: &HashMap<String, ExportedTypeDef>,
) -> Result<TypeRef, syn::Error> {
    match ty {
        Type::Reference(reference) => Err(syn::Error::new(
            reference.span(),
            "references are not supported in component interfaces or exported types",
        )),
        Type::Group(group) => map_type_to_type_ref(&group.elem, exported_types),
        Type::Paren(paren) => map_type_to_type_ref(&paren.elem, exported_types),
        Type::Path(path) => {
            let last = path.path.segments.last().ok_or_else(|| {
                syn::Error::new(ty.span(), "unsupported type in component interface")
            })?;
            let ident = last.ident.to_string();
            if ident.is_empty() {
                return Err(syn::Error::new(
                    ty.span(),
                    "unsupported type in component interface; identifier cannot be empty",
                ));
            }

            let path_segments: Vec<String> =
                path.path.segments.iter().map(|segment| segment.ident.to_string()).collect();

            reject_unsupported_component_primitive(&ident, last.span())?;

            if !last.arguments.is_empty() {
                if ident == "Option" {
                    let inner = single_generic_type_argument(last)?;
                    let inner = map_type_to_type_ref(inner, exported_types)?;
                    let wit_name = format!("option<{}>", inner.wit_name);

                    return Ok(TypeRef {
                        wit_name,
                        is_custom: false,
                        path: path_segments,
                        dependencies: vec![inner],
                    });
                }

                if ident == "Result" {
                    let args = generic_type_arguments(last, "Result<T, E>", 2)?;
                    let ok = map_result_argument_type_to_type_ref(args[0], exported_types)?;
                    let err = map_result_argument_type_to_type_ref(args[1], exported_types)?;
                    let wit_name = format!("result<{}, {}>", ok.wit_name, err.wit_name);

                    return Ok(TypeRef {
                        wit_name,
                        is_custom: false,
                        path: path_segments,
                        dependencies: vec![ok, err],
                    });
                }

                return Err(syn::Error::new(
                    last.span(),
                    "generic type arguments are not supported in exported types",
                ));
            }

            let wit_name = ident.to_kebab_case();

            if let Some(wit_type) = rust_type_to_wit_type(&ident) {
                return Ok(TypeRef {
                    wit_name: wit_type_name(wit_type).to_string(),
                    is_custom: false,
                    path: path_segments,
                    dependencies: Vec::new(),
                });
            }

            if exported_types.contains_key(&ident) {
                return Ok(TypeRef {
                    wit_name,
                    is_custom: true,
                    path: path_segments,
                    dependencies: Vec::new(),
                });
            }

            if sdk_core_type_names().contains(&wit_name) {
                return Ok(TypeRef {
                    wit_name,
                    is_custom: false,
                    path: path_segments,
                    dependencies: Vec::new(),
                });
            }

            Ok(TypeRef {
                wit_name,
                is_custom: true,
                path: path_segments,
                dependencies: Vec::new(),
            })
        }
        _ => Err(syn::Error::new(
            ty.span(),
            "unsupported type in component interface; only paths are supported",
        )),
    }
}

/// Returns the single type argument from a supported generic Rust type path segment.
fn single_generic_type_argument(segment: &syn::PathSegment) -> Result<&Type, syn::Error> {
    let args = generic_type_arguments(segment, "Option<T>", 1)?;
    Ok(args[0])
}

/// Returns type arguments from a supported generic Rust type path segment.
fn generic_type_arguments<'a>(
    segment: &'a syn::PathSegment,
    type_name: &str,
    expected_len: usize,
) -> Result<Vec<&'a Type>, syn::Error> {
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new(
            segment.arguments.span(),
            "generic type arguments must be angle-bracketed",
        ));
    };
    if args.args.len() != expected_len {
        let plural = if expected_len == 1 { "" } else { "s" };
        return Err(syn::Error::new(
            args.span(),
            format!("{type_name} must have exactly {expected_len} type argument{plural}"),
        ));
    }
    args.args
        .iter()
        .map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Ok(ty),
            other => Err(syn::Error::new(
                other.span(),
                format!("{type_name} only supports type arguments"),
            )),
        })
        .collect()
}

/// Converts one Rust `Result` type argument into its WIT representation.
fn map_result_argument_type_to_type_ref(
    ty: &Type,
    exported_types: &HashMap<String, ExportedTypeDef>,
) -> Result<TypeRef, syn::Error> {
    match ty {
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(TypeRef {
            wit_name: "_".to_string(),
            is_custom: false,
            path: Vec::new(),
            dependencies: Vec::new(),
        }),
        _ => map_type_to_type_ref(ty, exported_types),
    }
}

/// Rejects Rust primitives that WIT can express but the Wasm frontend cannot lower yet.
fn reject_unsupported_component_primitive(ident: &str, span: Span) -> Result<(), syn::Error> {
    if matches!(ident, "f32" | "f64" | "char") {
        return Err(syn::Error::new(
            span,
            format!("`{ident}` is not supported in component interfaces yet"),
        ));
    }

    Ok(())
}

/// Converts a Rust primitive type identifier into the equivalent WIT primitive type.
///
/// `f32`, `f64`, and `char` intentionally have no mapping; they are rejected earlier by
/// [`reject_unsupported_component_primitive`].
fn rust_type_to_wit_type(ident: &str) -> Option<WitType> {
    match ident {
        "bool" => Some(WitType::Bool),
        "i8" => Some(WitType::S8),
        "u8" => Some(WitType::U8),
        "i16" => Some(WitType::S16),
        "u16" => Some(WitType::U16),
        "i32" => Some(WitType::S32),
        "u32" => Some(WitType::U32),
        "i64" => Some(WitType::S64),
        "u64" => Some(WitType::U64),
        _ => None,
    }
}

/// Returns the canonical WIT syntax for a WIT type produced by [`rust_type_to_wit_type`].
fn wit_type_name(ty: WitType) -> &'static str {
    match ty {
        WitType::Bool => "bool",
        WitType::U8 => "u8",
        WitType::U16 => "u16",
        WitType::U32 => "u32",
        WitType::U64 => "u64",
        WitType::S8 => "s8",
        WitType::S16 => "s16",
        WitType::S32 => "s32",
        WitType::S64 => "s64",
        WitType::F32 | WitType::F64 | WitType::Char | WitType::String | WitType::ErrorContext => {
            unreachable!("`{ty:?}` has no Rust mapping in component interfaces")
        }
        WitType::Id(_) => unreachable!("named WIT type ids are not primitive syntax"),
    }
}

fn sdk_core_type_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| parse_wit_type_names(SDK_WIT_SOURCE))
}

/// Returns the record names declared by the SDK core-types WIT document.
fn sdk_core_record_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        SDK_WIT_SOURCE
            .lines()
            .filter_map(|line| extract_wit_type_name(line.trim_start(), "record"))
            .collect()
    })
}

fn parse_wit_type_names(source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(name) = extract_wit_type_name(trimmed, "record") {
            names.insert(name);
            continue;
        }
        if let Some(name) = extract_wit_type_name(trimmed, "variant") {
            names.insert(name);
            continue;
        }
        if let Some(name) = extract_wit_type_name(trimmed, "enum") {
            names.insert(name);
            continue;
        }
        if let Some(name) = extract_wit_type_name(trimmed, "flags") {
            names.insert(name);
            continue;
        }
        if let Some(name) = extract_wit_type_name(trimmed, "resource") {
            names.insert(name);
            continue;
        }
        if let Some(name) = extract_wit_type_name(trimmed, "type") {
            names.insert(name);
            continue;
        }
    }
    names
}

fn extract_wit_type_name(line: &str, keyword: &str) -> Option<String> {
    let prefix = format!("{keyword} ");
    let rest = line.strip_prefix(&prefix)?;
    let mut name = String::new();
    for ch in rest.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            name.push(ch);
        } else {
            break;
        }
    }
    if name.is_empty() { None } else { Some(name) }
}

pub(crate) fn exported_type_from_struct(
    item_struct: &ItemStruct,
) -> Result<ExportedTypeDef, syn::Error> {
    let known_exported = registered_export_type_map();
    match &item_struct.fields {
        syn::Fields::Named(named) => {
            let mut fields = Vec::new();
            for field in &named.named {
                let field_ident = field.ident.as_ref().ok_or_else(|| {
                    syn::Error::new(field.span(), "exported type fields must be named")
                })?;
                let field_ty = map_type_to_type_ref(&field.ty, &known_exported)?;
                fields.push(ExportedField {
                    docs: doc_comments(&field.attrs),
                    name: field_ident.to_string(),
                    ty: field_ty,
                });
            }

            Ok(ExportedTypeDef {
                docs: doc_comments(&item_struct.attrs),
                rust_name: item_struct.ident.to_string(),
                wit_name: item_struct.ident.to_string().to_kebab_case(),
                kind: ExportedTypeKind::Record { fields },
            })
        }
        syn::Fields::Unit => Ok(ExportedTypeDef {
            docs: doc_comments(&item_struct.attrs),
            rust_name: item_struct.ident.to_string(),
            wit_name: item_struct.ident.to_string().to_kebab_case(),
            kind: ExportedTypeKind::Record { fields: Vec::new() },
        }),
        syn::Fields::Unnamed(_) => Err(syn::Error::new(
            item_struct.ident.span(),
            "tuple structs are not supported by #[export_type]",
        )),
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn exported_type_from_enum(
    item_enum: &syn::ItemEnum,
) -> Result<ExportedTypeDef, syn::Error> {
    let known_exported = registered_export_type_map();
    let mut variants = Vec::new();
    for variant in &item_enum.variants {
        let wit_name = variant.ident.to_string().to_kebab_case();
        let payload = match &variant.fields {
            syn::Fields::Unit => None,
            syn::Fields::Unnamed(fields) => {
                if fields.unnamed.len() != 1 {
                    return Err(syn::Error::new(
                        fields.span(),
                        "tuple variants in #[export_type] enums must have exactly one field",
                    ));
                }
                let field_ty = &fields.unnamed[0].ty;
                let type_ref = map_type_to_type_ref(field_ty, &known_exported)?;
                Some(type_ref)
            }
            syn::Fields::Named(named) => {
                return Err(syn::Error::new(
                    named.span(),
                    "struct variants are not supported by #[export_type]",
                ));
            }
        };

        variants.push(ExportedVariant {
            docs: doc_comments(&variant.attrs),
            wit_name,
            payload,
        });
    }

    Ok(ExportedTypeDef {
        docs: doc_comments(&item_enum.attrs),
        rust_name: item_enum.ident.to_string(),
        wit_name: item_enum.ident.to_string().to_kebab_case(),
        kind: ExportedTypeKind::Variant { variants },
    })
}

pub(crate) fn ensure_custom_type_defined(
    type_ref: &TypeRef,
    exported_type_names: &HashSet<String>,
    span: Span,
) -> Result<(), syn::Error> {
    if type_ref.is_custom && !exported_type_names.contains(&type_ref.wit_name) {
        let rust_name = type_ref
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| type_ref.wit_name.replace('-', "::"));
        return Err(syn::Error::new(
            span,
            format!(
                "type `{rust_name}` is used in the exported interface but is not exported; add \
                 #[export_type] to its definition"
            ),
        ));
    }
    for dependency in &type_ref.dependencies {
        ensure_custom_type_defined(dependency, exported_type_names, span)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_export_type_registry_for_tests() {
    if let Some(registry) = EXPORTED_TYPES.get() {
        registry.lock().expect("mutex poisoned").clear();
    }
}
