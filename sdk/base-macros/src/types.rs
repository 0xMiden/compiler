use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use heck::ToKebabCase;
use proc_macro2::Span;
use syn::{spanned::Spanned, ItemStruct, Type};

#[derive(Clone, Debug)]
pub(crate) struct TypeRef {
    pub(crate) wit_name: String,
    pub(crate) is_custom: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedField {
    pub(crate) name: String,
    pub(crate) ty: TypeRef,
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedTypeDef {
    pub(crate) rust_name: String,
    pub(crate) wit_name: String,
    pub(crate) fields: Vec<ExportedField>,
}

static EXPORTED_TYPES: OnceLock<Mutex<Vec<ExportedTypeDef>>> = OnceLock::new();

fn exported_types_registry() -> &'static Mutex<Vec<ExportedTypeDef>> {
    EXPORTED_TYPES.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn register_export_type(def: ExportedTypeDef, span: Span) -> Result<(), syn::Error> {
    let mut registry = exported_types_registry().lock().expect("mutex poisoned");

    if registry.iter().any(|existing| existing.wit_name == def.wit_name) {
        return Err(syn::Error::new(
            span,
            format!("duplicate exported type '{}'; names must be unique", def.rust_name),
        ));
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
        Type::Reference(reference) => map_type_to_type_ref(&reference.elem, exported_types),
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

            if exported_types.contains_key(&ident) {
                Ok(TypeRef {
                    wit_name: ident.to_kebab_case(),
                    is_custom: true,
                })
            } else {
                Ok(TypeRef {
                    wit_name: ident.to_kebab_case(),
                    is_custom: false,
                })
            }
        }
        _ => Err(syn::Error::new(
            ty.span(),
            "unsupported type in component interface; only paths are supported",
        )),
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
                fields,
            })
        }
        syn::Fields::Unit => Ok(ExportedTypeDef {
            rust_name: item_struct.ident.to_string(),
            wit_name: item_struct.ident.to_string().to_kebab_case(),
            fields: Vec::new(),
        }),
        syn::Fields::Unnamed(_) => Err(syn::Error::new(
            item_struct.ident.span(),
            "tuple structs are not supported by #[export_type]",
        )),
    }
}
