use std::sync::{Mutex, OnceLock};

use heck::ToKebabCase;
use proc_macro2::Span;
use syn::{spanned::Spanned, ItemStruct, Type};

#[derive(Clone, Debug)]
pub(crate) enum WitType {
    Core(String),
    Custom(String),
}

impl WitType {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            WitType::Core(name) | WitType::Custom(name) => name,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExportedField {
    pub(crate) name: String,
    pub(crate) ty: WitType,
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

const CORE_TYPE_MAPPINGS: &[(&str, &str)] = &[
    ("AccountCodeRoot", "account-code-root"),
    ("AccountHash", "account-hash"),
    ("AccountId", "account-id"),
    ("Asset", "asset"),
    ("BlockHash", "block-hash"),
    ("Digest", "digest"),
    ("Felt", "felt"),
    ("NoteExecutionHint", "note-execution-hint"),
    ("NoteIdx", "note-idx"),
    ("NoteType", "note-type"),
    ("Nonce", "nonce"),
    ("Recipient", "recipient"),
    ("StorageRoot", "storage-root"),
    ("StorageValue", "storage-value"),
    ("Tag", "tag"),
    ("VaultCommitment", "vault-commitment"),
    ("Word", "word"),
];

pub(crate) fn map_type_to_wit_type(ty: &Type) -> Result<WitType, syn::Error> {
    match ty {
        Type::Reference(reference) => map_type_to_wit_type(&reference.elem),
        Type::Group(group) => map_type_to_wit_type(&group.elem),
        Type::Paren(paren) => map_type_to_wit_type(&paren.elem),
        Type::Path(path) => {
            if let Some(last) = path.path.segments.last() {
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

                if let Some((_, wit)) =
                    CORE_TYPE_MAPPINGS.iter().find(|(core_ident, _)| *core_ident == ident)
                {
                    Ok(WitType::Core((*wit).to_string()))
                } else {
                    Ok(WitType::Custom(ident.to_kebab_case()))
                }
            } else {
                Err(syn::Error::new(ty.span(), "unsupported type in component interface"))
            }
        }
        _ => Err(syn::Error::new(
            ty.span(),
            format!(
                "unsupported type `{}` in component interface; only paths are supported",
                quote::ToTokens::to_token_stream(ty)
            ),
        )),
    }
}

pub(crate) fn exported_type_from_struct(
    item_struct: &ItemStruct,
) -> Result<ExportedTypeDef, syn::Error> {
    match &item_struct.fields {
        syn::Fields::Named(named) => {
            let mut fields = Vec::new();
            for field in &named.named {
                let field_ident = field.ident.as_ref().ok_or_else(|| {
                    syn::Error::new(field.span(), "exported type fields must be named")
                })?;
                let field_ty = map_type_to_wit_type(&field.ty)?;
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
