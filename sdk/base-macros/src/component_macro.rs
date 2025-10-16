use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write,
    fs,
    io::ErrorKind,
    path::Path,
    str::FromStr,
};

use heck::{ToKebabCase, ToSnakeCase};
use miden_objects::{account::AccountType, utils::Serializable};
use proc_macro::Span;
use proc_macro2::{Ident, Literal, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use semver::Version;
use syn::{
    spanned::Spanned, Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, ItemStruct, ReturnType,
    Type, Visibility,
};
use toml::Value;

use crate::{
    account_component_metadata::AccountComponentMetadataBuilder,
    types::{map_type_to_wit_type, registered_export_types, ExportedTypeDef, WitType},
};

/// Cargo metadata relevant for the `#[component]` macro expansion.
struct CargoMetadata {
    name: String,
    version: Version,
    description: String,
    supported_types: Vec<String>,
    component_package: Option<String>,
}

/// Parsed arguments collected from a `#[storage(...)]` attribute.
struct StorageAttributeArgs {
    slot: u8,
    description: Option<String>,
    type_attr: Option<String>,
}

/// Default version appended to component WIT package identifiers when a version is not provided in
/// manifest metadata.
const COMPONENT_PACKAGE_VERSION: &str = "1.0.0";

/// Fully-qualified identifier for the core types package used by exported component interfaces.
const CORE_TYPES_PACKAGE: &str = "miden:base/core-types@1.0.0";

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
    wit_type: WitType,
    wit_type_name: String,
    wit_param_name: String,
}

enum MethodReturn {
    Unit,
    Type {
        user_ty: Box<syn::Type>,
        wit_type: WitType,
        wit_type_name: String,
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
            let field_inits = process_storage_fields(fields, &mut acc_builder)?;
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

    let component_metadata = acc_builder.build();

    let mut metadata_bytes = component_metadata.to_bytes();
    let padded_len = metadata_bytes.len().div_ceil(16) * 16;
    metadata_bytes.resize(padded_len, 0);

    let link_section = generate_link_section(&metadata_bytes);

    Ok(quote! {
        #input_struct
        #default_impl
        #link_section
    })
}

/// Expands the `#[component]` attribute applied to an inherent implementation block by generating
/// the inline WIT interface, invoking `miden::generate!`, and wiring the guest trait implementation.
fn expand_component_impl(
    call_site_span: Span,
    impl_block: ItemImpl,
) -> Result<TokenStream2, syn::Error> {
    if impl_block.trait_.is_some() {
        return Err(syn::Error::new(
            impl_block.span(),
            "The `component` macro does not support trait implementations.",
        ));
    }

    let component_type = (*impl_block.self_ty).clone();
    let struct_ident = extract_type_ident(&component_type).ok_or_else(|| {
        syn::Error::new(
            impl_block.self_ty.span(),
            "Failed to determine the struct name targeted by this implementation.",
        )
    })?;

    let metadata = get_package_metadata(call_site_span)?;
    let component_package = metadata.component_package.clone().ok_or_else(|| {
        syn::Error::new(
            call_site_span.into(),
            "package.metadata.component.package in Cargo.toml is required to derive the component \
             interface",
        )
    })?;

    let interface_name = metadata.name.to_kebab_case();
    let interface_module = to_snake_case(&interface_name);
    let world_name = format!("{interface_name}-world");

    let mut methods = Vec::new();
    let mut type_imports = BTreeSet::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            if !matches!(method.vis, Visibility::Public(_)) {
                continue;
            }

            let (parsed_method, imports) = parse_component_method(method)?;
            type_imports.extend(imports);
            methods.push(parsed_method);
        }
    }

    let mut exported_types = registered_export_types();
    exported_types.sort_by(|a, b| a.wit_name.cmp(&b.wit_name));
    let exported_types_map: HashMap<_, _> =
        exported_types.iter().cloned().map(|def| (def.wit_name.clone(), def)).collect();
    let bindings_segments = build_bindings_module_segments(&component_package, &interface_module);
    let conversion_impls: Vec<TokenStream2> = exported_types_map
        .values()
        .map(|def| generate_export_type_impls(def, &bindings_segments))
        .collect();

    let wit_source = build_component_wit(
        &component_package,
        &interface_name,
        &world_name,
        &type_imports,
        &methods,
        &exported_types,
    );
    write_component_wit_file(call_site_span, &wit_source, &interface_name)?;
    let inline_literal = Literal::string(&wit_source);

    let guest_trait_path = build_guest_trait_path(&component_package, &interface_module)?;
    let guest_methods: Vec<TokenStream2> = methods
        .iter()
        .map(|method| {
            render_guest_method(method, &component_type, &bindings_segments, &exported_types_map)
        })
        .collect();

    Ok(quote! {
        ::miden::generate!(inline = #inline_literal);
        #(#conversion_impls)*
        #impl_block
        impl #guest_trait_path for #component_type {
            #(#guest_methods)*
        }
        self::bindings::export!(#struct_ident);
    })
}

