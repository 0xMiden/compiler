use std::{
    collections::BTreeSet,
    env,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
};

use heck::{ToKebabCase, ToSnakeCase};
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, Item, ItemImpl, ItemStruct, PathArguments, Type,
    parse_macro_input, spanned::Spanned,
};
use toml::Value;

use crate::{
    boilerplate::runtime_boilerplate,
    component_macro::{
        FRONTEND_METADATA_LINK_SECTION, encode_frontend_metadata, metadata::get_package_metadata,
    },
    util::strip_line_comment,
};

const NOTE_SCRIPT_ATTR: &str = "note_script";
const NOTE_SCRIPT_MARKER_ATTR: &str = "miden_note_script_requires_note";
const NOTE_SCRIPT_DOC_MARKER: &str = "__miden_note_script_marker";
const NOTE_SCRIPT_UNIQUENESS_GUARD_SYMBOL: &str = "__MIDEN_NOTE_SCRIPT_UNIQUENESS_GUARD";
const CORE_TYPES_PACKAGE: &str = "miden:base/core-types@1.0.0";

/// Expands `#[note]` for either a note input `struct` or an inherent `impl` block.
pub(crate) fn expand_note(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "this attribute does not accept arguments")
            .into_compile_error()
            .into();
    }

    let item = parse_macro_input!(item as Item);
    match item {
        Item::Struct(item_struct) => expand_note_struct(item_struct).into(),
        Item::Impl(item_impl) => expand_note_impl(item_impl).into(),
        other => syn::Error::new(
            other.span(),
            "`#[note]` must be applied to a `struct` or an inherent `impl` block",
        )
        .into_compile_error()
        .into(),
    }
}

/// Expands `#[note_script]`.
///
/// This attribute must be applied to a method inside an inherent `impl` block annotated with
/// `#[note]`. It acts as a marker for `#[note]` to locate the entrypoint method and emit
/// frontend metadata for the generated note-script export.
pub(crate) fn expand_note_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "this attribute does not accept arguments")
            .into_compile_error()
            .into();
    }

    let item_tokens: TokenStream2 = item.clone().into();
    let mut item_fn: ImplItemFn = match syn::parse2(item_tokens.clone()) {
        Ok(item_fn) => item_fn,
        Err(_) => {
            if let Ok(item_fn) = syn::parse2::<syn::ItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[note_script]` must be applied to a method inside a `#[note]` `impl` block",
                )
                .into_compile_error()
                .into();
            }

            if let Ok(item_fn) = syn::parse2::<syn::TraitItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[note_script]` must be applied to a method inside a `#[note]` `impl` block",
                )
                .into_compile_error()
                .into();
            }

            return syn::Error::new(
                Span::call_site(),
                "`#[note_script]` must be applied to a method inside a `#[note]` `impl` block",
            )
            .into_compile_error()
            .into();
        }
    };

    // Preserve a helper attribute for `#[note]` to consume. If the surrounding impl forgets
    // `#[note]`, rustc rejects this unknown helper attribute instead of silently compiling a
    // method that emits no note-script metadata.
    let marker_attr = format_ident!("{}", NOTE_SCRIPT_MARKER_ATTR);
    item_fn.attrs.push(syn::parse_quote!(#[#marker_attr]));
    quote!(#item_fn).into()
}

fn expand_note_struct(item_struct: ItemStruct) -> TokenStream2 {
    let struct_ident = &item_struct.ident;

    if !item_struct.generics.params.is_empty() {
        return syn::Error::new(
            item_struct.generics.span(),
            "`#[note]` does not support generic note input structs",
        )
        .into_compile_error();
    }

    let from_impl = match &item_struct.fields {
        syn::Fields::Unit => {
            quote! {
                impl ::core::convert::TryFrom<&[::miden::Felt]> for #struct_ident {
                    type Error = ::miden::felt_repr::FeltReprError;

                    #[inline(always)]
                    fn try_from(felts: &[::miden::Felt]) -> Result<Self, Self::Error> {
                        let reader = ::miden::felt_repr::FeltReader::new(felts);
                        reader.ensure_eof()?;
                        Ok(Self)
                    }
                }
            }
        }
        syn::Fields::Named(fields) => {
            let field_inits = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named fields must have identifiers");
                let ty = &field.ty;
                quote! {
                    #ident: <#ty as ::miden::felt_repr::FromFeltRepr>::from_felt_repr(&mut reader)?
                }
            });

            quote! {
                impl ::core::convert::TryFrom<&[::miden::Felt]> for #struct_ident {
                    type Error = ::miden::felt_repr::FeltReprError;

                    #[inline(always)]
                    fn try_from(felts: &[::miden::Felt]) -> Result<Self, Self::Error> {
                        let mut reader = ::miden::felt_repr::FeltReader::new(felts);
                        let value = Self { #(#field_inits),* };
                        reader.ensure_eof()?;
                        Ok(value)
                    }
                }
            }
        }
        syn::Fields::Unnamed(fields) => {
            let field_inits = fields.unnamed.iter().map(|field| {
                let ty = &field.ty;
                quote! {
                    <#ty as ::miden::felt_repr::FromFeltRepr>::from_felt_repr(&mut reader)?
                }
            });

            quote! {
                impl ::core::convert::TryFrom<&[::miden::Felt]> for #struct_ident {
                    type Error = ::miden::felt_repr::FeltReprError;

                    #[inline(always)]
                    fn try_from(felts: &[::miden::Felt]) -> Result<Self, Self::Error> {
                        let mut reader = ::miden::felt_repr::FeltReader::new(felts);
                        let value = Self(#(#field_inits),*);
                        reader.ensure_eof()?;
                        Ok(value)
                    }
                }
            }
        }
    };

    quote! {
        #item_struct
        #from_impl
    }
}

