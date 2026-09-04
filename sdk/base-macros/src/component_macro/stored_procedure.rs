//! Stored-procedure storage slots for the `#[component_storage]` expansion.
//!
//! A component may keep the MAST root of a procedure exported by a *sibling* component (another
//! component deployed on the same account) in one of its storage slots and call it. The slot is
//! declared as `StorageValue<StoredProcedure<fn(..) -> R>>`; the `fn` type is not a Rust function
//! pointer but the way the call signature is spelled.
//!
//! Each such field expands into three things: a marker type sealing the signature to that one
//! slot, a per-slot trait providing the typed `call` method, and one entry in a hidden
//! wit-bindgen bindings module shared by all stored-procedure slots of the storage struct. The
//! generated import is named `dyncall-<field>` and takes the procedure root as its leading `word`
//! parameter, which the Wasm frontend lowers to a dynamic call in a new VM context — nothing has
//! to be linked or resolved for it, so no dependency package is consulted here.

use std::collections::BTreeSet;

use heck::{ToKebabCase, ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use semver::Version;
use syn::{Error, FieldsNamed, ReturnType, Type, Visibility, spanned::Spanned};

use crate::{
    component_macro::CORE_TYPES_PACKAGE,
    fpi, generate, manifest_paths,
    types::{TypeRef, map_type_to_type_ref, registered_export_type_map},
    wit_builder::WitBuilder,
};

/// Name of the inline WIT world generated for stored-procedure slots.
const STORED_PROCEDURE_BINDINGS_WORLD: &str = "stored-procedure-bindings";
/// WIT package name of the generated inline world.
const STORED_PROCEDURE_BINDINGS_PACKAGE: &str = generate::STORED_PROCEDURE_BINDINGS_PACKAGE;
/// Prefix of the generated import functions recognized by the Wasm frontend, without the
/// separator: it is joined with `-` in WIT names and with `_` in Rust names.
const DYNCALL_PREFIX: &str = "dyncall";
/// Name synthesized for unnamed signature parameters, suffixed with the parameter index.
const UNNAMED_PARAM_PREFIX: &str = "arg";
/// WIT name of the leading procedure-root parameter of every generated import.
const PROC_ROOT_PARAM: &str = "proc-root";
/// WIT name of the core `word` type carrying a procedure root.
const WORD_WIT_TYPE: &str = "word";
/// Rust type name of a typed storage value slot.
const STORAGE_VALUE: &str = "StorageValue";
/// Rust type name of a stored sibling procedure root.
const STORED_PROCEDURE: &str = "StoredProcedure";

/// Diagnostic emitted for a `StoredProcedure` whose type argument is not a bare `fn` type.
const SIGNATURE_SHAPE_ERROR: &str =
    "stored procedure slots must spell their signature as `StoredProcedure<fn(..) -> R>`";

/// One parameter of a stored-procedure signature.
struct StoredProcedureParam {
    /// Rust identifier used in the generated `call` method (`argN` when the user left it out).
    ident: Ident,
    /// Parameter name rendered in the generated WIT signature.
    wit_name: String,
    /// Parameter type exactly as the user spelled it.
    user_ty: Type,
    /// WIT mapping of the parameter type.
    type_ref: TypeRef,
}

/// A `StorageValue<StoredProcedure<fn(..) -> R>>` field of a `#[component_storage]` struct.
pub(super) struct StoredProcedureSlot {
    /// Field identifier, e.g. `authority`.
    field_ident: Ident,
    /// Generated marker type sealing the signature, e.g. `AuthoritySignature`.
    marker_ident: Ident,
    /// Generated trait carrying the typed call, e.g. `AuthorityCall`.
    trait_ident: Ident,
    /// Identifier of the generated import function, e.g. `dyncall_authority`.
    import_fn_ident: Ident,
    /// Name of the generated import function in WIT, e.g. `dyncall-authority`.
    wit_fn_name: String,
    /// Call parameters, in declaration order.
    params: Vec<StoredProcedureParam>,
    /// Result of the call, absent for a unit return.
    result: Option<(Type, TypeRef)>,
}

/// Detects the stored-procedure slots of a storage struct and rewrites their field types.
///
/// The `fn(..)` signature is replaced in place by the generated marker type, so everything
/// downstream (`typecheck_storage_field`, the metadata schema builder, rustc) sees the ordinary
/// `StorageValue<StoredProcedure<AuthoritySignature>>` path type. All field diagnostics are
/// collected before the first one is returned, matching `process_storage_fields`.
pub(super) fn collect_stored_procedure_slots(
    fields: &mut FieldsNamed,
) -> Result<Vec<StoredProcedureSlot>, Error> {
    let mut slots = Vec::new();
    let mut errors = Vec::new();

    for field in fields.named.iter_mut() {
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        let Some(signature_arg) = stored_procedure_signature_arg_mut(&mut field.ty) else {
            continue;
        };

        let signature = signature_arg.clone();
        match build_slot(&field_ident, &signature) {
            Ok(slot) => {
                let marker_ident = slot.marker_ident.clone();
                *signature_arg = syn::parse_quote!(#marker_ident);
                slots.push(slot);
            }
            Err(err) => errors.push(err),
        }
    }

    match errors.into_iter().next() {
        Some(err) => Err(err),
        None => Ok(slots),
    }
}

/// Returns true when `ty` mentions `StoredProcedure` anywhere in its spelling.
///
/// Used to reject the type where it is not supported; like the rest of the storage type checks
/// this is purely textual, since types are not resolved during macro expansion.
pub(crate) fn mentions_stored_procedure(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => type_path.path.segments.iter().any(|segment| {
            if segment.ident == STORED_PROCEDURE {
                return true;
            }
            match &segment.arguments {
                syn::PathArguments::AngleBracketed(args) => args.args.iter().any(|arg| match arg {
                    syn::GenericArgument::Type(ty) => mentions_stored_procedure(ty),
                    _ => false,
                }),
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(mentions_stored_procedure)
                        || matches!(&args.output, ReturnType::Type(_, ty)
                            if mentions_stored_procedure(ty))
                }
                syn::PathArguments::None => false,
            }
        }),
        Type::Group(group) => mentions_stored_procedure(&group.elem),
        Type::Paren(paren) => mentions_stored_procedure(&paren.elem),
        Type::Reference(reference) => mentions_stored_procedure(&reference.elem),
        Type::Ptr(ptr) => mentions_stored_procedure(&ptr.elem),
        Type::Slice(slice) => mentions_stored_procedure(&slice.elem),
        Type::Array(array) => mentions_stored_procedure(&array.elem),
        Type::Tuple(tuple) => tuple.elems.iter().any(mentions_stored_procedure),
        _ => false,
    }
}

