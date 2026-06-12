use std::{
    collections::{BTreeSet, HashMap},
    env,
};

use heck::{ToKebabCase, ToSnakeCase};
use miden_project::TargetType;
use miden_protocol::utils::serde::Serializable;
use midenc_frontend_wasm_metadata::FrontendMetadata;
use proc_macro::Span;
use proc_macro2::{Ident, Literal, Span as Span2, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, ItemStruct, ItemTrait, PathArguments,
    ReturnType, TraitItem, TraitItemFn, Type, spanned::Spanned,
};

pub(crate) use crate::component_macro::storage::typecheck_storage_field;
use crate::{
    account_component_metadata::AccountComponentMetadataBuilder,
    boilerplate::runtime_boilerplate,
    component_macro::{
        generate_wit::{ComponentWitSpec, build_component_wit, write_component_wit_file},
        storage::process_storage_fields,
    },
    types::{
        ExportedTypeDef, ExportedTypeKind, TypeRef, map_type_to_type_ref, registered_export_types,
    },
    util::generate_frontend_link_section,
};

mod generate_wit;
mod storage;

/// Fully-qualified identifier for the core types package used by exported component interfaces.
const CORE_TYPES_PACKAGE: &str = "miden:base/core-types@1.0.0";
/// Attribute name used to mark the authentication procedure on a component method.
const AUTH_SCRIPT_ATTR: &str = "auth_script";
/// Helper attribute preserved by `#[auth_script]` so `#[component]` can recognize the method.
const AUTH_SCRIPT_MARKER_ATTR: &str = "miden_auth_script_requires_component";
/// Name of the hidden associated constant injected into `#[component]` traits.
///
/// The trait implementation expansion references this constant through the implemented trait (see
/// [`render_trait_marker_check`]), so forgetting `#[component]` on the trait surfaces as a
/// missing-item error naming this constant instead of silently skipping the trait-side validation.
const COMPONENT_TRAIT_MARKER_CONST: &str = "__MIDEN_COMPONENT_TRAIT_MARKER";
/// Name of the hidden inherent constant injected by `#[component_storage]`.
///
/// The trait implementation expansion references this constant on the storage type (see
/// [`render_storage_marker_check`]), so forgetting `#[component_storage]` on the storage struct
/// surfaces as a missing-item error naming this constant instead of silently producing a
/// component without storage metadata, account trait impls, or runtime boilerplate.
const COMPONENT_STORAGE_MARKER_CONST: &str = "__MIDEN_COMPONENT_STORAGE_MARKER";

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
}

/// Expands the `#[component]` attribute applied to either a component trait declaration or a trait
/// implementation block.
///
/// The trait declaration defines the component's API and is the source of the generated WIT
/// interface. The trait implementation block provides the behavior and is wired to the generated
/// guest bindings. Storage lives on a separate struct annotated with `#[component_storage]`.
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span2::call_site(), "#[component] does not accept arguments")
            .into_compile_error()
            .into();
    }

    let call_site_span = Span::call_site();
    let item_tokens: TokenStream2 = item.into();

    if let Ok(item_trait) = syn::parse2::<ItemTrait>(item_tokens.clone()) {
        match expand_component_trait(call_site_span, item_trait) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        }
    } else if let Ok(item_impl) = syn::parse2::<ItemImpl>(item_tokens.clone()) {
        match expand_component_trait_impl(call_site_span, item_impl) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        }
    } else if syn::parse2::<ItemStruct>(item_tokens).is_ok() {
        syn::Error::new(
            call_site_span.into(),
            "`#[component]` no longer applies to structs; annotate the storage struct with \
             `#[component_storage]` instead.",
        )
        .into_compile_error()
        .into()
    } else {
        syn::Error::new(
            call_site_span.into(),
            "The `component` macro only supports a component trait or a trait implementation \
             block.",
        )
        .into_compile_error()
        .into()
    }
}

/// Expands the `#[component_storage]` attribute applied to the component's storage struct.
///
/// Wires storage metadata, generates the `Default` implementation, and implements the account
/// traits required to access storage and account operations from the component's methods.
pub fn component_storage(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            Span2::call_site(),
            "#[component_storage] does not accept arguments",
        )
        .into_compile_error()
        .into();
    }

    let call_site_span = Span::call_site();
    let item_tokens: TokenStream2 = item.into();

    match syn::parse2::<ItemStruct>(item_tokens) {
        Ok(item_struct) => match expand_component_storage(call_site_span, item_struct) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        },
        Err(_) => syn::Error::new(
            call_site_span.into(),
            "`#[component_storage]` only applies to a struct declaration.",
        )
        .into_compile_error()
        .into(),
    }
}