fn expand_note_impl(item_impl: ItemImpl) -> TokenStream2 {
    if item_impl.trait_.is_some() {
        return syn::Error::new(
            item_impl.span(),
            "`#[note]` cannot be applied to trait impl blocks",
        )
        .into_compile_error();
    }

    if !item_impl.generics.params.is_empty() {
        return syn::Error::new(
            item_impl.generics.span(),
            "`#[note]` does not support generic impl blocks",
        )
        .into_compile_error();
    }

    let note_ty = match item_impl.self_ty.as_ref() {
        Type::Path(type_path) if type_path.qself.is_none() => type_path.clone(),
        other => {
            return syn::Error::new(
                other.span(),
                "`#[note]` requires an impl block for a concrete type (e.g. `impl MyNote { ... }`)",
            )
            .into_compile_error();
        }
    };

    let (entrypoint_fn, item_impl) = match extract_entrypoint(item_impl) {
        Ok(val) => val,
        Err(err) => return err.into_compile_error(),
    };

    let (arg_idx, account_param) = match parse_entrypoint_signature(&entrypoint_fn) {
        Ok(val) => val,
        Err(err) => return err.into_compile_error(),
    };

    let entrypoint_ident = &entrypoint_fn.sig.ident;
    let note_ident = note_ty
        .path
        .segments
        .last()
        .expect("type path must have at least one segment")
        .ident
        .clone();
    let guest_struct_ident = quote::format_ident!("__MidenNoteScript_{note_ident}");
    let export_name = entrypoint_ident.to_string().to_kebab_case();

    let note_init = note_instantiation(&note_ty);
    let (account_instantiation, account_arg, account_trait_impl) = match account_param {
        Some(AccountParam { ty, mut_ref }) => {
            let account_ident = quote::format_ident!("__miden_account");
            (
                quote! {
                    let mut #account_ident = <#ty as ::core::default::Default>::default();
                },
                if mut_ref {
                    quote! { &mut #account_ident }
                } else {
                    quote! { &#account_ident }
                },
                quote! {
                    impl ::miden::active_account::ActiveAccount for #ty {}
                },
            )
        }
        None => (quote! {}, quote! {}, quote! {}),
    };

    let args = match build_entrypoint_call_args(entrypoint_fn.sig.span(), arg_idx, account_arg) {
        Ok(args) => args,
        Err(err) => return err.into_compile_error(),
    };
    let call = quote! { __miden_note.#entrypoint_ident(#(#args),*); };

    let metadata = match get_package_metadata(proc_macro::Span::call_site()) {
        Ok(metadata) => metadata,
        Err(err) => return err.to_compile_error(),
    };
    let component_package = metadata
        .component_package
        .unwrap_or_else(|| format!("miden:{}", metadata.name.to_kebab_case()));
    let interface_name = metadata.name.to_kebab_case();
    let world_name = format!("{interface_name}-world");
    let interface_module = interface_name.to_snake_case();
    let dependency_imports = match collect_dependency_world_imports(proc_macro::Span::call_site()) {
        Ok(imports) => imports,
        Err(err) => return err.to_compile_error(),
    };

    let inline_wit = build_note_script_wit(
        &component_package,
        &metadata.version,
        &interface_name,
        &world_name,
        &export_name,
        &dependency_imports,
    );
    let inline_literal = Literal::string(&inline_wit);
    let guest_trait_path = match build_guest_trait_path(&component_package, &interface_module) {
        Ok(path) => path,
        Err(err) => return err.into_compile_error(),
    };
    let runtime_boilerplate = runtime_boilerplate();
    let frontend_link_section = generate_frontend_link_section(&export_name);

    quote! {
        #runtime_boilerplate
        #item_impl

        ::miden::generate!(inline = #inline_literal);
        self::bindings::export!(#guest_struct_ident);

        #account_trait_impl

        // Bring ActiveAccount trait into scope so users can call account.get_id(), etc.
        #[allow(unused_imports)]
        use ::miden::active_account::ActiveAccount as _;

        #[doc = "Guest entry point generated by the Miden note script macros."]
        pub struct #guest_struct_ident;

        impl #guest_trait_path for #guest_struct_ident {
            fn #entrypoint_ident(arg: ::miden::Word) {
                #note_init
                #account_instantiation
                #call
            }
        }

        #frontend_link_section
    }
}

#[derive(Clone)]
struct AccountParam {
    ty: Type,
    mut_ref: bool,
}

fn note_instantiation(note_ty: &syn::TypePath) -> TokenStream2 {
    // NOTE: Avoid calling `active_note::get_storage()` for zero-sized note types so that "no
    // storage" notes can execute without requiring a full active-note runtime context.
    quote! {
        let __miden_note: #note_ty = if ::core::mem::size_of::<#note_ty>() == 0 {
            match <#note_ty as ::core::convert::TryFrom<&[::miden::Felt]>>::try_from(&[]) {
                Ok(note) => note,
                Err(err) => ::core::panic!("failed to decode note inputs: {err:?}"),
            }
        } else {
            let inputs = ::miden::active_note::get_storage();
            match <#note_ty as ::core::convert::TryFrom<&[::miden::Felt]>>::try_from(inputs.as_slice()) {
                Ok(note) => note,
                Err(err) => ::core::panic!("failed to decode note inputs: {err:?}"),
            }
        };
    }
}