/// Renders the items generated for the stored-procedure slots of one storage struct.
///
/// Emits the shared hidden bindings module holding the wit-bindgen imports, and per slot the
/// sealed marker type plus the trait providing the typed `call` on
/// `StoredProcedure<Marker>`. Returns an empty token stream when the struct has no such slot, so
/// storage structs that do not use the feature never run bindings generation.
pub(super) fn expand_stored_procedure_slots(
    struct_ident: &Ident,
    struct_vis: &Visibility,
    slots: &[StoredProcedureSlot],
) -> Result<TokenStream2, Error> {
    if slots.is_empty() {
        return Ok(TokenStream2::new());
    }
    for slot in slots {
        for generated in [&slot.marker_ident, &slot.trait_ident] {
            if generated == struct_ident {
                return Err(Error::new(
                    slot.field_ident.span(),
                    format!(
                        "stored procedure slot `{}` generates the item `{generated}`, which \
                         collides with the storage struct; rename the field or the struct",
                        slot.field_ident
                    ),
                ));
            }
        }
    }

    let inline_wit = build_stored_procedure_wit(struct_ident, slots);
    let wit_config = manifest_paths::resolve_wit_paths(manifest_paths::ResolveOptions {
        allow_missing_local_wit: true,
    })?;
    let bindings = generate::generate_inline_import_bindings(
        &wit_config,
        &inline_wit,
        STORED_PROCEDURE_BINDINGS_WORLD,
        &[],
    )?;

    let file: syn::File = syn::parse2(bindings)?;
    let modules = fpi::collect_import_modules(&file.items, &fpi::is_plain_import_function)?;
    let bindings_module_ident = bindings_module_ident(struct_ident);

    let mut items = Vec::with_capacity(slots.len());
    for slot in slots {
        let module = modules
            .iter()
            .find(|module| {
                module.functions.iter().any(|func| func.sig.ident == slot.import_fn_ident)
            })
            .ok_or_else(|| {
                Error::new(
                    slot.field_ident.span(),
                    format!(
                        "generated stored-procedure bindings are missing the import `{}`",
                        slot.wit_fn_name
                    ),
                )
            })?;
        items.push(build_slot_items(slot, struct_vis, &bindings_module_ident, &module.module_path));
    }

    let bindings_tokens = file.into_token_stream();
    Ok(quote! {
        #[doc(hidden)]
        #[allow(dead_code)]
        pub mod #bindings_module_ident {
            #bindings_tokens
        }

        #(#items)*
    })
}