/// Expands `#[auth_script]`.
///
/// This attribute must be applied to a method inside a `trait` annotated with `#[component]`. It
/// acts as a marker for `#[component]` so the macro can emit frontend metadata for the annotated
/// export without rewriting its user-defined name.
pub fn expand_auth_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span2::call_site(), "#[auth_script] does not accept arguments")
            .into_compile_error()
            .into();
    }

    let item_tokens: TokenStream2 = item.clone().into();
    let mut item_fn: TraitItemFn = match syn::parse2(item_tokens.clone()) {
        Ok(item_fn) => item_fn,
        Err(_) => {
            if let Ok(item_fn) = syn::parse2::<ImplItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[auth_script]` must be applied to a method inside a `#[component]` \
                     `trait`, not the implementation block",
                )
                .into_compile_error()
                .into();
            }

            if let Ok(item_fn) = syn::parse2::<syn::ItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[auth_script]` must be applied to a method inside a `#[component]` `trait`",
                )
                .into_compile_error()
                .into();
            }

            return syn::Error::new(
                Span2::call_site(),
                "`#[auth_script]` must be applied to a method inside a `#[component]` `trait`",
            )
            .into_compile_error()
            .into();
        }
    };

    // Preserve a helper attribute for `#[component]` to consume. If the surrounding trait forgets
    // `#[component]`, rustc rejects this unknown helper attribute instead of silently compiling a
    // method that emits no auth metadata.
    let marker_attr = format_ident!("{}", AUTH_SCRIPT_MARKER_ATTR);
    item_fn.attrs.push(syn::parse_quote!(#[#marker_attr]));
    quote!(#item_fn).into()
}

/// Expands the `#[component_storage]` attribute applied to a struct by wiring storage metadata and
/// link section exports.
fn expand_component_storage(
    call_site_span: Span,
    mut input_struct: ItemStruct,
) -> Result<TokenStream2, syn::Error> {
    let struct_name = &input_struct.ident;

    // The expansion emits bare-ident impls (`Default`, the account traits, the marker constant),
    // which cannot compile for a generic struct; reject it here like the sibling expansions do.
    reject_generics(&input_struct.generics, "component storage structs cannot be generic")?;

    let metadata = crate::wit_world::ManifestPackage::load_or_default(call_site_span.into())?;
    let mut acc_builder = AccountComponentMetadataBuilder::new(
        metadata.package.name().to_string(),
        metadata.package.version().into_inner().clone(),
        metadata.description.clone(),
    );

    let default_impl = match &mut input_struct.fields {
        syn::Fields::Named(fields) => {
            let storage_namespace = metadata.package.name().into_inner();
            // Slot names derive from the component's public identity (the `[lib].namespace`
            // interface segment) rather than the storage struct name, so renaming the private
            // struct cannot change deployed storage slot names.
            let component_interface = namespace_interface_segment(&metadata).to_string();
            let field_inits = process_storage_fields(
                fields,
                &mut acc_builder,
                &storage_namespace,
                &component_interface,
            )?;
            // Checked after field validation so type errors take priority; slot names derived
            // from the synthesized placeholder metadata must never reach a build.
            if !fields.named.is_empty() && !metadata.has_miden_project_toml {
                return Err(syn::Error::new(
                    struct_name.span(),
                    "`#[component_storage]` with `#[storage]` fields requires a \
                     `miden-project.toml` next to the crate's `Cargo.toml`: storage slot names \
                     derive from the `[lib].namespace` interface segment",
                ));
            }
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
                "`#[component_storage]` only supports unit structs or structs with named fields.",
            ));
        }
    };

    let component_metadata = acc_builder.build(call_site_span.into())?;

    let mut metadata_bytes = component_metadata.to_bytes();
    let padded_len = metadata_bytes.len().div_ceil(16) * 16;
    metadata_bytes.resize(padded_len, 0);

    let link_section = generate_link_section(&metadata_bytes);
    let runtime_boilerplate = runtime_boilerplate();

    // Hidden handshake constant consumed by the `#[component]` impl expansion (see
    // `render_storage_marker_check`).
    let marker_ident = format_ident!("{}", COMPONENT_STORAGE_MARKER_CONST);

    Ok(quote! {
        #runtime_boilerplate
        #input_struct
        #default_impl
        impl #struct_name {
            #[doc(hidden)]
            pub const #marker_ident: () = ();
        }
        impl ::miden::native_account::NativeAccount for #struct_name {}
        impl ::miden::active_account::ActiveAccount for #struct_name {}
        #link_section
    })
}