fn extract_entrypoint(mut item_impl: ItemImpl) -> syn::Result<(ImplItemFn, ItemImpl)> {
    let mut entrypoints = Vec::new();

    for item in &mut item_impl.items {
        let ImplItem::Fn(item_fn) = item else {
            continue;
        };

        if has_entrypoint_marker_attr(&item_fn.attrs) {
            entrypoints.push(item_fn.clone());
            // Remove entrypoint markers so they don't reach the output.
            item_fn.attrs.retain(|attr| !is_entrypoint_marker_attr(attr));
        }
    }

    match entrypoints.as_slice() {
        [only] => Ok((only.clone(), item_impl)),
        [] => Err(syn::Error::new(
            item_impl.span(),
            "`#[note]` requires an entrypoint method annotated with `#[note_script]`",
        )),
        _ => Err(syn::Error::new(
            item_impl.span(),
            "`#[note]` requires a single entrypoint method (only one `#[note_script]` method is \
             allowed)",
        )),
    }
}

/// Parses the entrypoint signature.
///
/// Returns:
/// - index of the Word argument among the non-receiver arguments (0 or 1)
/// - optional injected account parameter
fn parse_entrypoint_signature(
    entrypoint: &ImplItemFn,
) -> syn::Result<(usize, Option<AccountParam>)> {
    let sig = &entrypoint.sig;

    if let Some(asyncness) = sig.asyncness {
        return Err(syn::Error::new(asyncness.span(), "entrypoint method must not be `async`"));
    }

    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new(sig.generics.span(), "entrypoint method must not be generic"));
    }

    let receiver = sig
        .receiver()
        .ok_or_else(|| syn::Error::new(sig.span(), "entrypoint method must accept `self`"))?;

    if receiver.colon_token.is_some() {
        return Err(syn::Error::new(
            receiver.span(),
            "entrypoint receiver must be `self` (by value); typed receivers (e.g. `self: \
             Box<Self>`) are not supported",
        ));
    }

    if receiver.reference.is_some() {
        return Err(syn::Error::new(
            receiver.span(),
            "entrypoint receiver must be `self` (by value); `&self` / `&mut self` are not \
             supported",
        ));
    }

    if receiver.mutability.is_some() {
        return Err(syn::Error::new(
            receiver.span(),
            "entrypoint receiver must be `self` (non-mutable); `mut self` is not supported",
        ));
    }

    if !is_unit_return_type(&sig.output) {
        return Err(syn::Error::new(sig.output.span(), "entrypoint method must return `()`"));
    }

    let non_receiver_args: Vec<_> =
        sig.inputs.iter().filter(|arg| !matches!(arg, FnArg::Receiver(_))).collect();

    if non_receiver_args.is_empty() || non_receiver_args.len() > 2 {
        return Err(syn::Error::new(
            sig.span(),
            "entrypoint must accept 1 or 2 arguments (excluding `self`): `Word` and optional \
             `&Account`/`&mut Account`",
        ));
    }

    let mut word_positions = Vec::new();
    let mut account: Option<AccountParam> = None;

    for (idx, arg) in non_receiver_args.iter().enumerate() {
        let FnArg::Typed(pat_type) = arg else {
            continue;
        };
        if is_type_named(pat_type.ty.as_ref(), "Word") {
            word_positions.push(idx);
            continue;
        }

        if let Some((ty, mut_ref)) = parse_account_ref_type(pat_type.ty.as_ref()) {
            if account.is_some() {
                return Err(syn::Error::new(
                    pat_type.ty.span(),
                    "entrypoint may only declare a single account parameter",
                ));
            }
            account = Some(AccountParam { ty, mut_ref });
            continue;
        }

        return Err(syn::Error::new(
            pat_type.ty.span(),
            "unsupported entrypoint parameter type; expected `Word` and optional `&Account`/`&mut \
             Account`",
        ));
    }

    let [word_idx] = word_positions.as_slice() else {
        return Err(syn::Error::new(
            sig.span(),
            "entrypoint must declare exactly one `Word` parameter",
        ));
    };

    if non_receiver_args.len() == 2 && account.is_none() {
        return Err(syn::Error::new(
            sig.span(),
            "entrypoint with two parameters must include an account reference (`&Account` or \
             `&mut Account`)",
        ));
    }

    Ok((*word_idx, account))
}

