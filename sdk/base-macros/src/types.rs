use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

static EXPORTED_TYPES: OnceLock<Mutex<Vec<ExportedTypeDef>>> = OnceLock::new();

use heck::ToKebabCase;
use proc_macro2::Span;
use syn::{ItemStruct, Type, spanned::Spanned};
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
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedField {
    pub(crate) name: String,
    pub(crate) ty: TypeRef,
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedVariant {
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
    pub(crate) rust_name: String,
    pub(crate) wit_name: String,
    pub(crate) kind: ExportedTypeKind,
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

pub(crate) fn register_export_type(def: ExportedTypeDef, _span: Span) -> Result<(), syn::Error> {
    let registry = EXPORTED_TYPES.get_or_init(|| Mutex::new(Vec::new()));
    let mut registry = registry.lock().expect("mutex poisoned");
    if let Some(existing) = registry.iter_mut().find(|existing| existing.wit_name == def.wit_name) {
        *existing = def;
        return Ok(());
    }
    registry.push(def);
    Ok(())
}

pub(crate) fn registered_export_types() -> Vec<ExportedTypeDef> {
    let registry = EXPORTED_TYPES.get_or_init(|| Mutex::new(Vec::new()));
    registry.lock().expect("mutex poisoned").clone()
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
    if matches!(ident, "f64" | "char") {
        return Err(syn::Error::new(
            span,
            format!("`{ident}` is not supported in component interfaces yet"),
        ));
    }

    Ok(())
}

/// Converts a Rust primitive type identifier into the equivalent WIT primitive type.
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
        "f32" => Some(WitType::F32),
        _ => None,
    }
}

/// Returns the canonical WIT syntax for a WIT type.
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
        WitType::F32 => "f32",
        WitType::F64 => "f64",
        WitType::Char => "char",
        WitType::String => "string",
        WitType::ErrorContext => "error-context",
        WitType::Id(_) => unreachable!("named WIT type ids are not primitive syntax"),
    }
}

fn sdk_core_type_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| parse_wit_type_names(SDK_WIT_SOURCE))
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
                    name: field_ident.to_string(),
                    ty: field_ty,
                });
            }

            Ok(ExportedTypeDef {
                rust_name: item_struct.ident.to_string(),
                wit_name: item_struct.ident.to_string().to_kebab_case(),
                kind: ExportedTypeKind::Record { fields },
            })
        }
        syn::Fields::Unit => Ok(ExportedTypeDef {
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

        variants.push(ExportedVariant { wit_name, payload });
    }

    Ok(ExportedTypeDef {
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