/// Builds the descriptor of one stored-procedure slot from the signature the user spelled.
fn build_slot(field_ident: &Ident, signature: &Type) -> Result<StoredProcedureSlot, Error> {
    let bare_fn = bare_fn_signature(signature)?;
    let exported_types = registered_export_type_map();

    // `_` is an unnamed parameter as far as the generated names are concerned
    let explicit_name = |input: &syn::BareFnArg| {
        input.name.as_ref().map(|(ident, _)| ident.clone()).filter(|ident| ident != "_")
    };
    let explicit_names = bare_fn.inputs.iter().filter_map(explicit_name).collect::<Vec<_>>();

    let mut params = Vec::with_capacity(bare_fn.inputs.len());
    for (index, input) in bare_fn.inputs.iter().enumerate() {
        let ident = match explicit_name(input) {
            Some(ident) => ident,
            None => {
                let ident = format_ident!("{UNNAMED_PARAM_PREFIX}{index}", span = input.ty.span());
                if explicit_names.contains(&ident) {
                    return Err(Error::new(
                        input.ty.span(),
                        format!(
                            "unnamed stored procedure parameter #{index} would be named \
                             `{ident}`, which another parameter already uses; name it explicitly"
                        ),
                    ));
                }
                ident
            }
        };
        let type_ref = map_type_to_type_ref(&input.ty, &exported_types)?;
        reject_custom_type(&type_ref, input.ty.span())?;
        params.push(StoredProcedureParam {
            wit_name: ident.to_string().to_kebab_case(),
            ident,
            user_ty: input.ty.clone(),
            type_ref,
        });
    }

    let result = match &bare_fn.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) if is_unit_type(ty) => None,
        ReturnType::Type(_, ty) => {
            let type_ref = map_type_to_type_ref(ty, &exported_types)?;
            reject_custom_type(&type_ref, ty.span())?;
            Some(((**ty).clone(), type_ref))
        }
    };

    let field_name = field_ident.to_string();
    let camel_name = field_name.to_upper_camel_case();
    Ok(StoredProcedureSlot {
        marker_ident: format_ident!("{}Signature", camel_name, span = field_ident.span()),
        trait_ident: format_ident!("{}Call", camel_name, span = field_ident.span()),
        import_fn_ident: format_ident!(
            "{}_{}",
            DYNCALL_PREFIX,
            field_name.to_snake_case(),
            span = field_ident.span()
        ),
        wit_fn_name: format!("{DYNCALL_PREFIX}-{}", field_name.to_kebab_case()),
        field_ident: field_ident.clone(),
        params,
        result,
    })
}

/// Validates that a stored-procedure signature is a plain bare `fn` type and returns it.
fn bare_fn_signature(signature: &Type) -> Result<&syn::TypeBareFn, Error> {
    let Type::BareFn(bare_fn) = signature else {
        return Err(Error::new(
            signature.span(),
            format!("{SIGNATURE_SHAPE_ERROR}; found `{}`", signature.to_token_stream()),
        ));
    };

    let unsupported = if bare_fn.lifetimes.is_some() {
        Some("a `for<..>` lifetime binder")
    } else if bare_fn.unsafety.is_some() {
        Some("`unsafe`")
    } else if bare_fn.abi.is_some() {
        Some("an explicit ABI")
    } else if bare_fn.variadic.is_some() {
        Some("a variadic parameter")
    } else {
        None
    };

    match unsupported {
        Some(found) => {
            Err(Error::new(signature.span(), format!("{SIGNATURE_SHAPE_ERROR}; found {found}")))
        }
        None => Ok(bare_fn),
    }
}