/// Renders the inline WIT source describing the component interface exported by the `impl` block.
fn build_component_wit(
    component_package: &str,
    interface_name: &str,
    world_name: &str,
    type_imports: &BTreeSet<String>,
    methods: &[ComponentMethod],
    exported_types: &[ExportedTypeDef],
) -> String {
    let package_with_version = if component_package.contains('@') {
        component_package.to_string()
    } else {
        format!("{component_package}@{COMPONENT_PACKAGE_VERSION}")
    };

    let mut wit_source = String::new();
    let _ = writeln!(wit_source, "// This file is auto-generated by the `#[component]` macro.");
    let _ = writeln!(wit_source, "// Do not edit this file manually.");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "package {package_with_version};");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "use {CORE_TYPES_PACKAGE};");
    wit_source.push('\n');

    let mut combined_core_imports = type_imports.clone();
    for exported in exported_types {
        for field in &exported.fields {
            if let WitType::Core(core_ty) = &field.ty {
                combined_core_imports.insert(core_ty.clone());
            }
        }
    }

    let _ = writeln!(wit_source, "interface {interface_name} {{");

    if !combined_core_imports.is_empty() {
        let imports = combined_core_imports.iter().cloned().collect::<Vec<_>>().join(", ");
        let _ = writeln!(wit_source, "    use core-types.{{{imports}}};");
        wit_source.push('\n');
    }

    for exported in exported_types {
        let _ = writeln!(wit_source, "    record {} {{", exported.wit_name);
        for field in &exported.fields {
            let field_name = to_kebab_case(&field.name);
            let _ = writeln!(wit_source, "        {}: {},", field_name, field.ty.as_str());
        }
        let _ = writeln!(wit_source, "    }}\n");
    }

    for method in methods {
        let signature = if method.params.is_empty() {
            match &method.return_info {
                MethodReturn::Unit => format!("    {}: func();", method.wit_name),
                MethodReturn::Type { wit_type_name, .. } => {
                    format!("    {}: func() -> {};", method.wit_name, wit_type_name)
                }
            }
        } else {
            let params = method
                .params
                .iter()
                .map(|param| format!("{}: {}", param.wit_param_name, param.wit_type_name))
                .collect::<Vec<_>>()
                .join(", ");
            match &method.return_info {
                MethodReturn::Unit => format!("    {}: func({});", method.wit_name, params),
                MethodReturn::Type { wit_type_name, .. } => {
                    format!("    {}: func({}) -> {};", method.wit_name, params, wit_type_name)
                }
            }
        };
        let _ = writeln!(wit_source, "{signature}");
    }

    let _ = writeln!(wit_source, "}}");
    wit_source.push('\n');
    let _ = writeln!(wit_source, "world {world_name} {{");
    let _ = writeln!(wit_source, "    export {interface_name};");
    let _ = writeln!(wit_source, "}}");

    wit_source
}