/// Expands the `#[component]` attribute applied to a component trait declaration.
///
/// The trait declares the component's API: its name yields the WIT interface name and its methods
/// yield the exported functions. This expansion validates the declaration and emits only
/// API-derived metadata (the `#[auth_script]` frontend link section) — the WIT interface and
/// guest bindings are generated by the `impl Trait for Storage` expansion, which re-derives
/// everything it needs from the implementation block (whose signatures rustc checks against this
/// trait), so the two expansions need no shared state.
fn expand_component_trait(
    call_site_span: Span,
    mut input_trait: ItemTrait,
) -> Result<TokenStream2, syn::Error> {
    let trait_ident = input_trait.ident.clone();

    reject_generics(&input_trait.generics, "component traits cannot be generic")?;
    if !input_trait.supertraits.is_empty() {
        return Err(syn::Error::new(
            input_trait.supertraits.span(),
            "component traits cannot declare supertraits",
        ));
    }

    let metadata = crate::wit_world::ManifestPackage::load_or_default(call_site_span.into())?;
    // Without a project manifest the synthesized metadata would fail the namespace validation
    // below with a baffling message about an interface named `empty`; name the real problem.
    if !metadata.has_miden_project_toml {
        return Err(syn::Error::new(
            trait_ident.span(),
            "`#[component]` requires a `miden-project.toml` next to the crate's `Cargo.toml`, \
             with `kind = \"account-component\"` and a `[lib].namespace` declaring the \
             component's interface",
        ));
    }
    let package_name = format!("miden:{}", metadata.package.name().into_inner().to_kebab_case());
    // The WIT interface name is derived from the component trait name. It must match the interface
    // segment of `[lib].namespace` in `miden-project.toml`, which is the library identity the
    // project assembler uses to resolve component-level procedures (e.g. `init`) during linking.
    let interface_name = trait_ident.to_string().to_kebab_case();
    validate_namespace_matches_interface(&metadata, &package_name, &interface_name, &trait_ident)?;

    let mut auth_method_idents = Vec::new();
    let mut method_count = 0usize;

    for item in &mut input_trait.items {
        let TraitItem::Fn(method) = item else {
            return Err(syn::Error::new(
                item.span(),
                "component traits only support method declarations",
            ));
        };
        if method.default.is_some() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "component trait methods cannot have default bodies; exports are derived from the \
                 `impl` block, so a defaulted method that is not overridden there would silently \
                 disappear from the component's interface",
            ));
        }

        let is_auth_script = has_auth_script_marker_attr(&method.attrs);
        // Strip the marker so the re-emitted trait does not carry the helper attribute.
        method.attrs.retain(|attr| !is_auth_script_marker_attr(attr));

        // Structural validation only: custom types may not be registered yet when the trait
        // expands, so type mapping is deferred to the implementation expansion.
        let (_, args) = validate_signature_shape(&method.sig)?;
        if is_auth_script {
            validate_auth_script_signature(&method.sig, &args)?;
            auth_method_idents.push(method.sig.ident.clone());
        }
        method_count += 1;
    }

    if method_count == 0 {
        return Err(syn::Error::new(
            input_trait.span(),
            "Component `trait` is missing methods. A component cannot have empty exports.",
        ));
    }

    validate_auth_script_count(
        metadata.target.ty,
        metadata.requires_auth_script(),
        auth_method_idents.len(),
        input_trait.span(),
    )?;

    // `#[auth_script]` lives on the trait method because auth-ness is part of the component's
    // contract, not its behavior — the API reader should see which method is the auth entrypoint.
    // That placement forces the metadata to be emitted here: the impl expansion cannot know which
    // method is the auth entrypoint without trait→impl state, which this design deliberately has
    // none of. This is the one API-derived artifact the trait expansion emits; everything derived
    // from the implementation (WIT, bindings, exports) is generated at the impl expansion.
    let frontend_link_section = auth_method_idents.first().map_or_else(
        || quote! {},
        |auth_ident| {
            let metadata = auth_script_frontend_metadata(&trait_ident, auth_ident);
            generate_frontend_link_section(&metadata)
        },
    );

    // Inject the hidden handshake constant consumed by the implementation expansion (see
    // `render_trait_marker_check`).
    let marker_ident = format_ident!("{}", COMPONENT_TRAIT_MARKER_CONST);
    input_trait.items.push(syn::parse_quote! {
        #[doc(hidden)]
        const #marker_ident: () = ();
    });

    Ok(quote! {
        #input_trait
        #frontend_link_section
    })
}