/// Rejects signature types that are not Miden core types or WIT primitives, including custom
/// types wrapped in `Option` or `Result`.
fn reject_custom_type(type_ref: &TypeRef, span: proc_macro2::Span) -> Result<(), Error> {
    if type_ref.is_custom {
        return Err(Error::new(
            span,
            "stored procedure signatures support only Miden core types and primitives",
        ));
    }
    type_ref
        .dependencies
        .iter()
        .try_for_each(|dependency| reject_custom_type(dependency, span))
}

/// Returns true for the unit type `()`.
fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Renders the inline WIT world declaring one `dyncall-<field>` import per stored-procedure slot.
fn build_stored_procedure_wit(struct_ident: &Ident, slots: &[StoredProcedureSlot]) -> String {
    let interface_name = struct_ident.to_string().to_kebab_case();

    // The procedure root is the leading parameter of every generated import, so `word` is always
    // imported regardless of what the signatures themselves need.
    let mut core_imports = BTreeSet::from([WORD_WIT_TYPE.to_string()]);
    for slot in slots {
        for param in &slot.params {
            param.type_ref.add_required_core_type_imports(&mut core_imports);
        }
        if let Some((_, type_ref)) = &slot.result {
            type_ref.add_required_core_type_imports(&mut core_imports);
        }
    }

    let mut wit = WitBuilder::new(
        "#[component_storage]",
        STORED_PROCEDURE_BINDINGS_PACKAGE,
        &Version::new(1, 0, 0),
    );
    wit.use_path(CORE_TYPES_PACKAGE);
    wit.blank_line();
    wit.interface(&interface_name, |interface| {
        let imports = core_imports.iter().cloned().collect::<Vec<_>>().join(", ");
        interface.line(&format!("use core-types.{{{imports}}};"));
        for slot in slots {
            interface.line(&stored_procedure_wit_signature(slot));
        }
    });
    wit.blank_line();
    wit.world(STORED_PROCEDURE_BINDINGS_WORLD, |world| {
        world.line(&format!("import {interface_name};"));
    });

    wit.finish()
}

/// Renders the WIT function signature of one generated stored-procedure import.
fn stored_procedure_wit_signature(slot: &StoredProcedureSlot) -> String {
    let mut params = vec![format!("{PROC_ROOT_PARAM}: {WORD_WIT_TYPE}")];
    params.extend(
        slot.params
            .iter()
            .map(|param| format!("{}: {}", param.wit_name, param.type_ref.wit_name)),
    );
    let params = params.join(", ");

    match &slot.result {
        Some((_, type_ref)) => {
            format!("{}: func({params}) -> {};", slot.wit_fn_name, type_ref.wit_name)
        }
        None => format!("{}: func({params});", slot.wit_fn_name),
    }
}