/// Writes the generated component WIT to the crate's `wit` directory so that dependent targets can
/// reference it via manifest metadata.
fn write_component_wit_file(
    call_site_span: Span,
    wit_source: &str,
    interface_name: &str,
) -> Result<(), syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|err| {
        syn::Error::new(call_site_span.into(), format!("failed to read CARGO_MANIFEST_DIR: {err}"))
    })?;

    let wit_dir = Path::new(&manifest_dir).join("wit");
    fs::create_dir_all(&wit_dir).map_err(|err| {
        syn::Error::new(
            call_site_span.into(),
            format!("failed to create WIT output directory '{}': {err}", wit_dir.display()),
        )
    })?;

    let wit_path = wit_dir.join(format!("{interface_name}.wit"));

    let needs_write = match fs::read_to_string(&wit_path) {
        Ok(existing) => existing != wit_source,
        Err(err) if err.kind() == ErrorKind::NotFound => true,
        Err(err) => {
            return Err(syn::Error::new(
                call_site_span.into(),
                format!("failed to read existing WIT file '{}': {err}", wit_path.display()),
            ));
        }
    };

    if needs_write {
        fs::write(&wit_path, wit_source).map_err(|err| {
            syn::Error::new(
                call_site_span.into(),
                format!("failed to write WIT file '{}': {err}", wit_path.display()),
            )
        })?;
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
fn render_guest_method(
    method: &ComponentMethod,
    component_type: &Type,
    bindings_segments: &[proc_macro2::Ident],
    exported_types: &HashMap<String, ExportedTypeDef>,
) -> TokenStream2 {
    let fn_ident = &method.fn_ident;
    let doc_attrs = &method.doc_attrs;
    let component_ident = format_ident!("__component_instance");
    let bindings_base = quote! { self::bindings::exports #( :: #bindings_segments )* };

    let mut param_tokens = Vec::new();
    let mut call_args = Vec::new();
    let mut pre_call = Vec::new();

    for param in &method.params {
        let ident = &param.ident;
        call_args.push(quote!(#ident));

        let param_ty = match param.wit_type {
            WitType::Core(_) => {
                let ty = &param.user_ty;
                quote!(#ty)
            }
            WitType::Custom(ref wit_name) => {
                let def = exported_types.get(wit_name).expect("unknown exported type");
                let type_ident = format_ident!("{}", def.rust_name);
                quote!(#bindings_base :: #type_ident)
            }
        };

        param_tokens.push(quote!(#ident: #param_ty));

        if matches!(param.wit_type, WitType::Custom(_)) {
            let user_ty = &param.user_ty;
            pre_call.push(quote! {
                let #ident: #user_ty = #ident.into();
            });
        }
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
        MethodReturn::Type {
            wit_type, user_ty, ..
        } => match wit_type {
            WitType::Core(_) => quote!(-> #user_ty),
            WitType::Custom(wit_name) => {
                let def = exported_types.get(wit_name).expect("unknown exported type");
                let type_ident = format_ident!("{}", def.rust_name);
                quote!(-> #bindings_base :: #type_ident)
            }
        },
    };

    let body = match &method.return_info {
        MethodReturn::Unit => quote! {
            #component_init
            #(#pre_call)*
            #call_expr;
        },
        MethodReturn::Type { wit_type, .. } => match wit_type {
            WitType::Core(_) => quote! {
                #component_init
                #(#pre_call)*
                #call_expr
            },
            WitType::Custom(_) => quote! {
                #component_init
                #(#pre_call)*
                let result = #call_expr;
                result.into()
            },
        },
    };

    quote! {
        #(#doc_attrs)*
        fn #fn_ident(#fn_inputs) #output {
            #body
        }
    }
}

fn build_bindings_module_segments(component_package: &str, interface_module: &str) -> Vec<Ident> {
    let mut segments = Vec::new();
    for segment in component_package.split([':', '/']) {
        if segment.is_empty() {
            continue;
        }
        segments.push(format_ident!("{}", to_snake_case(segment)));
    }
    segments.push(format_ident!("{}", to_snake_case(interface_module)));
    segments
}

fn generate_export_type_impls(def: &ExportedTypeDef, bindings_segments: &[Ident]) -> TokenStream2 {
    let struct_ident = format_ident!("{}", def.rust_name);
    let binding_type_ident = format_ident!("{}", def.rust_name);
    let bindings_path =
        quote! { self::bindings::exports #( :: #bindings_segments )* :: #binding_type_ident };

    if def.fields.is_empty() {
        return quote! {
            impl From<#bindings_path> for #struct_ident {
                fn from(_: #bindings_path) -> Self {
                    Self
                }
            }

            impl From<#struct_ident> for #bindings_path {
                fn from(_: #struct_ident) -> Self {
                    Self
                }
            }
        };
    }

    let field_idents: Vec<_> =
        def.fields.iter().map(|field| format_ident!("{}", field.name)).collect();

    let binding_to_user_fields: Vec<_> = def
        .fields
        .iter()
        .map(|field| {
            let ident = format_ident!("{}", field.name);
            match field.ty {
                WitType::Core(_) => quote! { #ident },
                WitType::Custom(_) => quote! { #ident.into() },
            }
        })
        .collect();

    let user_to_binding_fields: Vec<_> = def
        .fields
        .iter()
        .map(|field| {
            let ident = format_ident!("{}", field.name);
            match field.ty {
                WitType::Core(_) => quote! { #ident },
                WitType::Custom(_) => quote! { #ident.into() },
            }
        })
        .collect();

    quote! {
        impl From<#bindings_path> for #struct_ident {
            fn from(value: #bindings_path) -> Self {
                let #bindings_path { #( #field_idents ),* } = value;
                Self {
                    #( #field_idents: #binding_to_user_fields ),*
                }
            }
        }

        impl From<#struct_ident> for #bindings_path {
            fn from(value: #struct_ident) -> Self {
                let #struct_ident { #( #field_idents ),* } = value;
                Self {
                    #( #field_idents: #user_to_binding_fields ),*
                }
            }
        }
    }
}

/// Parses a public inherent method and extracts the metadata necessary to export it via WIT.
fn parse_component_method(
    method: &ImplItemFn,
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
                let wit_type = map_type_to_wit_type(&pat_type.ty)?;
                if let WitType::Core(core_ty) = &wit_type {
                    type_imports.insert(core_ty.clone());
                }

                params.push(MethodParam {
                    ident: ident.clone(),
                    user_ty,
                    wit_type: wit_type.clone(),
                    wit_type_name: wit_type.as_str().to_string(),
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
            let wit_type = map_type_to_wit_type(ty)?;
            if let WitType::Core(core_ty) = &wit_type {
                type_imports.insert(core_ty.clone());
            }
            MethodReturn::Type {
                user_ty: ty.clone(),
                wit_type: wit_type.clone(),
                wit_type_name: wit_type.as_str().to_string(),
            }
        }
    };

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

/// Converts a snake_case identifier into kebab-case.
fn to_kebab_case(name: &str) -> String {
    name.to_kebab_case()
}

/// Converts a kebab-case identifier into snake_case.
fn to_snake_case(name: &str) -> String {
    name.to_snake_case()
}

/// Reads component metadata (name/description/version/supported types) from the enclosing package
/// manifest.
fn get_package_metadata(call_site_span: Span) -> Result<CargoMetadata, syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let current_dir = Path::new(&manifest_dir);

    let cargo_toml_path = current_dir.join("Cargo.toml");
    if !cargo_toml_path.is_file() {
        return Ok(CargoMetadata {
            name: String::new(),
            version: Version::new(0, 0, 1),
            description: String::new(),
            supported_types: vec![],
            component_package: None,
        });
    }

    let cargo_toml_content = fs::read_to_string(&cargo_toml_path).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to read {}: {}", cargo_toml_path.display(), e),
        )
    })?;
    let cargo_toml: Value = cargo_toml_content.parse::<Value>().map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to parse {}: {}", cargo_toml_path.display(), e),
        )
    })?;

    let package_table = cargo_toml.get("package").ok_or_else(|| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Cargo.toml ({}) does not contain a [package] table",
                cargo_toml_path.display()
            ),
        )
    })?;

    let name = package_table
        .get("name")
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!("Missing 'name' field in [package] table of {}", cargo_toml_path.display()),
            )
        })?;

    let version_str = package_table
        .get("version")
        .and_then(|v| v.as_str())
        .or_else(|| {
            let base = env!("CARGO_MANIFEST_DIR");
            if base.ends_with(cargo_toml_path.parent().unwrap().to_str().unwrap()) {
                Some("0.0.0")
            } else {
                None
            }
        })
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!(
                    "Missing 'version' field in [package] table of {} (version.workspace = true \
                     is not yet supported for external crates)",
                    cargo_toml_path.display()
                ),
            )
        })?;

    let version = Version::parse(version_str).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Failed to parse version '{}' from {}: {}",
                version_str,
                cargo_toml_path.display(),
                e
            ),
        )
    })?;

    let description = package_table
        .get("description")
        .and_then(|d| d.as_str())
        .map(String::from)
        .unwrap_or_default();

    let supported_types = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("metadata"))
        .and_then(|m| m.get("miden"))
        .and_then(|m| m.get("supported-types"))
        .and_then(|st| st.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let component_package = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("metadata"))
        .and_then(|meta| meta.get("component"))
        .and_then(|component| component.get("package"))
        .and_then(|pkg_val| pkg_val.as_str())
        .map(|pkg| pkg.to_string());

    Ok(CargoMetadata {
        name,
        version,
        description,
        supported_types,
        component_package,
    })
}