/// Builds the arguments passed to the user's entrypoint method call.
fn build_entrypoint_call_args(
    error_span: Span,
    arg_word_idx: usize,
    account_arg: TokenStream2,
) -> syn::Result<Vec<TokenStream2>> {
    let arg = quote! { arg };

    if account_arg.is_empty() {
        return Ok(vec![arg]);
    }

    match arg_word_idx {
        0 => Ok(vec![arg, account_arg]),
        1 => Ok(vec![account_arg, arg]),
        _ => Err(syn::Error::new(error_span, "internal error: invalid entrypoint argument index")),
    }
}

fn parse_account_ref_type(ty: &Type) -> Option<(Type, bool)> {
    let Type::Reference(type_ref) = ty else {
        return None;
    };
    if !is_type_named(type_ref.elem.as_ref(), "Account") {
        return None;
    }
    Some(((*type_ref.elem).clone(), type_ref.mutability.is_some()))
}

/// Returns true if the entrypoint return type is unit.
fn is_unit_return_type(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => matches!(ty.as_ref(), Type::Tuple(t) if t.elems.is_empty()),
    }
}

fn is_type_named(ty: &Type, name: &str) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    type_path
        .path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == name && matches!(seg.arguments, PathArguments::None))
}

/// Returns true if any entrypoint marker attribute is present.
fn has_entrypoint_marker_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_entrypoint_marker_attr)
}

fn is_attr_named(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|seg| seg.ident == name && matches!(seg.arguments, PathArguments::None))
}