/// Expands the `#[component]` attribute applied to an `impl Trait for Storage` block.
///
/// This is the component's single generative site: it derives the WIT interface from the
/// implementation's method signatures (which rustc checks against the component trait), invokes
/// `miden::generate!`, wires the generated guest bindings to the user's implementation, and
/// exports the component.
fn expand_component_trait_impl(
    call_site_span: Span,
    mut impl_block: ItemImpl,
) -> Result<TokenStream2, syn::Error> {
    let Some((_, trait_path, _)) = impl_block.trait_.clone() else {
        return Err(syn::Error::new(
            impl_block.span(),
            "`#[component]` requires a trait implementation. Write `impl MyComponent for \
             MyComponentStorage` and annotate the storage struct with `#[component_storage]`.",
        ));
    };

    reject_generics(&impl_block.generics, "component trait implementations cannot be generic")?;

    let component_type = (*impl_block.self_ty).clone();
    if extract_type_ident(&component_type).is_none() {
        return Err(syn::Error::new(
            impl_block.self_ty.span(),
            "Failed to determine the storage type targeted by this implementation.",
        ));
    }

    let trait_segment = trait_path.segments.last().ok_or_else(|| {
        syn::Error::new(trait_path.span(), "Failed to determine the component trait name.")
    })?;
    if !matches!(trait_segment.arguments, PathArguments::None) {
        return Err(syn::Error::new(
            trait_segment.arguments.span(),
            "component trait paths cannot use generic arguments",
        ));
    }
    let trait_ident = trait_segment.ident.clone();

    let metadata = crate::wit_world::ManifestPackage::load_or_default(call_site_span.into())?;
    // Without a project manifest the namespace validation below would run against synthesized
    // placeholder metadata and recommend `miden:empty/...@0.0.0` as the fix; name the real
    // problem instead, mirroring the trait-side guard.
    if !metadata.has_miden_project_toml {
        return Err(syn::Error::new(
            trait_ident.span(),
            "`#[component]` requires a `miden-project.toml` next to the crate's `Cargo.toml`, \
             with `kind = \"account-component\"` and a `[lib].namespace` declaring the \
             component's interface",
        ));
    }
    let package_name = format!("miden:{}", metadata.package.name().into_inner().to_kebab_case());
    // The generated WIT interface is named after the trait *as spelled here*. The trait-side
    // validation only covers the declared trait name, so an impl that spells the trait through an
    // alias (`use api::Foo as Bar; impl Bar for ...`) would silently generate an interface named
    // after the alias — validate the impl-side spelling against `[lib].namespace` as well.
    let interface_name = trait_ident.to_string().to_kebab_case();
    validate_namespace_matches_interface(&metadata, &package_name, &interface_name, &trait_ident)?;
    let interface_module = interface_name.to_snake_case();
    let world_name = format!("{interface_name}-world");

    let mut exported_types = registered_export_types();
    exported_types.sort_by(|a, b| a.wit_name.cmp(&b.wit_name));
    let exported_types_by_rust: HashMap<_, _> =
        exported_types.iter().map(|def| (def.rust_name.clone(), def.clone())).collect();

    let mut methods = Vec::new();
    let mut type_imports = BTreeSet::new();
    for item in &mut impl_block.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        // This outer `#[component]` expansion sees the raw `#[auth_script]` tokens before the
        // standalone attribute macro would run, so stripping the marker here would silently
        // discard a misplaced annotation; reject it with the same guidance instead.
        if has_auth_script_marker_attr(&method.attrs) {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "`#[auth_script]` must be applied to a method inside a `#[component]` `trait`, \
                 not the implementation block",
            ));
        }
        let (parsed_method, imports) =
            parse_component_signature(&method.sig, &method.attrs, &exported_types_by_rust)?;
        type_imports.extend(imports);
        methods.push(parsed_method);
    }

    if methods.is_empty() {
        return Err(syn::Error::new(
            impl_block.span(),
            "Component `impl` is missing methods. A component cannot have empty exports.",
        ));
    }

    let dependency_imports = metadata.collect_miden_dependency_imports(Span2::call_site())?;
    let inline_wit_source = build_component_wit(ComponentWitSpec {
        component_package: &package_name,
        component_version: metadata.package.version().inner(),
        interface_name: &interface_name,
        world_name: &world_name,
        dependency_imports: &dependency_imports,
        type_imports: &type_imports,
        methods: &methods,
        exported_types: &exported_types,
    })?;
    // Dependency imports are only needed while generating this crate's bindings. The public WIT
    // file stays export-only so downstream crates can depend on this account without also
    // materializing all of its transitive FPI dependencies next to the generated WIT.
    let public_wit_source = build_component_wit(ComponentWitSpec {
        component_package: &package_name,
        component_version: metadata.package.version().inner(),
        interface_name: &interface_name,
        world_name: &world_name,
        dependency_imports: &[],
        type_imports: &type_imports,
        methods: &methods,
        exported_types: &exported_types,
    })?;
    write_component_wit_file(call_site_span, &public_wit_source, &package_name)?;
    let inline_literal = Literal::string(&inline_wit_source);

    let interface_path =
        format!("{}/{}@{}", package_name, interface_name, metadata.package.version());
    // Custom types are resolved relative to the crate root using the paths written in the
    // implementation's method signatures.
    let custom_type_paths = collect_custom_type_paths(&exported_types, &methods, None);

    let (custom_with_entries, debug_with_entries) =
        build_custom_with_entries(&exported_types, &interface_path, None, &custom_type_paths);

    if env::var_os("MIDEN_COMPONENT_DEBUG_WITH").is_some() {
        eprintln!(
            "[miden::component] with mappings for {package_name}: {}",
            debug_with_entries.join(", ")
        );
    }

    let guest_trait_path = build_guest_trait_path(&package_name, &interface_module)?;
    let guest_methods: Vec<TokenStream2> = methods
        .iter()
        .map(|method| render_guest_method(method, &component_type, &trait_path))
        .collect();

    let marker_check = render_trait_marker_check(&component_type, &trait_path);
    let storage_marker_check = render_storage_marker_check(&component_type);

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
        #marker_check
        #storage_marker_check
        // Use the fully-qualified component type here so the export macro works even when
        // the impl block was declared through a module-qualified path (e.g. `impl Foo for super::Bar`).
        self::bindings::export!(#component_type);
    })
}