/// Attempts to parse a `#[storage(...)]` attribute and returns the extracted arguments.
fn parse_storage_attribute(
    attr: &syn::Attribute,
) -> Result<Option<StorageAttributeArgs>, syn::Error> {
    if !attr.path().is_ident("storage") {
        return Ok(None);
    }

    let mut slot_value = None;
    let mut description_value = None;
    let mut type_value = None;

    let list = match &attr.meta {
        syn::Meta::List(list) => list,
        _ => return Err(syn::Error::new(attr.span(), "Expected #[storage(...)]")),
    };

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("slot") {
            let value_stream;
            syn::parenthesized!(value_stream in meta.input);
            let lit: syn::LitInt = value_stream.parse()?;
            slot_value = Some(lit.base10_parse::<u8>()?);
            Ok(())
        } else if meta.path.is_ident("description") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            description_value = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("type") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            type_value = Some(lit.value());
            Ok(())
        } else {
            Err(meta.error("unrecognized storage attribute argument"))
        }
    });

    list.parse_args_with(parser)?;

    let slot = slot_value.ok_or_else(|| {
        syn::Error::new(attr.span(), "missing required `slot(N)` argument in `storage` attribute")
    })?;

    Ok(Some(StorageAttributeArgs {
        slot,
        description: description_value,
        type_attr: type_value,
    }))
}

