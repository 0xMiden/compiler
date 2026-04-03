use std::{
    collections::{BTreeSet, HashMap},
    env,
    str::FromStr,
};

use heck::{ToKebabCase, ToSnakeCase};
use miden_protocol::{account::AccountType, utils::serde::Serializable};
use proc_macro::Span;
use proc_macro2::{Ident, Literal, Span as Span2, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, ItemStruct, PathArguments, ReturnType, Type,
    Visibility, spanned::Spanned,
};

pub(crate) use crate::component_macro::storage::typecheck_storage_field;
use crate::{
    account_component_metadata::AccountComponentMetadataBuilder,
    boilerplate::runtime_boilerplate,
    component_macro::{
        generate_wit::{build_component_wit, write_component_wit_file},
        metadata::get_package_metadata,
        storage::process_storage_fields,
    },
    types::{
        ExportedTypeDef, ExportedTypeKind, TypeRef, map_type_to_type_ref, registered_export_types,
    },
};

mod generate_wit;
mod metadata;
mod storage;

/// Fully-qualified identifier for the core types package used by exported component interfaces.
const CORE_TYPES_PACKAGE: &str = "miden:base/core-types@1.0.0";
/// Attribute name used to mark the authentication procedure on a component method.
const AUTH_SCRIPT_ATTR: &str = "auth_script";
/// Stable marker preserved by `#[auth_script]` so `#[component]` can recognize the method.
const AUTH_SCRIPT_DOC_MARKER: &str = "__miden_auth_script_marker";
/// Symbol emitted for every `#[auth_script]` method to enforce project-wide uniqueness.
const AUTH_SCRIPT_UNIQUENESS_GUARD_SYMBOL: &str = "__MIDEN_AUTH_SCRIPT_UNIQUENESS_GUARD";
/// Wasm custom section used to pass frontend-only component metadata into the compiler frontend.
const FRONTEND_METADATA_LINK_SECTION: &str = "rodata,miden_account_component_frontend";
/// Cargo project kind used by authentication component crates.
const AUTH_COMPONENT_PROJECT_KIND: &str = "authentication-component";

/// Receiver kinds supported by the derived guest trait implementation.
#[derive(Clone, Copy)]
enum ReceiverKind {
    /// The method receives `&self`.
    Ref,
    /// The method receives `&mut self`.
    RefMut,
    /// The method receives `self` by value.
    Value,
}

/// Metadata describing a WIT function parameter generated from a Rust method argument.
struct MethodParam {
    ident: syn::Ident,
    user_ty: syn::Type,
    type_ref: TypeRef,
    wit_param_name: String,
}

enum MethodReturn {
    Unit,
    Type {
        user_ty: Box<syn::Type>,
        type_ref: TypeRef,
    },
}

/// Captures all information required to render WIT signatures and the guest trait implementation
/// for a single exported method.
struct ComponentMethod {
    /// Method identifier in Rust.
    fn_ident: syn::Ident,
    /// Documentation attributes carried over to the guest trait implementation.
    doc_attrs: Vec<Attribute>,
    /// Method parameters metadata.
    params: Vec<MethodParam>,
    /// Receiver mode required by the method.
    receiver_kind: ReceiverKind,
    /// Return type metadata.
    return_info: MethodReturn,
    /// Method name rendered in kebab-case for WIT output.
    wit_name: String,
    /// Indicates whether this method is the authentication procedure entrypoint.
    is_auth_script: bool,
}

/// Expands the `#[component]` attribute applied to either a struct declaration or an inherent
/// implementation block.
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site().into(), "#[component] does not accept arguments")
            .into_compile_error()
            .into();
    }

    let call_site_span = Span::call_site();
    let item_tokens: TokenStream2 = item.into();

    if let Ok(item_struct) = syn::parse2::<ItemStruct>(item_tokens.clone()) {
        match expand_component_struct(call_site_span, item_struct) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        }
    } else if let Ok(item_impl) = syn::parse2::<ItemImpl>(item_tokens) {
        match expand_component_impl(call_site_span, item_impl) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        }
    } else {
        syn::Error::new(
            call_site_span.into(),
            "The `component` macro only supports structs and inherent impl blocks.",
        )
        .into_compile_error()
        .into()
    }
}

