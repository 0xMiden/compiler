use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use heck::ToKebabCase;
use proc_macro2::Span;
use syn::{spanned::Spanned, ItemStruct, Type};

use crate::generate::SDK_WIT_SOURCE;

#[derive(Clone, Debug)]
pub(crate) struct TypeRef {
    pub(crate) wit_name: String,
    pub(crate) is_custom: bool,
    pub(crate) path: Vec<String>,
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

static EXPORTED_TYPES: OnceLock<Mutex<Vec<ExportedTypeDef>>> = OnceLock::new();

fn exported_types_registry() -> &'static Mutex<Vec<ExportedTypeDef>> {
    EXPORTED_TYPES.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn register_export_type(def: ExportedTypeDef, _span: Span) -> Result<(), syn::Error> {
    let mut registry = exported_types_registry().lock().expect("mutex poisoned");

    if let Some(existing) = registry.iter_mut().find(|existing| existing.wit_name == def.wit_name) {
        *existing = def;
        return Ok(());
    }

    registry.push(def);
    Ok(())
}

pub(crate) fn registered_export_types() -> Vec<ExportedTypeDef> {
    exported_types_registry().lock().expect("mutex poisoned").clone()
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

            if !last.arguments.is_empty() {
                return Err(syn::Error::new(
                    last.span(),
                    "generic type arguments are not supported in exported types",
                ));
            }

            let ident = last.ident.to_string();
            if ident.is_empty() {
                return Err(syn::Error::new(
                    ty.span(),
                    "unsupported type in component interface; identifier cannot be empty",
                ));
            }

            let path_segments: Vec<String> =
                path.path.segments.iter().map(|segment| segment.ident.to_string()).collect();

            if exported_types.contains_key(&ident) {
                Ok(TypeRef {
                    wit_name: ident.to_kebab_case(),
                    is_custom: true,
                    path: path_segments,
                })
            } else {
                let wit_name = ident.to_kebab_case();
                if !sdk_core_type_names().contains(&wit_name) {
                    return Err(syn::Error::new(
                        ty.span(),
                        format!(
                            "type `{ident}` is not a known Miden SDK type; add #[export_type] to \
                             its definition to export it from this component"
                        ),
                    ));
                }
                Ok(TypeRef {
                    wit_name: ident.to_kebab_case(),
                    is_custom: false,
                    path: path_segments,
                })
            }
        }
        _ => Err(syn::Error::new(
            ty.span(),
            "unsupported type in component interface; only paths are supported",
        )),
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
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
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