/// Builds the marker type and the call trait generated for one stored-procedure slot.
fn build_slot_items(
    slot: &StoredProcedureSlot,
    struct_vis: &Visibility,
    bindings_module_ident: &Ident,
    call_module_path: &[Ident],
) -> TokenStream2 {
    let StoredProcedureSlot {
        field_ident,
        marker_ident,
        trait_ident,
        import_fn_ident,
        ..
    } = slot;

    let param_idents = slot.params.iter().map(|param| &param.ident).collect::<Vec<_>>();
    let param_tys = slot.params.iter().map(|param| &param.user_ty).collect::<Vec<_>>();
    let result_ty = slot.result.as_ref().map(|(ty, _)| ty);
    let output = match result_ty {
        Some(ty) => quote!(-> #ty),
        None => quote!(),
    };

    let mut call_path = quote!(#bindings_module_ident);
    for ident in call_module_path {
        call_path = quote!(#call_path::#ident);
    }
    // The procedure root is passed as the leading argument; the frontend takes it off the import's
    // parameter list and turns the call into a dynamic call into a new VM context.
    let call = quote!(#call_path::#import_fn_ident(self.root() #(, #param_idents)*));
    // A unit-returning call must be discarded, otherwise the body's type is the import's.
    let body = match result_ty {
        Some(_) => quote!({ #call }),
        None => quote!({ #call; }),
    };

    let marker_doc = format!(
        "Signature marker for the `{field_ident}` stored-procedure storage slot.\n\nGenerated by \
         `#[component_storage]`; it seals the slot's [`StoredProcedure`] to the call signature \
         declared on the field."
    );
    let call_doc = format!(
        "Calls the procedure whose root is stored in the `{field_ident}` slot.\n\nFails the \
         transaction when the slot is unset."
    );
    let trait_doc = format!(
        "Typed call into the procedure whose root is stored in the `{field_ident}` storage \
         slot.\n\nThe procedure runs in a new VM context on the account this component is \
         deployed on. The root is set from off-chain code; a root that does not match the \
         declared signature fails the transaction or yields wrong results."
    );

    quote! {
        #[doc = #marker_doc]
        #struct_vis struct #marker_ident;

        impl ::miden::__stored_procedure_sealed::Sealed for #marker_ident {}
        impl ::miden::ProcedureSignature for #marker_ident {}

        #[doc = #trait_doc]
        #struct_vis trait #trait_ident {
            #[doc = #call_doc]
            fn call(&self #(, #param_idents: #param_tys)*) #output;
        }

        impl #trait_ident for ::miden::StoredProcedure<#marker_ident> {
            #[inline(always)]
            fn call(&self #(, #param_idents: #param_tys)*) #output #body
        }
    }
}

/// Builds the name of the hidden module holding the stored-procedure bindings of a storage struct.
fn bindings_module_ident(struct_ident: &Ident) -> Ident {
    format_ident!("__miden_stored_procedure_bindings_{}", struct_ident.to_string().to_snake_case())
}

/// Returns the signature type argument of a `StorageValue<StoredProcedure<..>>` field type.
///
/// Like the other storage type checks this matches the written spelling (the last path segment),
/// so `miden::StorageValue<..>` and a bare `StorageValue<..>` are both recognized.
fn stored_procedure_signature_arg_mut(ty: &mut Type) -> Option<&mut Type> {
    let segment = last_path_segment_mut(ty, STORAGE_VALUE)?;
    let value_ty = single_type_argument_mut(segment)?;
    let segment = last_path_segment_mut(value_ty, STORED_PROCEDURE)?;
    single_type_argument_mut(segment)
}

/// Returns the last segment of a path type when it is named `name`.
fn last_path_segment_mut<'a>(ty: &'a mut Type, name: &str) -> Option<&'a mut syn::PathSegment> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    let segment = type_path.path.segments.last_mut()?;
    (segment.ident == name).then_some(segment)
}

/// Returns the single angle-bracketed type argument of a path segment.
fn single_type_argument_mut(segment: &mut syn::PathSegment) -> Option<&mut Type> {
    let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match args.args.first_mut()? {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    /// Parses the named fields of a storage struct body.
    fn fields(tokens: TokenStream2) -> FieldsNamed {
        syn::parse2(tokens).expect("test fields must parse")
    }

    /// Collects the slots of `fields`, expecting a diagnostic, and returns its message.
    fn collect_error(fields: &mut FieldsNamed) -> String {
        match collect_stored_procedure_slots(fields) {
            Ok(_) => panic!("expected a stored-procedure diagnostic"),
            Err(err) => err.to_string(),
        }
    }

    /// Collects the slots of the two example fields used across the rendering tests.
    fn example_fields() -> FieldsNamed {
        fields(quote! {
            {
                authority: StorageValue<StoredProcedure<fn(role: Felt, caller: AccountId) -> bool>>,
                hook: StorageValue<StoredProcedure<fn()>>,
            }
        })
    }

    #[test]
    fn renders_the_inline_wit_world_for_all_slots() {
        let mut fields = example_fields();
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();
        let wit = build_stored_procedure_wit(&format_ident!("AuthorityStorage"), &slots);

        let expected = r#"// This file is auto-generated by the `#[component_storage]` macro.
// Do not edit this file manually.

package miden:stored-procedure-bindings@1.0.0;

use miden:base/core-types@1.0.0;

interface authority-storage {
    use core-types.{account-id, felt, word};
    dyncall-authority: func(proc-root: word, role: felt, caller: account-id) -> bool;
    dyncall-hook: func(proc-root: word);
}

world stored-procedure-bindings {
    import authority-storage;
}
"#;

        assert_eq!(wit, expected);
    }

    #[test]
    fn derives_generated_names_from_the_field_name() {
        let mut fields = example_fields();
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();

        assert_eq!(slots[0].marker_ident.to_string(), "AuthoritySignature");
        assert_eq!(slots[0].trait_ident.to_string(), "AuthorityCall");
        assert_eq!(slots[0].import_fn_ident.to_string(), "dyncall_authority");
        assert_eq!(slots[0].wit_fn_name, "dyncall-authority");
        assert_eq!(
            bindings_module_ident(&format_ident!("AuthorityStorage")).to_string(),
            "__miden_stored_procedure_bindings_authority_storage"
        );
    }

    #[test]
    fn rewrites_the_field_type_to_the_marker_and_keeps_the_outer_spelling() {
        let mut fields = fields(quote! {
            {
                authority: miden::StorageValue<miden::StoredProcedure<fn(role: Felt) -> bool>>,
            }
        });
        collect_stored_procedure_slots(&mut fields).unwrap();

        let rendered = fields.named[0].ty.to_token_stream().to_string();
        assert_eq!(
            rendered,
            "miden :: StorageValue < miden :: StoredProcedure < AuthoritySignature > >"
        );
    }

    #[test]
    fn names_unnamed_signature_parameters_by_position() {
        let mut fields = fields(quote! {
            {
                hook: StorageValue<StoredProcedure<fn(Felt, u32)>>,
            }
        });
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();

        let names: Vec<String> =
            slots[0].params.iter().map(|param| param.ident.to_string()).collect();
        assert_eq!(names, vec!["arg0", "arg1"]);
        assert_eq!(
            stored_procedure_wit_signature(&slots[0]),
            "dyncall-hook: func(proc-root: word, arg0: felt, arg1: u32);"
        );
    }

    #[test]
    fn treats_an_underscore_parameter_name_as_unnamed() {
        let mut fields = fields(quote! {
            {
                hook: StorageValue<StoredProcedure<fn(_: Felt, amount: u32)>>,
            }
        });
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();

        assert_eq!(
            stored_procedure_wit_signature(&slots[0]),
            "dyncall-hook: func(proc-root: word, arg0: felt, amount: u32);"
        );
    }

    #[test]
    fn rejects_an_explicit_name_colliding_with_a_synthesized_one() {
        let mut fields = fields(quote! {
            {
                hook: StorageValue<StoredProcedure<fn(Felt, arg0: u32)>>,
            }
        });
        let message = collect_error(&mut fields);

        assert!(message.contains("would be named `arg0`"), "{message}");
    }

    #[test]
    fn rejects_custom_types_nested_in_option_and_result() {
        for signature in [quote!(fn(x: Option<MyStruct>)), quote!(fn() -> Result<Felt, MyError>)] {
            let mut fields = fields(quote! {
                {
                    authority: StorageValue<StoredProcedure<#signature>>,
                }
            });
            let message = collect_error(&mut fields);
            assert!(
                message.contains(
                    "stored procedure signatures support only Miden core types and primitives"
                ),
                "{message}"
            );
        }
    }

    #[test]
    fn rejects_generated_items_colliding_with_the_storage_struct() {
        let mut fields = fields(quote! {
            {
                hook: StorageValue<StoredProcedure<fn()>>,
            }
        });
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();
        let err = expand_stored_procedure_slots(
            &format_ident!("HookCall"),
            &Visibility::Inherited,
            &slots,
        )
        .unwrap_err();

        assert!(err.to_string().contains("collides with the storage struct"), "{err}");
    }

    #[test]
    fn generates_a_result_less_signature_for_a_unit_return() {
        let mut fields = fields(quote! {
            {
                hook: StorageValue<StoredProcedure<fn() -> ()>>,
            }
        });
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();

        assert!(slots[0].result.is_none());
        assert_eq!(
            stored_procedure_wit_signature(&slots[0]),
            "dyncall-hook: func(proc-root: word);"
        );
    }

    #[test]
    fn ignores_storage_fields_that_are_not_stored_procedures() {
        let mut fields = fields(quote! {
            {
                value: StorageValue<Word>,
                map: StorageMap<Felt, Word>,
            }
        });
        assert!(collect_stored_procedure_slots(&mut fields).unwrap().is_empty());
    }

    #[test]
    fn rejects_a_non_fn_signature_argument() {
        let mut fields = fields(quote! {
            {
                authority: StorageValue<StoredProcedure<u32>>,
            }
        });
        let message = collect_error(&mut fields);

        assert!(message.contains(SIGNATURE_SHAPE_ERROR), "{message}");
        assert!(message.contains("found `u32`"), "{message}");
    }

    #[test]
    fn rejects_unsafe_extern_variadic_and_bound_signatures() {
        let cases = [
            (quote!(unsafe fn()), "found `unsafe`"),
            (quote!(extern "C" fn()), "found an explicit ABI"),
            (quote!(for<'a> fn(x: &'a Felt)), "found a `for<..>` lifetime binder"),
        ];

        for (signature, expected) in cases {
            let mut fields = fields(quote! {
                {
                    authority: StorageValue<StoredProcedure<#signature>>,
                }
            });
            let message = collect_error(&mut fields);
            assert!(message.contains(SIGNATURE_SHAPE_ERROR), "{message}");
            assert!(message.contains(expected), "{message}");
        }
    }

    #[test]
    fn rejects_reference_parameters() {
        let mut fields = fields(quote! {
            {
                authority: StorageValue<StoredProcedure<fn(x: &Felt)>>,
            }
        });
        let message = collect_error(&mut fields);

        assert!(message.contains("references are not supported"), "{message}");
    }

    #[test]
    fn rejects_custom_types_in_signatures() {
        let mut fields = fields(quote! {
            {
                authority: StorageValue<StoredProcedure<fn(payload: MyStruct)>>,
            }
        });
        let message = collect_error(&mut fields);

        assert!(
            message.contains(
                "stored procedure signatures support only Miden core types and primitives"
            ),
            "{message}"
        );
    }

    #[test]
    fn detects_stored_procedure_mentions_in_nested_types() {
        let map_value: Type = syn::parse_quote!(StorageMap<Felt, StoredProcedure<fn()>>);
        assert!(mentions_stored_procedure(&map_value));

        let unrelated: Type = syn::parse_quote!(StorageMap<Felt, Word>);
        assert!(!mentions_stored_procedure(&unrelated));

        // A type whose name merely contains the marker name is not a mention.
        let lookalike: Type = syn::parse_quote!(StorageValue<MyStoredProcedureThing>);
        assert!(!mentions_stored_procedure(&lookalike));
    }

    #[test]
    fn builds_the_call_trait_and_marker_for_a_slot() {
        let mut fields = example_fields();
        let slots = collect_stored_procedure_slots(&mut fields).unwrap();
        let module_ident = bindings_module_ident(&format_ident!("AuthorityStorage"));
        let module_path = vec![format_ident!("miden"), format_ident!("authority_storage")];

        let items =
            build_slot_items(&slots[0], &syn::parse_quote!(pub), &module_ident, &module_path)
                .to_string();

        assert!(items.contains("pub struct AuthoritySignature ;"), "{items}");
        assert!(
            items.contains("impl :: miden :: ProcedureSignature for AuthoritySignature { }"),
            "{items}"
        );
        assert!(
            items.contains("fn call (& self , role : Felt , caller : AccountId) -> bool"),
            "{items}"
        );
        assert!(
            items.contains(
                "__miden_stored_procedure_bindings_authority_storage :: miden :: \
                 authority_storage :: dyncall_authority (self . root () , role , caller)"
            ),
            "{items}"
        );

        let unit_items =
            build_slot_items(&slots[1], &syn::parse_quote!(pub), &module_ident, &module_path)
                .to_string();
        assert!(unit_items.contains("dyncall_hook (self . root ()) ; }"), "{unit_items}");
    }
}