/// Returns true if an attribute marks a method as the note entrypoint.
fn is_entrypoint_marker_attr(attr: &Attribute) -> bool {
    is_attr_named(attr, NOTE_SCRIPT_ATTR)
        || is_attr_named(attr, NOTE_SCRIPT_MARKER_ATTR)
        || is_doc_marker_attr(attr, NOTE_SCRIPT_DOC_MARKER)
}

/// Returns true if `attr` is `#[doc = "..."]` with `marker` as the string value.
fn is_doc_marker_attr(attr: &Attribute, marker: &str) -> bool {
    if !attr.path().is_ident("doc") {
        return false;
    }

    let syn::Meta::NameValue(meta) = &attr.meta else {
        return false;
    };

    let syn::Expr::Lit(expr) = &meta.value else {
        return false;
    };

    let syn::Lit::Str(value) = &expr.lit else {
        return false;
    };

    value.value() == marker
}

/// Renders the inline WIT world exported by a note script.
fn build_note_script_wit(
    component_package: &str,
    component_version: &semver::Version,
    interface_name: &str,
    world_name: &str,
    export_name: &str,
    dependency_imports: &[String],
) -> String {
    let package_with_version = if component_package.contains('@') {
        component_package.to_string()
    } else {
        format!("{component_package}@{component_version}")
    };

    let mut wit_source = String::new();
    let _ = writeln!(wit_source, "// This file is auto-generated by the `#[note]` macro.");
    let _ = writeln!(wit_source, "// Do not edit this file manually.");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "package {package_with_version};");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "use {CORE_TYPES_PACKAGE};");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "interface {interface_name} {{");
    let _ = writeln!(wit_source, "    use core-types.{{word}};");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "    {export_name}: func(arg: word);");
    let _ = writeln!(wit_source, "}}");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "world {world_name} {{");
    for import in dependency_imports {
        let _ = writeln!(wit_source, "    import {import};");
    }
    if !dependency_imports.is_empty() {
        wit_source.push('\n');
    }
    let _ = writeln!(wit_source, "    export {interface_name};");
    let _ = writeln!(wit_source, "}}");

    wit_source
}

/// Synthesizes the generated guest trait path for the inline note-script interface.
fn build_guest_trait_path(
    component_package: &str,
    interface_module: &str,
) -> syn::Result<syn::Path> {
    let package_without_version =
        component_package.split('@').next().unwrap_or(component_package).trim();

    let segments: Vec<_> = package_without_version
        .split([':', '/'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_snake_case())
        .collect();

    if segments.is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "invalid component package identifier provided in manifest metadata",
        ));
    }

    let mut path = String::from("self::bindings::exports");
    for segment in segments {
        path.push_str("::");
        path.push_str(&segment);
    }
    path.push_str("::");
    path.push_str(interface_module);
    path.push_str("::Guest");

    syn::parse_str(&path).map_err(|err| {
        syn::Error::new(
            Span::call_site(),
            format!("failed to parse guest trait path '{path}': {err}"),
        )
    })
}

/// Collects fully-qualified WIT interfaces exported by the crate's component target dependencies.
fn collect_dependency_world_imports(call_site_span: proc_macro::Span) -> syn::Result<Vec<String>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cargo_toml_path = Path::new(&manifest_dir).join("Cargo.toml");
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path).map_err(|err| {
        syn::Error::new(
            call_site_span.into(),
            format!("failed to read '{}': {err}", cargo_toml_path.display()),
        )
    })?;
    let cargo_toml: Value = cargo_toml_content.parse::<Value>().map_err(|err| {
        syn::Error::new(
            call_site_span.into(),
            format!("failed to parse '{}': {err}", cargo_toml_path.display()),
        )
    })?;

    let Some(dependencies) = cargo_toml
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("metadata"))
        .and_then(Value::as_table)
        .and_then(|metadata| metadata.get("component"))
        .and_then(Value::as_table)
        .and_then(|component| component.get("target"))
        .and_then(Value::as_table)
        .and_then(|target| target.get("dependencies"))
        .and_then(Value::as_table)
    else {
        return Ok(Vec::new());
    };

    let mut imports = BTreeSet::new();
    for (name, dependency) in dependencies {
        let table = dependency.as_table().ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!(
                    "dependency '{name}' under [package.metadata.component.target.dependencies] \
                     must be a table"
                ),
            )
        })?;
        let path_value = table.get("path").and_then(Value::as_str).ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!("dependency '{name}' is missing a 'path' entry"),
            )
        })?;
        let raw_path = PathBuf::from(path_value);
        let path = if raw_path.is_absolute() {
            raw_path
        } else {
            Path::new(&manifest_dir).join(raw_path)
        };

        collect_dependency_exports(&path, call_site_span, &mut imports)?;
    }

    Ok(imports.into_iter().collect())
}