/// Emits a compile-time check that the implemented trait carries the `#[component]` attribute.
///
/// The trait expansion injects a hidden associated constant; referencing it here turns a forgotten
/// `#[component]` on the trait into a missing-item error naming the constant, instead of silently
/// skipping the trait-side validation (default-body, namespace, and `#[auth_script]` checks).
fn render_trait_marker_check(component_type: &Type, trait_path: &syn::Path) -> TokenStream2 {
    let marker_ident = format_ident!("{}", COMPONENT_TRAIT_MARKER_CONST);
    quote! {
        const _: () = <#component_type as #trait_path>::#marker_ident;
    }
}

/// Emits a compile-time check that the storage type carries the `#[component_storage]` attribute.
///
/// The storage expansion injects a hidden inherent constant; referencing it here turns a forgotten
/// `#[component_storage]` on the storage struct into a missing-item error naming the constant,
/// instead of silently building a component without storage metadata, account trait impls, or
/// runtime boilerplate.
fn render_storage_marker_check(component_type: &Type) -> TokenStream2 {
    let marker_ident = format_ident!("{}", COMPONENT_STORAGE_MARKER_CONST);
    quote! {
        const _: () = <#component_type>::#marker_ident;
    }
}

/// Validates that the component's WIT interface name (derived from the trait) matches the interface
/// segment of `[lib].namespace` in `miden-project.toml`.
///
/// The project assembler ties the component's library identity to `[lib].namespace`, overriding the
/// component root module's path with it during assembly. A mismatch otherwise surfaces only as a
/// cryptic linker error about an undefined component `init` procedure, so we catch it here with an
/// actionable message.
fn validate_namespace_matches_interface(
    metadata: &crate::wit_world::ManifestPackage,
    package_name: &str,
    interface_name: &str,
    trait_ident: &syn::Ident,
) -> Result<(), syn::Error> {
    let namespace = declared_namespace(metadata);
    // Require full equality, not just the interface segment: the generated WIT and `with`
    // mappings use the manifest's package name and version, so a namespace with a divergent
    // package or version would let the declared library identity drift from the generated WIT
    // paths even though the interface segment matches.
    let version = metadata.package.version();
    let expected_namespace = format!("{package_name}/{interface_name}@{version}");

    if namespace != expected_namespace {
        return Err(syn::Error::new(
            trait_ident.span(),
            format!(
                "component trait `{trait_ident}` produces WIT interface `{interface_name}` in \
                 package `{package_name}` version `{version}`, but `[lib].namespace` in \
                 `miden-project.toml` declares `{namespace}`. Update `[lib].namespace` to \
                 `{expected_namespace}`."
            ),
        ));
    }

    Ok(())
}