/// Expands `#[auth_script]`.
///
/// This attribute must be applied to a method inside an inherent `impl` block annotated with
/// `#[component]`. It acts as a marker for `#[component]` so the macro can emit frontend metadata
/// for the annotated export without rewriting its user-defined name.
pub fn expand_auth_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span2::call_site(), "this attribute does not accept arguments")
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
                    "`#[auth_script]` must be applied to a method inside a `#[component]` `impl` \
                     block",
                )
                .into_compile_error()
                .into();
            }

            if let Ok(item_fn) = syn::parse2::<syn::TraitItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[auth_script]` must be applied to a method inside a `#[component]` `impl` \
                     block",
                )
                .into_compile_error()
                .into();
            }

            return syn::Error::new(
                Span2::call_site(),
                "`#[auth_script]` must be applied to a method inside a `#[component]` `impl` block",
            )
            .into_compile_error()
            .into();
        }
    };

    let marker = syn::LitStr::new(AUTH_SCRIPT_DOC_MARKER, Span2::call_site());
    item_fn.attrs.push(syn::parse_quote!(#[doc = #marker]));
    quote!(#item_fn).into()
}

/// Expands the `#[component]` attribute applied to a struct by wiring storage metadata and link
/// section exports.
fn expand_component_struct(
    call_site_span: Span,
    mut input_struct: ItemStruct,
) -> Result<TokenStream2, syn::Error> {
    let struct_name = &input_struct.ident;

    let metadata = get_package_metadata(call_site_span)?;
    let mut acc_builder = AccountComponentMetadataBuilder::new(
        metadata.name.clone(),
        metadata.version.clone(),
        metadata.description.clone(),
    );

    for st in &metadata.supported_types {
        match AccountType::from_str(st) {
            Ok(account_type) => acc_builder.add_supported_type(account_type),
            Err(err) => {
                return Err(syn::Error::new(
                    call_site_span.into(),
                    format!("Invalid account type '{st}' in supported-types: {err}"),
                ));
            }
        }
    }

    let default_impl = match &mut input_struct.fields {
        syn::Fields::Named(fields) => {
            let storage_namespace = metadata.component_package.as_deref().unwrap_or(&metadata.name);
            let component_struct_name = struct_name.to_string();
            let field_inits = process_storage_fields(
                fields,
                &mut acc_builder,
                storage_namespace,
                &component_struct_name,
            )?;
            generate_default_impl(struct_name, &field_inits)
        }
        syn::Fields::Unit => quote! {
            impl Default for #struct_name {
                fn default() -> Self {
                    Self
                }
            }
        },
        _ => {
            return Err(syn::Error::new(
                input_struct.fields.span(),
                "The `component` macro only supports unit structs or structs with named fields.",
            ));
        }
    };

    let component_metadata = acc_builder.build(call_site_span.into())?;

    let mut metadata_bytes = component_metadata.to_bytes();
    let padded_len = metadata_bytes.len().div_ceil(16) * 16;
    metadata_bytes.resize(padded_len, 0);

    let link_section = generate_link_section(&metadata_bytes);
    let runtime_boilerplate = runtime_boilerplate();

    Ok(quote! {
        #runtime_boilerplate
        #input_struct
        #default_impl
        impl ::miden::native_account::NativeAccount for #struct_name {}
        impl ::miden::active_account::ActiveAccount for #struct_name {}
        #link_section
    })
}

/// Expands the `#[component]` attribute applied to an inherent implementation block by generating
/// the inline WIT interface, invoking `miden::generate!`, and wiring the guest trait implementation.
fn expand_component_impl(
    call_site_span: Span,
    mut impl_block: ItemImpl,
) -> Result<TokenStream2, syn::Error> {
    if impl_block.trait_.is_some() {
        return Err(syn::Error::new(
            impl_block.span(),
            "The `component` macro does not support trait implementations.",
        ));
    }

    let component_type = (*impl_block.self_ty).clone();
    if extract_type_ident(&component_type).is_none() {
        return Err(syn::Error::new(
            impl_block.self_ty.span(),
            "Failed to determine the struct name targeted by this implementation.",
        ));
    }

    let metadata = get_package_metadata(call_site_span)?;
    let component_package = metadata.component_package.clone().ok_or_else(|| {
        syn::Error::new(
            call_site_span.into(),
            "package.metadata.component.package in Cargo.toml is required to derive the component \
             interface",
        )
    })?;

    let interface_name = metadata.name.to_kebab_case();
    let interface_module = {
        let name: &str = &interface_name;
        name.to_snake_case()
    };
    let world_name = format!("{interface_name}-world");

    let mut exported_types = registered_export_types();
    exported_types.sort_by(|a, b| a.wit_name.cmp(&b.wit_name));
    let exported_types_by_rust: HashMap<_, _> =
        exported_types.iter().map(|def| (def.rust_name.clone(), def.clone())).collect();
    let mut methods = Vec::new();
    let mut type_imports = BTreeSet::new();
    let mut auth_method_count = 0usize;

    for item in &mut impl_block.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let is_auth_script = has_auth_script_marker_attr(&method.attrs);
        if is_auth_script && !matches!(method.vis, Visibility::Public(_)) {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "`#[auth_script]` can only be applied to `pub` component methods",
            ));
        }
        method.attrs.retain(|attr| !is_auth_script_marker_attr(attr));

        if !matches!(method.vis, Visibility::Public(_)) {
            continue;
        }

        let (parsed_method, imports) =
            parse_component_method(method, &exported_types_by_rust, is_auth_script)?;
        if parsed_method.is_auth_script {
            auth_method_count += 1;
        }
        type_imports.extend(imports);
        methods.push(parsed_method);
    }

    if methods.is_empty() {
        return Err(syn::Error::new(
            call_site_span.into(),
            "Component `impl` is missing `pub` methods. A component cannot have empty exports.",
        ));
    }

    validate_auth_script_count(
        metadata.project_kind.as_deref(),
        auth_method_count,
        impl_block.span(),
    )?;

    let wit_source = build_component_wit(
        &component_package,
        &metadata.version,
        &interface_name,
        &world_name,
        &type_imports,
        &methods,
        &exported_types,
    )?;
    write_component_wit_file(call_site_span, &wit_source, &component_package)?;
    let inline_literal = Literal::string(&wit_source);

    let guest_trait_path = build_guest_trait_path(&component_package, &interface_module)?;
    let guest_methods: Vec<TokenStream2> = methods
        .iter()
        .map(|method| render_guest_method(method, &component_type))
        .collect();

    let interface_path = format!("{}/{}@{}", component_package, interface_name, metadata.version);
    let module_prefix = component_module_prefix(&component_type);
    let module_prefix_segments: Option<Vec<String>> = module_prefix
        .as_ref()
        .map(|path| path.segments.iter().map(|segment| segment.ident.to_string()).collect());

    let custom_type_paths =
        collect_custom_type_paths(&exported_types, &methods, module_prefix_segments.as_deref());

    let (custom_with_entries, debug_with_entries) = build_custom_with_entries(
        &exported_types,
        &interface_path,
        module_prefix.as_ref(),
        &custom_type_paths,
    );

    if env::var_os("MIDEN_COMPONENT_DEBUG_WITH").is_some() {
        eprintln!(
            "[miden::component] with mappings for {}: {}",
            component_package,
            debug_with_entries.join(", ")
        );
    }

    let frontend_link_section = generate_frontend_link_section(&methods);

    Ok(quote! {
        ::miden::generate!(inline = #inline_literal, with = { #(#custom_with_entries)* });
        // Bring account traits into scope so users can call `self.add_asset()`, etc.
        #[allow(unused_imports)]
        use ::miden::native_account::NativeAccount as _;
        #[allow(unused_imports)]
        use ::miden::active_account::ActiveAccount as _;
        #impl_block
        impl #guest_trait_path for #component_type {
            #(#guest_methods)*
        }
        #frontend_link_section
        // Use the fully-qualified component type here so the export macro works even when
        // the impl block was declared through a module-qualified path (e.g. `impl super::Foo`).
        self::bindings::export!(#component_type);
    })
}