/// Collects exported interfaces from a dependency WIT file or directory.
fn collect_dependency_exports(
    path: &Path,
    call_site_span: proc_macro::Span,
    imports: &mut BTreeSet<String>,
) -> syn::Result<()> {
    let metadata = fs::metadata(path).map_err(|err| {
        syn::Error::new(
            call_site_span.into(),
            format!("failed to read metadata for '{}': {err}", path.display()),
        )
    })?;

    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(|err| {
                syn::Error::new(
                    call_site_span.into(),
                    format!("failed to read '{}': {err}", path.display()),
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                syn::Error::new(
                    call_site_span.into(),
                    format!("failed to iterate '{}': {err}", path.display()),
                )
            })?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                continue;
            }
            if entry_path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
                continue;
            }
            collect_wit_file_exports(&entry_path, call_site_span, imports)?;
        }
    } else {
        collect_wit_file_exports(path, call_site_span, imports)?;
    }

    Ok(())
}

/// Collects exported interfaces from a single WIT source file.
fn collect_wit_file_exports(
    path: &Path,
    call_site_span: proc_macro::Span,
    imports: &mut BTreeSet<String>,
) -> syn::Result<()> {
    let wit_source = fs::read_to_string(path).map_err(|err| {
        syn::Error::new(
            call_site_span.into(),
            format!("failed to read '{}': {err}", path.display()),
        )
    })?;
    let Some(package_id) = parse_wit_package_id(&wit_source) else {
        return Ok(());
    };

    for export in parse_wit_world_exports(&wit_source) {
        if export == "core-types" {
            continue;
        }

        let import = if export.contains(':') {
            export
        } else {
            qualify_wit_export(&package_id, &export, call_site_span)?
        };
        imports.insert(import);
    }

    Ok(())
}

/// Parses the WIT package identifier from the source contents.
fn parse_wit_package_id(wit_source: &str) -> Option<String> {
    wit_source.lines().find_map(|line| {
        let trimmed = strip_line_comment(line).trim();
        trimmed
            .strip_prefix("package ")
            .map(|rest| rest.trim_end_matches(';').trim().to_string())
    })
}

/// Parses the exported interface names declared by WIT worlds in the source contents.
fn parse_wit_world_exports(wit_source: &str) -> Vec<String> {
    wit_source
        .lines()
        .filter_map(|line| {
            let trimmed = strip_line_comment(line).trim();
            trimmed
                .strip_prefix("export ")
                .map(|rest| rest.trim_end_matches(';').trim().to_string())
        })
        .collect()
}

/// Qualifies an unqualified world export using its enclosing package identifier.
fn qualify_wit_export(
    package_id: &str,
    export_name: &str,
    call_site_span: proc_macro::Span,
) -> syn::Result<String> {
    let Some((package_without_version, version)) = package_id.rsplit_once('@') else {
        return Err(syn::Error::new(
            call_site_span.into(),
            format!("WIT package '{package_id}' is missing a version suffix"),
        ));
    };

    Ok(format!("{package_without_version}/{export_name}@{version}"))
}