/// Returns the component id declared in `[lib].namespace` without the assembler path decoration
/// (the leading `::` root marker and the component quoting).
fn declared_namespace(metadata: &crate::wit_world::ManifestPackage) -> &str {
    metadata
        .target
        .namespace
        .inner()
        .as_str()
        .trim_start_matches("::")
        .trim_matches('"')
}

/// Rejects any generic parameters or `where` clause on a component item.
///
/// Shared by the trait, trait-impl, and storage expansions, which all generate code that cannot
/// be generic.
fn reject_generics(generics: &syn::Generics, message: &str) -> Result<(), syn::Error> {
    if generics.lt_token.is_some() || !generics.params.is_empty() || generics.where_clause.is_some()
    {
        return Err(syn::Error::new(generics.span(), message));
    }

    Ok(())
}

/// Extracts the interface segment of the fully-qualified component id declared in
/// `[lib].namespace` (`namespace:package/interface@version`); the interface segment sits between
/// the last `/` and the `@`.
fn namespace_interface_segment(metadata: &crate::wit_world::ManifestPackage) -> &str {
    declared_namespace(metadata)
        .rsplit('/')
        .next()
        .and_then(|segment| segment.split('@').next())
        .unwrap_or_default()
}

/// Validates how many methods may be annotated with `#[auth_script]` for the current project kind.
fn validate_auth_script_count(
    target_type: TargetType,
    requires_auth_script: bool,
    auth_method_count: usize,
    span: Span2,
) -> Result<(), syn::Error> {
    match (target_type, requires_auth_script, auth_method_count) {
        (TargetType::AccountComponent, true, 1) => Ok(()),
        (TargetType::AccountComponent, true, 0) => Err(syn::Error::new(
            span,
            "authentication components require exactly one `#[auth_script]` method",
        )),
        (TargetType::AccountComponent, _, count) if count > 1 => Err(syn::Error::new(
            span,
            "only one `#[auth_script]` method is allowed per `#[component]` trait",
        )),
        (TargetType::AccountComponent, ..) => Ok(()),
        (_, _, count) if count > 0 => Err(syn::Error::new(
            span,
            "`#[auth_script]` method is only permitted on components of 'account-component' type",
        )),
        _ => Ok(()),
    }
}