/// Processes component struct fields, recording storage metadata and building default
/// initializers.
fn process_storage_fields(
    fields: &mut syn::FieldsNamed,
    builder: &mut AccountComponentMetadataBuilder,
) -> Result<Vec<proc_macro2::TokenStream>, syn::Error> {
    let mut field_inits = Vec::new();
    let mut errors = Vec::new();

    for field in fields.named.iter_mut() {
        let field_name = field.ident.as_ref().expect("Named field must have an identifier");
        let field_type = &field.ty;
        let mut storage_args = None;
        let mut attr_indices_to_remove = Vec::new();

        for (attr_idx, attr) in field.attrs.iter().enumerate() {
            match parse_storage_attribute(attr) {
                Ok(Some(args)) => {
                    if storage_args.is_some() {
                        errors.push(syn::Error::new(attr.span(), "duplicate `storage` attribute"));
                    }
                    storage_args = Some(args);
                    attr_indices_to_remove.push(attr_idx);
                }
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }

        for (removed_count, idx_to_remove) in attr_indices_to_remove.into_iter().enumerate() {
            field.attrs.remove(idx_to_remove - removed_count);
        }

        if let Some(args) = storage_args {
            let slot = args.slot;
            field_inits.push(quote! {
                #field_name: #field_type { slot: #slot }
            });

            builder.add_storage_entry(
                &field_name.to_string(),
                args.description,
                args.slot,
                field_type,
                args.type_attr,
            );
        } else {
            errors
                .push(syn::Error::new(field.span(), "field is missing the `#[storage]` attribute"));
        }
    }

    if let Some(first_error) = errors.into_iter().next() {
        Err(first_error)
    } else {
        Ok(field_inits)
    }
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