/// Emits frontend-only note-script metadata into the shared component frontend custom section.
fn generate_frontend_link_section(export_name: &str) -> TokenStream2 {
    let metadata_bytes = encode_frontend_metadata(None, Some(export_name));
    let metadata_len = metadata_bytes.len();
    let encoded_bytes = Literal::byte_string(&metadata_bytes);
    let uniqueness_guard_symbol = NOTE_SCRIPT_UNIQUENESS_GUARD_SYMBOL;

    quote! {
        const _: () = {
            // A crate may contain exactly one `#[note_script]` method. Reusing a fixed symbol name
            // lets the linker reject duplicates across modules or impl blocks.
            #[doc(hidden)]
            #[used]
            #[unsafe(export_name = #uniqueness_guard_symbol)]
            static __miden_note_script_uniqueness_guard: u8 = 0;
        };

        #[unsafe(
            // Keep the Mach-O-friendly `segment,section` naming scheme used by the main metadata
            // section so the linker preserves these bytes in test and release builds.
            link_section = #FRONTEND_METADATA_LINK_SECTION
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_NOTE_SCRIPT_FRONTEND_METADATA_BYTES: [u8; #metadata_len] = *#encoded_bytes;
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    #[test]
    fn entrypoint_signature_allows_non_run_name() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn execute(self, _arg: Word) {}
        };

        assert!(parse_entrypoint_signature(&item_fn).is_ok());
    }

    #[test]
    fn entrypoint_signature_requires_unit_return() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn run(self, arg: Word) -> Word { arg }
        };

        let err = match parse_entrypoint_signature(&item_fn) {
            Ok(_) => panic!("expected signature validation to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must return `()`"));
    }

    #[test]
    fn entrypoint_signature_rejects_async() {
        let item_fn: ImplItemFn = parse_quote! {
            pub async fn execute(self, _arg: Word) {}
        };

        let err = match parse_entrypoint_signature(&item_fn) {
            Ok(_) => panic!("expected signature validation to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must not be `async`"));
    }

    #[test]
    fn entrypoint_signature_rejects_typed_receiver() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn execute(self: Box<Self>, _arg: Word) {}
        };

        let err = match parse_entrypoint_signature(&item_fn) {
            Ok(_) => panic!("expected signature validation to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("typed receivers"));
    }

    #[test]
    fn entrypoint_signature_rejects_generics() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn execute<T>(self, _arg: Word) {}
        };

        let err = match parse_entrypoint_signature(&item_fn) {
            Ok(_) => panic!("expected signature validation to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must not be generic"));
    }

    #[test]
    fn extract_entrypoint_accepts_doc_marker() {
        let marker = syn::LitStr::new(NOTE_SCRIPT_DOC_MARKER, Span::call_site());
        let item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[doc = #marker]
                pub fn execute(self, _arg: Word) {}
            }
        };

        let (entrypoint_fn, item_impl) = extract_entrypoint(item_impl).unwrap();
        assert_eq!(entrypoint_fn.sig.ident, "execute");

        let ImplItem::Fn(method) = item_impl.items.first().expect("method must exist") else {
            panic!("expected function method");
        };
        assert!(
            method
                .attrs
                .iter()
                .all(|attr| !is_doc_marker_attr(attr, NOTE_SCRIPT_DOC_MARKER)),
            "entrypoint markers must be removed from output"
        );
    }

    #[test]
    fn extract_entrypoint_accepts_qualified_note_script_attr() {
        let item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[miden::note_script]
                pub fn execute(self, _arg: Word) {}
            }
        };

        let (entrypoint_fn, item_impl) = extract_entrypoint(item_impl).unwrap();
        assert_eq!(entrypoint_fn.sig.ident, "execute");

        let ImplItem::Fn(method) = item_impl.items.first().expect("method must exist") else {
            panic!("expected function method");
        };
        assert!(
            method.attrs.iter().all(|attr| !is_entrypoint_marker_attr(attr)),
            "entrypoint markers must be removed from output"
        );
    }

    #[test]
    fn note_script_frontend_metadata_emits_project_wide_uniqueness_guard() {
        let tokens = generate_frontend_link_section("execute").to_string();

        assert!(tokens.contains(NOTE_SCRIPT_UNIQUENESS_GUARD_SYMBOL));
        assert!(tokens.contains("execute"));
    }

    #[test]
    fn note_script_wit_uses_the_marked_method_name() {
        let wit = build_note_script_wit(
            "miden:my-note",
            &semver::Version::new(1, 0, 0),
            "my-note",
            "my-note-world",
            "execute",
            &[],
        );

        assert!(wit.contains("execute: func(arg: word);"));
        assert!(!wit.contains("run: func(arg: word);"));
    }

    #[test]
    fn note_script_marker_accepts_helper_attribute() {
        let method: ImplItemFn = parse_quote! {
            #[miden_note_script_requires_note]
            pub fn execute(self, _arg: Word) {}
        };

        assert!(has_entrypoint_marker_attr(&method.attrs));
    }
}