/// Synthesizes the guest trait path exposed by `wit-bindgen` for the generated interface.
fn build_guest_trait_path(
    package_name: &str,
    interface_module: &str,
) -> Result<TokenStream2, syn::Error> {
    let package_without_version = package_name.split('@').next().unwrap_or(package_name).trim();

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
///
/// The user's method is invoked through fully-qualified trait syntax (`<Storage as Trait>::method`)
/// so the forwarding does not depend on the component trait being in scope at the generated guest
/// implementation.
fn render_guest_method(
    method: &ComponentMethod,
    component_type: &Type,
    trait_path: &syn::Path,
) -> TokenStream2 {
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
        ReceiverKind::Ref | ReceiverKind::Value => {
            quote! { let #component_ident = #component_type::default(); }
        }
        ReceiverKind::RefMut => quote! { let mut #component_ident = #component_type::default(); },
    };

    let receiver_arg = match method.receiver_kind {
        ReceiverKind::Ref => quote!(&#component_ident),
        ReceiverKind::RefMut => quote!(&mut #component_ident),
        ReceiverKind::Value => quote!(#component_ident),
    };

    let call_expr = quote! {
        <#component_type as #trait_path>::#fn_ident(#receiver_arg #(, #call_args)*)
    };

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

fn record_type_path(
    paths: &mut HashMap<String, Vec<String>>,
    type_ref: &TypeRef,
    module_prefix_segments: Option<&[String]>,
) {
    for dependency in &type_ref.dependencies {
        record_type_path(paths, dependency, module_prefix_segments);
    }

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

/// Validates the structural requirements shared by component trait declarations and trait
/// implementations, returning the receiver kind and the typed `(identifier, type)` arguments.
///
/// This pass is registry-free on purpose: the trait may expand before the crate's
/// `#[export_type]` types are registered, so custom-type mapping is deferred to
/// [`parse_component_signature`], which only runs for the implementation block.
fn validate_signature_shape(
    sig: &syn::Signature,
) -> Result<(ReceiverKind, Vec<(syn::Ident, syn::Type)>), syn::Error> {
    if sig.constness.is_some() {
        return Err(syn::Error::new(sig.ident.span(), "component methods cannot be `const`"));
    }
    if sig.asyncness.is_some() {
        return Err(syn::Error::new(sig.ident.span(), "component methods cannot be `async`"));
    }
    if sig.unsafety.is_some() {
        return Err(syn::Error::new(sig.ident.span(), "component methods cannot be `unsafe`"));
    }
    if sig.abi.is_some() {
        return Err(syn::Error::new(
            sig.ident.span(),
            "component methods cannot specify an `extern` ABI",
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new(sig.generics.span(), "component methods cannot be generic"));
    }
    if sig.variadic.is_some() {
        return Err(syn::Error::new(
            sig.ident.span(),
            "variadic component methods are unsupported",
        ));
    }

    let mut inputs_iter = sig.inputs.iter();
    let receiver = inputs_iter.next().ok_or_else(|| {
        syn::Error::new(
            sig.span(),
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

    let mut args = Vec::new();
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
                args.push((ident, (*pat_type.ty).clone()));
            }
            FnArg::Receiver(other) => {
                return Err(syn::Error::new(
                    other.span(),
                    "component methods support a single receiver argument",
                ));
            }
        }
    }

    Ok((receiver_kind, args))
}

/// Parses an implementation method and extracts the metadata necessary to export it via WIT.
fn parse_component_signature(
    sig: &syn::Signature,
    attrs: &[Attribute],
    exported_types: &HashMap<String, ExportedTypeDef>,
) -> Result<(ComponentMethod, BTreeSet<String>), syn::Error> {
    let (receiver_kind, args) = validate_signature_shape(sig)?;

    let mut params = Vec::new();
    let mut type_imports = BTreeSet::new();

    for (ident, user_ty) in args {
        let type_ref = map_type_to_type_ref(&user_ty, exported_types)?;
        type_ref.add_required_core_type_imports(&mut type_imports);

        params.push(MethodParam {
            wit_param_name: to_kebab_case(&ident.to_string()),
            ident,
            user_ty,
            type_ref,
        });
    }

    let return_info = match &sig.output {
        ReturnType::Default => MethodReturn::Unit,
        ReturnType::Type(_, ty) if is_unit_type(ty) => MethodReturn::Unit,
        ReturnType::Type(_, ty) => {
            let type_ref = map_type_to_type_ref(ty, exported_types)?;
            type_ref.add_required_core_type_imports(&mut type_imports);
            MethodReturn::Type {
                user_ty: ty.clone(),
                type_ref,
            }
        }
    };

    let doc_attrs = attrs.iter().filter(|attr| attr.path().is_ident("doc")).cloned().collect();

    let component_method = ComponentMethod {
        fn_ident: sig.ident.clone(),
        doc_attrs,
        params,
        receiver_kind,
        return_info,
        wit_name: to_kebab_case(&sig.ident.to_string()),
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
    sig: &syn::Signature,
    args: &[(syn::Ident, syn::Type)],
) -> Result<(), syn::Error> {
    if args.len() != 1 || !is_type_named(&args[0].1, "Word") {
        return Err(syn::Error::new(
            sig.span(),
            "`#[auth_script]` methods must accept exactly one `Word` argument (excluding `self`)",
        ));
    }

    let returns_unit = match &sig.output {
        ReturnType::Default => true,
        ReturnType::Type(_, ty) => is_unit_type(ty),
    };
    if !returns_unit {
        return Err(syn::Error::new(
            sig.output.span(),
            "`#[auth_script]` methods must return `()`",
        ));
    }

    Ok(())
}

/// Builds frontend metadata for the single `#[auth_script]` method exported by a component.
///
/// `method_path` is diagnostic-only (used in error messages), so the trait-qualified path is
/// sufficient; `export_name` is the WIT export name matched against the lifted component export.
fn auth_script_frontend_metadata(
    trait_ident: &syn::Ident,
    auth_method_ident: &syn::Ident,
) -> FrontendMetadata {
    FrontendMetadata::AuthScript {
        method_path: format!("{trait_ident}::{auth_method_ident}"),
        export_name: to_kebab_case(&auth_method_ident.to_string()),
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
    is_attr_named(attr, AUTH_SCRIPT_ATTR)
        || is_attr_named(attr, AUTH_SCRIPT_MARKER_ATTR)
        // Accept the previous doc marker while older generated test inputs are still around.
        || is_doc_marker_attr(attr, "__miden_auth_script_marker")
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
            dependencies: Vec::new(),
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
            dependencies: Vec::new(),
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
            dependencies: Vec::new(),
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
        let method: TraitItemFn = parse_quote! {
            fn whatever_name(&mut self, arg: Word);
        };

        let (_, args) = validate_signature_shape(&method.sig).unwrap();
        validate_auth_script_signature(&method.sig, &args).unwrap();
        let trait_ident = format_ident!("AuthComponent");
        let metadata = auth_script_frontend_metadata(&trait_ident, &method.sig.ident);

        assert!(matches!(
            metadata,
            FrontendMetadata::AuthScript { export_name, .. } if export_name == "whatever-name"
        ));
    }

    #[test]
    fn auth_script_methods_require_word_argument() {
        let method: TraitItemFn = parse_quote! {
            fn auth_procedure(&mut self, arg: u32);
        };

        let (_, args) = validate_signature_shape(&method.sig).unwrap();
        let err = match validate_auth_script_signature(&method.sig, &args) {
            Ok(_) => panic!("expected `#[auth_script]` validation to reject non-`Word` arguments"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("exactly one `Word` argument"));
    }

    #[test]
    fn auth_script_methods_require_unit_return() {
        let method: TraitItemFn = parse_quote! {
            fn auth_procedure(&mut self, arg: Word) -> Word;
        };

        let (_, args) = validate_signature_shape(&method.sig).unwrap();
        let err = match validate_auth_script_signature(&method.sig, &args) {
            Ok(_) => panic!("expected `#[auth_script]` validation to reject non-unit returns"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must return `()`"));
    }

    #[test]
    fn auth_script_frontend_metadata_emits_project_wide_uniqueness_guard() {
        let trait_ident = format_ident!("AuthComponent");
        let method_ident = format_ident!("whatever_name");
        let metadata = auth_script_frontend_metadata(&trait_ident, &method_ident);
        let tokens = generate_frontend_link_section(&metadata).to_string();

        assert!(tokens.contains(crate::util::FRONTEND_METADATA_UNIQUENESS_GUARD_SYMBOL));
    }

    #[test]
    fn auth_script_frontend_metadata_stores_method_path() {
        let trait_ident = format_ident!("AuthComponent");
        let method_ident = format_ident!("whatever_name");
        let metadata = auth_script_frontend_metadata(&trait_ident, &method_ident);

        assert_eq!(
            metadata,
            FrontendMetadata::AuthScript {
                method_path: "AuthComponent::whatever_name".into(),
                export_name: "whatever-name".into(),
            }
        );
    }

    #[test]
    fn authentication_components_require_exactly_one_auth_script() {
        let err =
            validate_auth_script_count(TargetType::AccountComponent, true, 0, Span2::call_site())
                .expect_err("expected authentication components to require an auth script");

        assert!(
            err.to_string()
                .contains("authentication components require exactly one `#[auth_script]` method")
        );

        validate_auth_script_count(TargetType::AccountComponent, true, 1, Span2::call_site())
            .expect("expected exactly one auth script to be accepted");
    }

    #[test]
    fn ordinary_account_components_may_omit_auth_script() {
        validate_auth_script_count(TargetType::AccountComponent, false, 0, Span2::call_site())
            .expect("expected ordinary account components to allow no auth script");
    }

    #[test]
    fn auth_script_marker_accepts_helper_attribute() {
        let method: TraitItemFn = parse_quote! {
            #[miden_auth_script_requires_component]
            fn whatever_name(&mut self, arg: Word);
        };

        assert!(has_auth_script_marker_attr(&method.attrs));
    }
}