/// Validates how many methods may be annotated with `#[auth_script]` for the current project kind.
fn validate_auth_script_count(
    project_kind: Option<&str>,
    auth_method_count: usize,
    span: Span2,
) -> Result<(), syn::Error> {
    if project_kind == Some(AUTH_COMPONENT_PROJECT_KIND) {
        if auth_method_count != 1 {
            return Err(syn::Error::new(
                span,
                "authentication components require exactly one `#[auth_script]` method",
            ));
        }
    } else if auth_method_count > 1 {
        return Err(syn::Error::new(
            span,
            "only one `#[auth_script]` method is allowed per `#[component]` impl block",
        ));
    }

    Ok(())
}

/// Synthesizes the guest trait path exposed by `wit-bindgen` for the generated interface.
fn build_guest_trait_path(
    component_package: &str,
    interface_module: &str,
) -> Result<TokenStream2, syn::Error> {
    let package_without_version =
        component_package.split('@').next().unwrap_or(component_package).trim();

    let segments: Vec<_> = package_without_version
        .split([':', '/'])
        .filter(|segment| !segment.is_empty())
        .map(to_snake_case)
        .collect();

    if segments.is_empty() {
        return Err(syn::Error::new(
            Span::call_site().into(),
            "Invalid component package identifier provided in manifest metadata.",
        ));
    }

    let module_idents: Vec<_> =
        segments.iter().map(|segment| format_ident!("{}", segment)).collect();
    let interface_ident = format_ident!("{}", to_snake_case(interface_module));

    Ok(quote! { self::bindings::exports #( :: #module_idents)* :: #interface_ident :: Guest })
}

/// Emits the guest trait method forwarding logic invoking the user-defined implementation.
fn render_guest_method(method: &ComponentMethod, component_type: &Type) -> TokenStream2 {
    let fn_ident = &method.fn_ident;
    let doc_attrs = &method.doc_attrs;
    let component_ident = format_ident!("__component_instance");

    let mut param_tokens = Vec::new();
    let mut call_args = Vec::new();

    for param in &method.params {
        let ident = &param.ident;
        call_args.push(quote!(#ident));

        let param_ty = &param.user_ty;
        param_tokens.push(quote!(#ident: #param_ty));
    }

    let fn_inputs = if param_tokens.is_empty() {
        quote!()
    } else {
        quote!(#(#param_tokens),*)
    };

    let component_init = match method.receiver_kind {
        ReceiverKind::Ref => quote! { let #component_ident = #component_type::default(); },
        ReceiverKind::RefMut | ReceiverKind::Value => {
            quote! { let mut #component_ident = #component_type::default(); }
        }
    };

    let call_expr = quote! { #component_ident.#fn_ident(#(#call_args),*) };

    let output = match &method.return_info {
        MethodReturn::Unit => quote!(),
        MethodReturn::Type { user_ty, .. } => {
            let user_ty = user_ty.as_ref();
            quote!(-> #user_ty)
        }
    };

    let body = match &method.return_info {
        MethodReturn::Unit => quote! {
            #component_init
            #call_expr;
        },
        MethodReturn::Type { .. } => {
            quote! {
                #component_init
                #call_expr
            }
        }
    };

    quote! {
        #(#doc_attrs)*
        fn #fn_ident(#fn_inputs) #output {
            #body
        }
    }
}

fn build_custom_with_entries(
    exported_types: &[ExportedTypeDef],
    interface_path: &str,
    module_prefix: Option<&syn::Path>,
    custom_type_paths: &HashMap<String, Vec<String>>,
) -> (Vec<TokenStream2>, Vec<String>) {
    let mut tokens = Vec::new();
    let mut debug = Vec::new();

    for def in exported_types {
        let wit_path_str = format!("{interface_path}/{}", def.wit_name);
        let wit_path = Literal::string(&wit_path_str);
        let type_ident = format_ident!("{}", def.rust_name);
        // Prefer the fully-qualified path discovered while scanning method signatures or exported
        // fields. These paths already include any crate/module prefixes, so they work even when
        // the type lives outside the component's module.
        let type_tokens = if let Some(segments) = custom_type_paths.get(&def.wit_name) {
            build_path_tokens(segments, &type_ident)
        } else if let Some(prefix) = module_prefix {
            // Fallback to the component's module prefix when no explicit path was collected. This
            // preserves the old behaviour for types declared alongside the component.
            quote!(#prefix :: #type_ident)
        } else {
            quote!(crate :: #type_ident)
        };

        debug.push(format!("{wit_path_str} => {type_tokens}"));
        tokens.push(quote! { #wit_path: #type_tokens, });
    }

    (tokens, debug)
}

fn component_module_prefix(component_type: &Type) -> Option<syn::Path> {
    if let Type::Path(type_path) = component_type {
        let mut path = type_path.path.clone();
        if path.segments.len() <= 1 {
            return None;
        }
        path.segments.pop();
        Some(path)
    } else {
        None
    }
}

fn record_type_path(
    paths: &mut HashMap<String, Vec<String>>,
    type_ref: &TypeRef,
    module_prefix_segments: Option<&[String]>,
) {
    if !type_ref.is_custom {
        return;
    }

    let mut segments = type_ref.path.clone();
    // Normalise `self::` and `super::` prefixes relative to the module where the component impl
    // lives so the generated path points at the original user type rather than the generated
    // bindings module.
    if let Some(first) = segments.first().cloned() {
        match first.as_str() {
            "self" => {
                segments.remove(0);
                if let Some(prefix) = module_prefix_segments {
                    let mut resolved = prefix.to_vec();
                    resolved.extend(segments);
                    segments = resolved;
                }
            }
            "super" => {
                let super_count = segments.iter().take_while(|segment| *segment == "super").count();
                let mut resolved =
                    module_prefix_segments.map(|prefix| prefix.to_vec()).unwrap_or_default();
                if super_count > resolved.len() {
                    resolved.clear();
                } else {
                    for _ in 0..super_count {
                        let _ = resolved.pop();
                    }
                }
                segments =
                    resolved.into_iter().chain(segments.into_iter().skip(super_count)).collect();
            }
            "crate" => {}
            _ => {}
        }
    }

    // Give single-segment paths a module prefix so we don't generate bare identifiers that fail to
    // resolve outside the component module.
    if segments.len() <= 1
        && let Some(last) = segments.last().cloned()
        && let Some(prefix) = module_prefix_segments
    {
        let mut resolved = prefix.to_vec();
        resolved.push(last);
        segments = resolved;
    }

    paths.entry(type_ref.wit_name.clone()).or_insert(segments);
}

fn collect_custom_type_paths(
    exported_types: &[ExportedTypeDef],
    methods: &[ComponentMethod],
    module_prefix_segments: Option<&[String]>,
) -> HashMap<String, Vec<String>> {
    let mut paths = HashMap::new();

    for def in exported_types {
        match &def.kind {
            ExportedTypeKind::Record { fields } => {
                for field in fields {
                    record_type_path(&mut paths, &field.ty, module_prefix_segments);
                }
            }
            ExportedTypeKind::Variant { variants } => {
                for variant in variants {
                    if let Some(payload) = &variant.payload {
                        record_type_path(&mut paths, payload, module_prefix_segments);
                    }
                }
            }
        }
    }

    for method in methods {
        for param in &method.params {
            record_type_path(&mut paths, &param.type_ref, module_prefix_segments);
        }
        if let MethodReturn::Type { type_ref, .. } = &method.return_info {
            record_type_path(&mut paths, type_ref, module_prefix_segments);
        }
    }

    paths
}

fn build_path_tokens(segments: &[String], type_ident: &Ident) -> TokenStream2 {
    if segments.is_empty() {
        return quote!(crate :: #type_ident);
    }

    let mut modules: Vec<String> = segments.to_vec();
    let type_name = type_ident.to_string();
    if modules.last().map(|seg| seg == &type_name).unwrap_or(false) {
        modules.pop();
    }

    let mut iter = modules.iter();
    let mut tokens: Option<TokenStream2> = None;

    if let Some(first) = iter.next() {
        tokens = Some(match first.as_str() {
            "crate" => quote!(crate),
            "self" => quote!(self),
            "super" => quote!(super),
            other => {
                let ident = format_ident!("{}", other);
                quote!(crate :: #ident)
            }
        });
    }

    for segment in iter {
        let ident = format_ident!("{}", segment);
        tokens = Some(match tokens {
            Some(existing) => quote!(#existing :: #ident),
            None => quote!(crate :: #ident),
        });
    }

    let base = tokens.unwrap_or_else(|| quote!(crate));
    quote!(#base :: #type_ident)
}

/// Parses a public inherent method and extracts the metadata necessary to export it via WIT.
fn parse_component_method(
    method: &ImplItemFn,
    exported_types: &HashMap<String, ExportedTypeDef>,
    is_auth_script: bool,
) -> Result<(ComponentMethod, BTreeSet<String>), syn::Error> {
    if method.sig.constness.is_some() {
        return Err(syn::Error::new(
            method.sig.ident.span(),
            "component methods cannot be `const`",
        ));
    }
    if method.sig.asyncness.is_some() {
        return Err(syn::Error::new(
            method.sig.ident.span(),
            "component methods cannot be `async`",
        ));
    }
    if method.sig.unsafety.is_some() {
        return Err(syn::Error::new(
            method.sig.ident.span(),
            "component methods cannot be `unsafe`",
        ));
    }
    if method.sig.abi.is_some() {
        return Err(syn::Error::new(
            method.sig.ident.span(),
            "component methods cannot specify an `extern` ABI",
        ));
    }
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            method.sig.generics.span(),
            "component methods cannot be generic",
        ));
    }
    if method.sig.variadic.is_some() {
        return Err(syn::Error::new(
            method.sig.ident.span(),
            "variadic component methods are unsupported",
        ));
    }

    let mut inputs_iter = method.sig.inputs.iter();
    let receiver = inputs_iter.next().ok_or_else(|| {
        syn::Error::new(
            method.sig.span(),
            "component methods must accept `self`, `&self`, or `&mut self` as the first argument",
        )
    })?;

    let receiver_kind = match receiver {
        FnArg::Receiver(recv) => match (&recv.reference, recv.mutability) {
            (Some(_), Some(_)) => ReceiverKind::RefMut,
            (Some(_), None) => ReceiverKind::Ref,
            (None, _) => ReceiverKind::Value,
        },
        FnArg::Typed(other) => {
            return Err(syn::Error::new(
                other.span(),
                "component methods must use an explicit receiver",
            ));
        }
    };

    let mut params = Vec::new();
    let mut type_imports = BTreeSet::new();

    for arg in inputs_iter {
        match arg {
            FnArg::Typed(pat_type) => {
                let ident = match pat_type.pat.as_ref() {
                    syn::Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "component method arguments must be simple identifiers",
                        ));
                    }
                };

                let user_ty = (*pat_type.ty).clone();
                let type_ref = map_type_to_type_ref(&pat_type.ty, exported_types)?;
                if !type_ref.is_custom {
                    type_imports.insert(type_ref.wit_name.clone());
                }

                params.push(MethodParam {
                    ident: ident.clone(),
                    user_ty,
                    type_ref,
                    wit_param_name: to_kebab_case(&ident.to_string()),
                });
            }
            FnArg::Receiver(other) => {
                return Err(syn::Error::new(
                    other.span(),
                    "component methods support a single receiver argument",
                ));
            }
        }
    }

    let return_info = match &method.sig.output {
        ReturnType::Default => MethodReturn::Unit,
        ReturnType::Type(_, ty) if is_unit_type(ty) => MethodReturn::Unit,
        ReturnType::Type(_, ty) => {
            let type_ref = map_type_to_type_ref(ty, exported_types)?;
            if !type_ref.is_custom {
                type_imports.insert(type_ref.wit_name.clone());
            }
            MethodReturn::Type {
                user_ty: ty.clone(),
                type_ref,
            }
        }
    };

    if is_auth_script {
        validate_auth_script_signature(method, &params, &return_info)?;
    }

    let doc_attrs = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .cloned()
        .collect();

    let component_method = ComponentMethod {
        fn_ident: method.sig.ident.clone(),
        doc_attrs,
        params,
        receiver_kind,
        return_info,
        wit_name: to_kebab_case(&method.sig.ident.to_string()),
        is_auth_script,
    };

    Ok((component_method, type_imports))
}

/// Attempts to recover the final identifier from a type path for use with `bindings::export!`.
fn extract_type_ident(ty: &Type) -> Option<syn::Ident> {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|segment| segment.ident.clone()),
        Type::Group(group) => extract_type_ident(&group.elem),
        Type::Paren(paren) => extract_type_ident(&paren.elem),
        _ => None,
    }
}

/// Maps a Rust type used in the public interface to the corresponding WIT core-types identifier.
/// Determines whether a type represents the unit type `()`.
fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Determines whether a type path resolves to a simple identifier with the given name.
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

/// Converts a snake_case identifier into kebab-case.
fn to_kebab_case(name: &str) -> String {
    name.to_kebab_case()
}

/// Converts a kebab-case identifier into snake_case.
fn to_snake_case(name: &str) -> String {
    name.to_snake_case()
}

/// Synthesizes the `Default` implementation for the component struct using the collected storage
/// initializers.
fn generate_default_impl(
    struct_name: &syn::Ident,
    field_inits: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        impl Default for #struct_name {
            fn default() -> Self {
                Self {
                    #(#field_inits),*
                }
            }
        }
    }
}

/// Validates the signature requirements for a method annotated with `#[auth_script]`.
fn validate_auth_script_signature(
    method: &ImplItemFn,
    params: &[MethodParam],
    return_info: &MethodReturn,
) -> Result<(), syn::Error> {
    if params.len() != 1 || !is_type_named(&params[0].user_ty, "Word") {
        return Err(syn::Error::new(
            method.sig.span(),
            "`#[auth_script]` methods must accept exactly one `Word` argument (excluding `self`)",
        ));
    }

    if !matches!(return_info, MethodReturn::Unit) {
        return Err(syn::Error::new(
            method.sig.output.span(),
            "`#[auth_script]` methods must return `()`",
        ));
    }

    Ok(())
}

/// Emits frontend-only component metadata into a dedicated custom section.
fn generate_frontend_link_section(methods: &[ComponentMethod]) -> proc_macro2::TokenStream {
    let auth_export_name = methods
        .iter()
        .find(|method| method.is_auth_script)
        .map(|method| method.wit_name.as_str());

    let Some(auth_export_name) = auth_export_name else {
        return quote! {};
    };

    let metadata_bytes = encode_frontend_metadata(Some(auth_export_name));
    let metadata_len = metadata_bytes.len();
    let encoded_bytes = Literal::byte_string(&metadata_bytes);
    let uniqueness_guard_symbol = AUTH_SCRIPT_UNIQUENESS_GUARD_SYMBOL;

    quote! {
        const _: () = {
            // A crate may contain exactly one `#[auth_script]` method. Reusing a fixed symbol name
            // lets the linker reject duplicates across modules or impl blocks.
            #[doc(hidden)]
            #[used]
            #[unsafe(export_name = #uniqueness_guard_symbol)]
            static __miden_auth_script_uniqueness_guard: u8 = 0;
        };

        #[unsafe(
            // Keep the Mach-O-friendly `segment,section` naming scheme used by the main metadata
            // section so the linker preserves these bytes in test and release builds.
            link_section = #FRONTEND_METADATA_LINK_SECTION
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_ACCOUNT_COMPONENT_FRONTEND_METADATA_BYTES: [u8; #metadata_len] = *#encoded_bytes;
    }
}

/// Encodes the frontend-only metadata payload consumed by the Wasm frontend.
fn encode_frontend_metadata(auth_export_name: Option<&str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(1);
    bytes.push(u8::from(auth_export_name.is_some()));

    if let Some(export_name) = auth_export_name {
        let name_bytes = export_name.as_bytes();
        bytes.push(
            u8::try_from(name_bytes.len())
                .expect("component frontend metadata supports auth export names up to 255 bytes"),
        );
        bytes.extend_from_slice(name_bytes);
    }

    bytes
}

/// Emits the static metadata blob inside the `rodata,miden_account` link section.
fn generate_link_section(metadata_bytes: &[u8]) -> proc_macro2::TokenStream {
    let link_section_bytes_len = metadata_bytes.len();
    let encoded_bytes_str = Literal::byte_string(metadata_bytes);

    quote! {
        #[unsafe(
            // to test it in the integration(this crate) tests the section name needs to make mach-o section
            // specifier happy and to have "segment and section separated by comma"
            link_section = "rodata,miden_account"
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_ACCOUNT_COMPONENT_METADATA_BYTES: [u8; #link_section_bytes_len] = *#encoded_bytes_str;
    }
}

/// Returns true if any authentication marker attribute is present.
fn has_auth_script_marker_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_auth_script_marker_attr)
}

fn is_attr_named(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|seg| seg.ident == name && matches!(seg.arguments, PathArguments::None))
}

/// Returns true if an attribute marks a method as the authentication procedure entrypoint.
fn is_auth_script_marker_attr(attr: &Attribute) -> bool {
    is_attr_named(attr, AUTH_SCRIPT_ATTR) || is_doc_marker_attr(attr, AUTH_SCRIPT_DOC_MARKER)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use syn::parse_quote;

    use super::*;

    #[test]
    fn record_type_path_defaults_to_crate_root() {
        let mut paths = HashMap::new();
        let type_ref = TypeRef {
            wit_name: "struct-a".into(),
            is_custom: true,
            path: vec!["StructA".into()],
        };

        record_type_path(&mut paths, &type_ref, None);

        assert_eq!(paths.get("struct-a"), Some(&vec!["StructA".to_string()]));
    }

    #[test]
    fn record_type_path_applies_module_prefix() {
        let mut paths = HashMap::new();
        let type_ref = TypeRef {
            wit_name: "struct-a".into(),
            is_custom: true,
            path: vec!["StructA".into()],
        };
        let prefix = vec!["foo".to_string(), "bar".to_string()];

        record_type_path(&mut paths, &type_ref, Some(prefix.as_slice()));

        assert_eq!(
            paths.get("struct-a"),
            Some(&vec!["foo".to_string(), "bar".to_string(), "StructA".to_string()])
        );
    }

    #[test]
    fn record_type_path_resolves_super_segments() {
        let mut paths = HashMap::new();
        let type_ref = TypeRef {
            wit_name: "struct-a".into(),
            is_custom: true,
            path: vec!["super".into(), "StructA".into()],
        };
        let prefix = vec!["foo".to_string(), "bar".to_string()];

        record_type_path(&mut paths, &type_ref, Some(prefix.as_slice()));

        assert_eq!(paths.get("struct-a"), Some(&vec!["foo".to_string(), "StructA".to_string()]));
    }

    #[test]
    fn build_path_tokens_generates_absolute_path() {
        let segments = vec!["foo".to_string(), "bar".to_string(), "StructA".to_string()];
        let ident = format_ident!("StructA");
        let tokens = build_path_tokens(&segments, &ident).to_string();
        assert_eq!(tokens, "crate :: foo :: bar :: StructA");
    }

    #[test]
    fn build_path_tokens_defaults_to_crate_root_for_single_segment() {
        let segments = vec!["StructA".to_string()];
        let ident = format_ident!("StructA");
        let tokens = build_path_tokens(&segments, &ident).to_string();
        assert_eq!(tokens, "crate :: StructA");
    }

    #[test]
    fn build_custom_with_entries_prefers_custom_paths() {
        let exported_types = vec![ExportedTypeDef {
            rust_name: "StructA".into(),
            wit_name: "struct-a".into(),
            kind: ExportedTypeKind::Record { fields: Vec::new() },
        }];
        let interface_path = "miden:component/path";
        let module_prefix: syn::Path = syn::parse_quote!(module::account);
        let mut custom_paths = HashMap::new();
        custom_paths.insert("struct-a".into(), vec!["types".into(), "StructA".into()]);

        let (entries, _) = build_custom_with_entries(
            &exported_types,
            interface_path,
            Some(&module_prefix),
            &custom_paths,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].to_string(),
            "\"miden:component/path/struct-a\" : crate :: types :: StructA ,"
        );
    }

    #[test]
    fn auth_script_methods_preserve_user_defined_names() {
        let mut method: ImplItemFn = parse_quote! {
            #[auth_script]
            pub fn whatever_name(&mut self, arg: Word) {}
        };
        let exported_types = HashMap::new();
        let is_auth_script = has_auth_script_marker_attr(&method.attrs);
        method.attrs.retain(|attr| !is_auth_script_marker_attr(attr));

        let (parsed_method, _) =
            parse_component_method(&method, &exported_types, is_auth_script).unwrap();

        assert_eq!(parsed_method.fn_ident.to_string(), "whatever_name");
        assert_eq!(parsed_method.wit_name, "whatever-name");
        assert!(parsed_method.is_auth_script);
    }

    #[test]
    fn auth_script_methods_require_word_argument() {
        let mut method: ImplItemFn = parse_quote! {
            #[auth_script]
            pub fn auth_procedure(&mut self, arg: u32) {}
        };
        let exported_types = HashMap::new();
        let is_auth_script = has_auth_script_marker_attr(&method.attrs);
        method.attrs.retain(|attr| !is_auth_script_marker_attr(attr));

        let err = match parse_component_method(&method, &exported_types, is_auth_script) {
            Ok(_) => panic!("expected `#[auth_script]` validation to reject non-`Word` arguments"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("exactly one `Word` argument"));
    }

    #[test]
    fn auth_script_frontend_metadata_emits_project_wide_uniqueness_guard() {
        let mut method: ImplItemFn = parse_quote! {
            #[auth_script]
            pub fn whatever_name(&mut self, arg: Word) {}
        };
        let exported_types = HashMap::new();
        let is_auth_script = has_auth_script_marker_attr(&method.attrs);
        method.attrs.retain(|attr| !is_auth_script_marker_attr(attr));

        let (parsed_method, _) =
            parse_component_method(&method, &exported_types, is_auth_script).unwrap();
        let tokens = generate_frontend_link_section(&[parsed_method]).to_string();

        assert!(tokens.contains(AUTH_SCRIPT_UNIQUENESS_GUARD_SYMBOL));
    }

    #[test]
    fn authentication_components_require_exactly_one_auth_script() {
        let err =
            validate_auth_script_count(Some(AUTH_COMPONENT_PROJECT_KIND), 0, Span2::call_site())
                .expect_err("expected authentication components to require an auth script");

        assert!(
            err.to_string()
                .contains("authentication components require exactly one `#[auth_script]` method")
        );

        validate_auth_script_count(Some(AUTH_COMPONENT_PROJECT_KIND), 1, Span2::call_site())
            .expect("expected exactly one auth script to be accepted");
    }
}
