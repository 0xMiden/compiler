use std::{
    collections::{BTreeSet, HashMap},
    env,
};

use miden_project::TargetType;
use miden_protocol::utils::serde::Serializable;
use midenc_frontend_wasm_metadata::{
    COMPONENT_INIT_PROCEDURE, FrontendMetadata, WASM_ACCOUNT_COMPONENT_METADATA_CUSTOM_SECTION_NAME,
};
use proc_macro::Span;
use proc_macro2::{Ident, Literal, Span as Span2, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, ItemStruct, ItemTrait, PathArguments,
    ReturnType, TraitItem, TraitItemFn, Type, ext::IdentExt as _, spanned::Spanned,
};

pub(crate) use crate::component_macro::storage::typecheck_storage_field;
use crate::{
    account_component_metadata::AccountComponentMetadataBuilder,
    boilerplate::runtime_boilerplate,
    component_macro::{
        generate_wit::{ComponentWitSpec, build_component_wit},
        storage::process_storage_fields,
    },
    dependency_ref::{DependencyRef, DependencyRefArgs},
    namespace::ComponentNamespace,
    types::{
        ExportedTypeDef, ExportedTypeKind, TypeRef, map_type_to_type_ref, registered_export_types,
    },
    util::{generate_frontend_link_section, generate_wit_link_section, is_type_named},
    wit_names::{rust_ident_to_wit_name, wit_bindgen_guest_ident},
};

pub(crate) mod generate_wit;
mod sibling;
mod storage;

/// Attribute name used to mark the authentication procedure on a component method.
const AUTH_SCRIPT_ATTR: &str = "auth_script";
/// Helper attribute preserved by `#[auth_script]` so `#[component]` can recognize the method.
const AUTH_SCRIPT_MARKER_ATTR: &str = "miden_auth_script_requires_component";
/// Attribute name used to mark an account-interface procedure on a component method.
const ACCOUNT_PROCEDURE_ATTR: &str = "account_procedure";
/// Helper attribute preserved by `#[account_procedure]` so `#[component]` can recognize the method.
const ACCOUNT_PROCEDURE_MARKER_ATTR: &str = "miden_account_procedure_requires_component";
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
    /// Canonical WIT name of the method.
    wit_name: String,
    /// Identifier of the guest trait method wit-bindgen generates for `wit_name`.
    guest_fn_ident: syn::Ident,
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
    let call_site_span = Span::call_site();
    let attr_tokens: TokenStream2 = attr.into();
    let item_tokens: TokenStream2 = item.into();

    if let Ok(item_trait) = syn::parse2::<ItemTrait>(item_tokens.clone()) {
        // Sibling component dependencies are declared on the trait: the trait is the component's
        // API, and the sibling traits it generates appear in that API as supertraits.
        let sibling_refs = match syn::parse2::<DependencyRefArgs>(attr_tokens) {
            Ok(args) => args.refs,
            Err(err) => return err.to_compile_error().into(),
        };
        match expand_component_trait(call_site_span, item_trait, sibling_refs) {
            Ok(expanded) => expanded.into(),
            Err(err) => err.to_compile_error().into(),
        }
    } else if !attr_tokens.is_empty() {
        syn::Error::new(
            attr_tokens.span(),
            "`#[component]` only accepts arguments on the component trait declaration; declare \
             sibling component dependencies as `#[component(package::Interface, ...)]` on the \
             trait",
        )
        .into_compile_error()
        .into()
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

    // `TraitItemFn` parses any visibility-less method with a body (the body reads as a default),
    // which is exactly what new-style impl blocks contain — so the dedicated `ImplItemFn` branch
    // above is unreachable for them. A body is never valid on an `#[auth_script]` declaration
    // (component traits reject default bodies), so reject it here with the placement guidance.
    if item_fn.default.is_some() {
        return syn::Error::new(
            item_fn.sig.span(),
            "`#[auth_script]` must be applied to a method inside a `#[component]` `trait`, not \
             the implementation block",
        )
        .into_compile_error()
        .into();
    }

    // Preserve a helper attribute for `#[component]` to consume. If the surrounding trait forgets
    // `#[component]`, rustc rejects this unknown helper attribute instead of silently compiling a
    // method that emits no auth metadata.
    let marker_attr = format_ident!("{}", AUTH_SCRIPT_MARKER_ATTR);
    item_fn.attrs.push(syn::parse_quote!(#[#marker_attr]));
    quote!(#item_fn).into()
}

/// Expands `#[account_procedure]`.
///
/// This attribute must be applied to a method inside a `trait` annotated with `#[component]`. It
/// acts as a marker for `#[component]` so the macro can emit frontend metadata marking the
/// annotated export as part of the account interface without rewriting its user-defined name.
pub fn expand_account_procedure(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            Span2::call_site(),
            "#[account_procedure] does not accept arguments",
        )
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
                    "`#[account_procedure]` must be applied to a method inside a `#[component]` \
                     `trait`, not the implementation block",
                )
                .into_compile_error()
                .into();
            }

            if let Ok(item_fn) = syn::parse2::<syn::ItemFn>(item_tokens.clone()) {
                return syn::Error::new(
                    item_fn.sig.span(),
                    "`#[account_procedure]` must be applied to a method inside a `#[component]` \
                     `trait`",
                )
                .into_compile_error()
                .into();
            }

            return syn::Error::new(
                Span2::call_site(),
                "`#[account_procedure]` must be applied to a method inside a `#[component]` \
                 `trait`",
            )
            .into_compile_error()
            .into();
        }
    };

    // `TraitItemFn` parses any visibility-less method with a body (the body reads as a default),
    // which is exactly what new-style impl blocks contain — so the dedicated `ImplItemFn` branch
    // above is unreachable for them. A body is never valid on an `#[account_procedure]` declaration
    // (component traits reject default bodies), so reject it here with the placement guidance.
    if item_fn.default.is_some() {
        return syn::Error::new(
            item_fn.sig.span(),
            "`#[account_procedure]` must be applied to a method inside a `#[component]` `trait`, \
             not the implementation block",
        )
        .into_compile_error()
        .into();
    }

    // Preserve a helper attribute for `#[component]` to consume. If the surrounding trait forgets
    // `#[component]`, rustc rejects this unknown helper attribute instead of silently compiling a
    // method that emits no account-procedure metadata.
    let marker_attr = format_ident!("{}", ACCOUNT_PROCEDURE_MARKER_ATTR);
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
            // Slot names derive from the component's public identity (the `[lib].namespace`)
            // rather than the storage struct name, so renaming the private struct cannot change
            // deployed storage slot names.
            let namespace = if metadata.has_miden_project_toml {
                Some(metadata.namespace(struct_name.span())?)
            } else {
                None
            };
            let field_inits = process_storage_fields(fields, &mut acc_builder, namespace.as_ref())?;
            // Checked after field validation so type errors take priority.
            if !fields.named.is_empty() && namespace.is_none() {
                return Err(syn::Error::new(
                    struct_name.span(),
                    "`#[component_storage]` with `#[storage]` fields requires a \
                     `miden-project.toml` next to the crate's `Cargo.toml`: storage slot names \
                     derive from its `[lib].namespace`",
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
/// The trait declares the component's API: its methods yield the exported functions, named
/// `<[lib].namespace>::<method>`; the WIT package and interface derive from `[lib].namespace`.
/// This expansion validates the declaration and emits only API-derived metadata (the
/// `#[auth_script]` frontend link section) — the WIT interface and
/// guest bindings are generated by the `impl Trait for Storage` expansion, which re-derives
/// everything it needs from the implementation block (whose signatures rustc checks against this
/// trait), so the two expansions need no shared state.
///
/// Sibling component dependencies named in the attribute (`#[component(pkg::Interface, ...)]`)
/// additionally expand here into one generated Rust trait per reference whose default methods
/// perform intra-account cross-context calls into the sibling component (see [`sibling`]).
/// Supertraits are permitted (and not validated) so the component trait can declare the generated
/// sibling traits and account traits it relies on; rustc enforces the corresponding impls.
fn expand_component_trait(
    call_site_span: Span,
    mut input_trait: ItemTrait,
    sibling_refs: Vec<DependencyRef>,
) -> Result<TokenStream2, syn::Error> {
    let trait_ident = input_trait.ident.clone();

    reject_generics(&input_trait.generics, "component traits cannot be generic")?;

    let metadata = crate::wit_world::ManifestPackage::load_or_default(call_site_span.into())?;
    // Without a project manifest the synthesized metadata would fail the namespace validation
    // below with a baffling message about a namespace named `empty`; name the real problem.
    if !metadata.has_miden_project_toml {
        return Err(syn::Error::new(
            trait_ident.span(),
            "`#[component]` requires a `miden-project.toml` next to the crate's `Cargo.toml`, \
             with `kind = \"account-component\"` and a `[lib].namespace` declaring the \
             component's Miden path",
        ));
    }
    // `[lib].namespace` names every exported procedure (`<namespace>::<method>`).
    let namespace = metadata.namespace(trait_ident.span())?;

    let mut auth_method_idents = Vec::new();
    let mut account_procedure_idents = Vec::new();
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
        let is_account_procedure = has_account_procedure_marker_attr(&method.attrs);
        // Strip the markers so the re-emitted trait does not carry the helper attributes.
        method.attrs.retain(|attr| {
            !is_auth_script_marker_attr(attr) && !is_account_procedure_marker_attr(attr)
        });

        // Structural validation only: custom types may not be registered yet when the trait
        // expands, so type mapping is deferred to the implementation expansion.
        let (_, args) = validate_signature_shape(&method.sig)?;
        if is_auth_script {
            validate_auth_script_signature(&method.sig, &args)?;
            auth_method_idents.push(method.sig.ident.clone());
        }
        if is_account_procedure {
            account_procedure_idents.push(method.sig.ident.clone());
        }
        method_count += 1;
    }

    if method_count == 0 {
        return Err(syn::Error::new(
            input_trait.span(),
            "Component `trait` is missing methods. A component cannot have empty exports.",
        ));
    }

    // `#[auth_script]` and `#[account_procedure]` belong to different project kinds and cannot be
    // combined in one component: an authentication component's `#[auth_script]` method is its
    // account interface implicitly, whereas a regular account component marks its interface
    // procedures with `#[account_procedure]`.
    if !auth_method_idents.is_empty() && !account_procedure_idents.is_empty() {
        return Err(syn::Error::new(
            trait_ident.span(),
            "a component cannot combine `#[auth_script]` and `#[account_procedure]`: \
             `#[auth_script]` is the interface of an authentication component (its auth method is \
             the account interface implicitly), while `#[account_procedure]` marks the interface \
             procedures of a regular account component",
        ));
    }

    validate_auth_script_count(
        metadata.target.ty,
        metadata.requires_auth_script(),
        auth_method_idents.len(),
        input_trait.span(),
    )?;

    // `#[auth_script]` and `#[account_procedure]` live on the trait methods because account-
    // interface membership is part of the component's contract, not its behavior — the API reader
    // should see which methods are account procedures. That placement forces the metadata to be
    // emitted here: the impl expansion cannot know which methods are marked without trait→impl
    // state, which this design deliberately has none of. This is the one API-derived artifact the
    // trait expansion emits; everything derived from the implementation (WIT, bindings, exports) is
    // generated at the impl expansion.
    let mut frontend_metadata_entries = Vec::new();
    if let Some(auth_ident) = auth_method_idents.first() {
        frontend_metadata_entries.push(auth_script_frontend_metadata(
            &namespace,
            &trait_ident,
            auth_ident,
        )?);
    }
    for account_ident in &account_procedure_idents {
        frontend_metadata_entries.push(account_procedure_frontend_metadata(
            &namespace,
            &trait_ident,
            account_ident,
        )?);
    }
    let frontend_link_section = if frontend_metadata_entries.is_empty() {
        quote! {}
    } else {
        generate_frontend_link_section(&frontend_metadata_entries)
    };

    // Inject the hidden handshake constant consumed by the implementation expansion (see
    // `render_trait_marker_check`).
    let marker_ident = format_ident!("{}", COMPONENT_TRAIT_MARKER_CONST);
    input_trait.items.push(syn::parse_quote! {
        #[doc(hidden)]
        const #marker_ident: () = ();
    });

    let sibling_traits = if sibling_refs.is_empty() {
        quote! {}
    } else {
        sibling::expand_sibling_traits(&metadata, &trait_ident, &sibling_refs)?
    };

    Ok(quote! {
        #input_trait
        #frontend_link_section
        #sibling_traits
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
    if !is_path_type(&component_type) {
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
    // placeholder metadata; name the real problem instead, mirroring the trait-side guard.
    if !metadata.has_miden_project_toml {
        return Err(syn::Error::new(
            trait_ident.span(),
            "`#[component]` requires a `miden-project.toml` next to the crate's `Cargo.toml`, \
             with `kind = \"account-component\"` and a `[lib].namespace` declaring the \
             component's Miden path",
        ));
    }
    // The WIT package, interface and every export path derive from `[lib].namespace`; the trait
    // name takes no part in naming.
    let namespace = metadata.namespace(trait_ident.span())?;

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
        // This outer `#[component]` expansion sees the raw `#[auth_script]` / `#[account_procedure]`
        // tokens before the standalone attribute macros would run, so stripping the markers here
        // would silently discard a misplaced annotation; reject it with the same guidance instead.
        if has_auth_script_marker_attr(&method.attrs) {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "`#[auth_script]` must be applied to a method inside a `#[component]` `trait`, \
                 not the implementation block",
            ));
        }
        if has_account_procedure_marker_attr(&method.attrs) {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "`#[account_procedure]` must be applied to a method inside a `#[component]` \
                 `trait`, not the implementation block",
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
    reject_duplicate_method_wit_names(&methods)?;

    let dependency_imports = metadata.collect_miden_dependency_imports(Span2::call_site())?;
    let inline_wit_source = build_component_wit(ComponentWitSpec {
        namespace: &namespace,
        component_version: metadata.package.version().inner(),
        dependency_imports: &dependency_imports,
        type_imports: &type_imports,
        methods: &methods,
        exported_types: &exported_types,
    })?;
    // Dependency imports are only needed while generating this crate's bindings. The public WIT
    // stays export-only so downstream crates can depend on this account without also
    // materializing all of its transitive FPI dependencies.
    let public_wit_source = build_component_wit(ComponentWitSpec {
        namespace: &namespace,
        component_version: metadata.package.version().inner(),
        dependency_imports: &[],
        type_imports: &type_imports,
        methods: &methods,
        exported_types: &exported_types,
    })?;
    // The public WIT is embedded into a Wasm custom section, carried by the compiler into the
    // Miden package (`.masp`), where dependent crates' macros read it back during expansion.
    let wit_link_section = generate_wit_link_section(&public_wit_source)?;
    let inline_literal = Literal::string(&inline_wit_source);

    let interface_path = namespace.wit_id(metadata.package.version());
    // Custom types are resolved relative to the crate root using the paths written in the
    // implementation's method signatures.
    let custom_type_paths = collect_custom_type_paths(&exported_types, &methods, None);

    let (custom_with_entries, debug_with_entries) =
        build_custom_with_entries(&exported_types, &interface_path, None, &custom_type_paths);

    if env::var_os("MIDEN_COMPONENT_DEBUG_WITH").is_some() {
        eprintln!(
            "[miden::component] with mappings for {interface_path}: {}",
            debug_with_entries.join(", ")
        );
    }

    let guest_trait_path = namespace.guest_trait_path();
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
        // wit-bindgen's `export!` accepts only an identifier, while the impl's self type may be a
        // qualified path (e.g. `impl Foo for super::Bar`). A local alias hands the macro an
        // identifier that resolves to the full type; the anonymous const keeps the alias private,
        // and the generated `export_name` items keep their global linkage inside the block.
        const _: () = {
            type __MidenComponentExport = #component_type;
            self::bindings::export!(__MidenComponentExport);
        };
        #wit_link_section
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

    // wit-bindgen names the guest trait method after the WIT name, which may differ from the
    // user's identifier (`getURL` -> `get_url`, `r#type` -> `type_`).
    let guest_fn_ident = &method.guest_fn_ident;
    quote! {
        #(#doc_attrs)*
        fn #guest_fn_ident(#fn_inputs) #output {
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

    let mut params: Vec<MethodParam> = Vec::new();
    let mut type_imports = BTreeSet::new();

    for (ident, user_ty) in args {
        let type_ref = map_type_to_type_ref(&user_ty, exported_types)?;
        type_ref.add_required_core_type_imports(&mut type_imports);

        // Distinct Rust identifiers can normalize to one WIT name; catch that here instead of
        // surfacing a WIT parse error from the generated bindings.
        let wit_param_name = rust_ident_to_wit_name(&ident)?;
        if let Some(previous) = params.iter().find(|param| param.wit_param_name == wit_param_name) {
            return Err(duplicate_wit_name_error(
                "parameter",
                &ident,
                &previous.ident,
                &wit_param_name,
            ));
        }

        params.push(MethodParam {
            wit_param_name,
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

    let wit_name = rust_ident_to_wit_name(&sig.ident)?;
    let component_method = ComponentMethod {
        fn_ident: sig.ident.clone(),
        doc_attrs,
        params,
        receiver_kind,
        return_info,
        guest_fn_ident: wit_bindgen_guest_ident(&wit_name, sig.ident.span()),
        wit_name,
    };

    Ok((component_method, type_imports))
}

/// Rejects component methods whose Rust identifiers normalize to one WIT name.
fn reject_duplicate_method_wit_names(methods: &[ComponentMethod]) -> Result<(), syn::Error> {
    for (index, method) in methods.iter().enumerate() {
        if let Some(previous) =
            methods[..index].iter().find(|previous| previous.wit_name == method.wit_name)
        {
            return Err(duplicate_wit_name_error(
                "method",
                &method.fn_ident,
                &previous.fn_ident,
                &method.wit_name,
            ));
        }
    }
    Ok(())
}

/// Rejects component methods whose WIT name is also the name of a type of the component's WIT
/// interface, `type_names`: an imported core type or an exported custom type. WIT interfaces share
/// one namespace between types and functions.
fn reject_method_type_name_collisions<'a>(
    methods: &[ComponentMethod],
    type_names: impl IntoIterator<Item = &'a String>,
) -> Result<(), syn::Error> {
    let type_names = type_names.into_iter().collect::<BTreeSet<_>>();
    match methods.iter().find(|method| type_names.contains(&method.wit_name)) {
        Some(method) => Err(syn::Error::new(
            method.fn_ident.span(),
            format!(
                "component method `{}` produces the WIT name `{}`, which collides with the type \
                 `{}` of the component's WIT interface; rename the method",
                method.fn_ident, method.wit_name, method.wit_name
            ),
        )),
        None => Ok(()),
    }
}

/// Builds the diagnostic for a component `kind` (method or parameter) `ident` whose WIT name
/// `wit_name` is already used by `previous`, pointing at both declarations.
fn duplicate_wit_name_error(
    kind: &str,
    ident: &syn::Ident,
    previous: &syn::Ident,
    wit_name: &str,
) -> syn::Error {
    let mut error = syn::Error::new(
        ident.span(),
        format!(
            "component {kind} `{ident}` produces the WIT name `{wit_name}`, which is already used \
             by {kind} `{previous}`"
        ),
    );
    error.combine(syn::Error::new(previous.span(), format!("first {kind} with this WIT name")));
    error
}

/// Returns true if `ty` names a type through a path, the only shape a component storage type
/// can take.
fn is_path_type(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => !path.path.segments.is_empty(),
        Type::Group(group) => is_path_type(&group.elem),
        Type::Paren(paren) => is_path_type(&paren.elem),
        _ => false,
    }
}

/// Maps a Rust type used in the public interface to the corresponding WIT core-types identifier.
/// Determines whether a type represents the unit type `()`.
fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
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
/// sufficient; `path` is the Miden path of the export, matched against its WIT `@external-id`.
fn auth_script_frontend_metadata(
    namespace: &ComponentNamespace,
    trait_ident: &syn::Ident,
    auth_method_ident: &syn::Ident,
) -> syn::Result<FrontendMetadata> {
    Ok(FrontendMetadata::AuthScript {
        method_path: format!("{trait_ident}::{auth_method_ident}"),
        path: export_path(namespace, auth_method_ident)?,
    })
}

/// Builds frontend metadata for a single `#[account_procedure]` method exported by a component.
///
/// `method_path` is diagnostic-only (used in error messages), so the trait-qualified path is
/// sufficient; `path` is the Miden path of the export, matched against its WIT `@external-id`.
fn account_procedure_frontend_metadata(
    namespace: &ComponentNamespace,
    trait_ident: &syn::Ident,
    account_method_ident: &syn::Ident,
) -> syn::Result<FrontendMetadata> {
    Ok(FrontendMetadata::AccountProcedure {
        method_path: format!("{trait_ident}::{account_method_ident}"),
        path: export_path(namespace, account_method_ident)?,
    })
}

/// Returns the Miden path of the export generated for the method `method_ident`: the namespace
/// followed by the method's Rust identifier (without any `r#` prefix).
///
/// Fails for `init`, whose path belongs to the compiler's component initializer.
pub(crate) fn export_path(
    namespace: &ComponentNamespace,
    method_ident: &syn::Ident,
) -> syn::Result<String> {
    let ident = method_ident.unraw().to_string();
    let path = namespace.procedure_path(&ident);
    // Codegen emits the component initializer as the public `init` procedure next to the exports.
    if ident == COMPONENT_INIT_PROCEDURE {
        return Err(syn::Error::new(
            method_ident.span(),
            format!(
                "`{ident}` would be exported at the path `{path}`, which is reserved for the \
                 compiler's component initializer; rename it"
            ),
        ));
    }
    Ok(path)
}

/// Emits the static metadata blob inside the account-component metadata link section.
fn generate_link_section(metadata_bytes: &[u8]) -> proc_macro2::TokenStream {
    let link_section_bytes_len = metadata_bytes.len();
    let encoded_bytes_str = Literal::byte_string(metadata_bytes);

    quote! {
        #[unsafe(
            // to test it in the integration(this crate) tests the section name needs to make mach-o section
            // specifier happy and to have "segment and section separated by comma"
            link_section = #WASM_ACCOUNT_COMPONENT_METADATA_CUSTOM_SECTION_NAME
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

/// Returns true if any account-procedure marker attribute is present.
fn has_account_procedure_marker_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_account_procedure_marker_attr)
}

/// Returns true if an attribute marks a method as an account-interface procedure.
fn is_account_procedure_marker_attr(attr: &Attribute) -> bool {
    is_attr_named(attr, ACCOUNT_PROCEDURE_ATTR)
        || is_attr_named(attr, ACCOUNT_PROCEDURE_MARKER_ATTR)
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

    /// Namespace used by the frontend-metadata tests.
    fn test_namespace() -> ComponentNamespace {
        ComponentNamespace::parse("miden::test_pkg::test_iface", Span2::call_site()).unwrap()
    }

    #[test]
    fn frontend_metadata_path_uses_unraw_rust_identifier() {
        let trait_ident = format_ident!("Wallet");
        let method_ident: syn::Ident = parse_quote!(r#type);
        let metadata =
            account_procedure_frontend_metadata(&test_namespace(), &trait_ident, &method_ident)
                .unwrap();

        assert_eq!(metadata.path(), "miden::test_pkg::test_iface::type");
    }

    /// Parses component method signatures the way the impl expansion does.
    fn parse_methods(signatures: &[syn::Signature]) -> Vec<ComponentMethod> {
        signatures
            .iter()
            .map(|signature| parse_component_signature(signature, &[], &HashMap::new()).unwrap().0)
            .collect()
    }

    /// Renders the component WIT for `methods` under the test namespace.
    fn method_wit_fixture(methods: &[ComponentMethod]) -> String {
        build_component_wit(ComponentWitSpec {
            namespace: &test_namespace(),
            component_version: &semver::Version::new(1, 0, 0),
            dependency_imports: &[],
            type_imports: &BTreeSet::new(),
            methods,
            exported_types: &[],
        })
        .unwrap()
    }

    /// Locates wit-bindgen's generated `Guest` trait inside its module hierarchy.
    fn generated_guest_trait(items: &[syn::Item]) -> Option<&syn::ItemTrait> {
        items.iter().find_map(|item| match item {
            syn::Item::Trait(item) if item.ident == "Guest" => Some(item),
            syn::Item::Mod(module) => {
                module.content.as_ref().and_then(|(_, items)| generated_guest_trait(items))
            }
            _ => None,
        })
    }

    #[test]
    fn component_methods_match_wit_bindgen_names_and_call_original_rust_methods() {
        use wit_bindgen_core::{WorldGenerator, wit_parser::Resolve};

        let methods = parse_methods(&[
            parse_quote!(fn getURL(&self, r#type: u32) -> u32),
            parse_quote!(fn r#type(&self, r#record: u32)),
            parse_quote!(fn get_count(&self) -> u32),
        ]);
        let wit = method_wit_fixture(&methods);
        assert!(wit.contains("%type: func(%record: u32)"), "{wit}");
        assert!(wit.contains("%get-url: func(%type: u32)"), "{wit}");

        let mut resolve = Resolve::default();
        resolve.push_str("miden.wit", crate::manifest_paths::SDK_WIT_SOURCE).unwrap();
        let package = resolve.push_str("test.wit", &wit).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();
        let interface_name = test_namespace().wit_interface();
        let interface = resolve
            .interfaces
            .iter()
            .find(|(_, interface)| interface.name.as_deref() == Some(interface_name.as_str()))
            .unwrap()
            .1;
        assert_eq!(
            interface.functions.keys().map(String::as_str).collect::<Vec<_>>(),
            ["get-url", "type", "get-count"]
        );
        assert_eq!(interface.functions["get-url"].params[0].name, "type");

        let mut files = wit_bindgen_core::Files::default();
        wit_bindgen_rust::Opts {
            generate_all: true,
            ..Default::default()
        }
        .build()
        .generate(&mut resolve, world, &mut files)
        .unwrap();
        let generated =
            syn::parse_file(std::str::from_utf8(files.iter().next().unwrap().1).unwrap()).unwrap();
        let guest =
            generated_guest_trait(&generated.items).expect("wit-bindgen must generate Guest");
        let guest_names = guest
            .items
            .iter()
            .filter_map(|item| match item {
                TraitItem::Fn(method) => Some(method.sig.ident.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();

        for (method, expected) in methods.iter().zip(["get_url", "type_", "get_count"]) {
            let wrapper = render_guest_method(method, &parse_quote!(Storage), &parse_quote!(Api));
            let function: syn::ItemFn = syn::parse2(wrapper.clone()).unwrap();
            assert_eq!(function.sig.ident, expected);
            assert!(guest_names.iter().any(|name| name == expected), "{guest_names:?}");
            let original = &method.fn_ident;
            let call = quote!(<Storage as Api>::#original).to_string();
            assert!(wrapper.to_string().contains(&call), "{wrapper}");
        }
    }

    #[test]
    fn exported_types_render_fields_and_cases_in_explicit_form() {
        let point: syn::ItemStruct = parse_quote! {
            struct Point {
                r#type: Felt,
                getURL: u32,
            }
        };
        let shape: syn::ItemEnum = parse_quote! {
            enum Shape {
                Record,
                Circle(Word),
            }
        };
        let exported_types = [
            crate::types::exported_type_from_struct(&point).unwrap(),
            crate::types::exported_type_from_enum(&shape).unwrap(),
        ];

        let wit = build_component_wit(ComponentWitSpec {
            namespace: &test_namespace(),
            component_version: &semver::Version::new(1, 0, 0),
            dependency_imports: &[],
            type_imports: &BTreeSet::new(),
            methods: &[],
            exported_types: &exported_types,
        })
        .unwrap();

        for expected in [
            "use core-types.{felt, word};",
            "record point {\n        %type: felt,\n        %get-url: u32,\n    }",
            "variant shape {\n        %record,\n        %circle(word),\n    }",
        ] {
            assert!(wit.contains(expected), "missing `{expected}` in:\n{wit}");
        }
    }

    #[test]
    fn component_methods_reject_colliding_wit_names() {
        let methods =
            parse_methods(&[parse_quote!(fn getURL(&self)), parse_quote!(fn get_url(&self))]);
        let error = reject_duplicate_method_wit_names(&methods).unwrap_err();
        let message = error.to_string();
        for expected in ["getURL", "get_url", "get-url"] {
            assert!(message.contains(expected), "{message}");
        }
        assert_eq!(error.into_iter().count(), 2, "diagnostic must point at both methods");
    }

    #[test]
    fn component_methods_reject_the_name_of_an_interface_type() {
        let methods = parse_methods(&[parse_quote!(fn felt(&self) -> Felt)]);
        let error = build_component_wit(ComponentWitSpec {
            namespace: &test_namespace(),
            component_version: &semver::Version::new(1, 0, 0),
            dependency_imports: &[],
            type_imports: &BTreeSet::from(["felt".to_string()]),
            methods: &methods,
            exported_types: &[],
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "component method `felt` produces the WIT name `felt`, which collides with the type \
             `felt` of the component's WIT interface; rename the method"
        );
    }

    #[test]
    fn component_methods_reject_the_initializer_path() {
        let methods = parse_methods(&[parse_quote!(fn r#init(&self))]);
        let error = export_path(&test_namespace(), &methods[0].fn_ident).unwrap_err();
        assert!(
            error.to_string().contains(
                "`init` would be exported at the path `miden::test_pkg::test_iface::init`, which \
                 is reserved for the compiler's component initializer"
            ),
            "{error}"
        );

        let error = build_component_wit(ComponentWitSpec {
            namespace: &test_namespace(),
            component_version: &semver::Version::new(1, 0, 0),
            dependency_imports: &[],
            type_imports: &BTreeSet::new(),
            methods: &methods,
            exported_types: &[],
        })
        .unwrap_err();
        assert!(error.to_string().contains("reserved for the compiler's component initializer"));
    }

    #[test]
    fn component_parameters_reject_colliding_wit_names() {
        let signature = parse_quote!(fn get(&self, foo__bar: u32, foo_bar: u32));
        let error = parse_component_signature(&signature, &[], &HashMap::new())
            .err()
            .expect("colliding parameter names must fail");
        let message = error.to_string();
        assert!(message.contains("foo__bar") && message.contains("foo_bar"), "{message}");
        assert_eq!(error.into_iter().count(), 2, "diagnostic must point at both parameters");
    }

    #[test]
    fn component_methods_reject_identifiers_without_a_wit_name() {
        let signatures: [syn::Signature; 3] = [
            parse_quote!(fn _1(&self)),
            parse_quote!(fn __(&self)),
            parse_quote!(fn größe(&self)),
        ];
        for signature in signatures {
            let error = parse_component_signature(&signature, &[], &HashMap::new())
                .err()
                .expect("a method without a valid WIT name must fail");
            let message = error.to_string();
            let ident = signature.ident.to_string();
            assert!(message.contains(&ident) && message.contains("WIT name"), "{message}");
        }
    }

    #[test]
    fn component_parameters_reject_identifiers_without_a_wit_name() {
        let signatures: [syn::Signature; 3] = [
            parse_quote!(fn get(&self, _1: u32)),
            parse_quote!(fn get(&self, __: u32)),
            parse_quote!(fn get(&self, größe: u32)),
        ];
        for (signature, ident) in signatures.iter().zip(["_1", "__", "größe"]) {
            let error = parse_component_signature(signature, &[], &HashMap::new())
                .err()
                .expect("a parameter without a valid WIT name must fail");
            let message = error.to_string();
            assert!(message.contains(ident) && message.contains("WIT name"), "{message}");
        }
    }

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
        let metadata =
            auth_script_frontend_metadata(&test_namespace(), &trait_ident, &method.sig.ident)
                .unwrap();

        assert!(matches!(
            metadata,
            FrontendMetadata::AuthScript { path, .. }
                if path == "miden::test_pkg::test_iface::whatever_name"
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
        let metadata =
            auth_script_frontend_metadata(&test_namespace(), &trait_ident, &method_ident).unwrap();
        let tokens = generate_frontend_link_section(&[metadata]).to_string();

        assert!(tokens.contains(crate::util::FRONTEND_METADATA_UNIQUENESS_GUARD_SYMBOL));
    }

    #[test]
    fn auth_script_frontend_metadata_stores_method_path() {
        let trait_ident = format_ident!("AuthComponent");
        let method_ident = format_ident!("whatever_name");
        let metadata =
            auth_script_frontend_metadata(&test_namespace(), &trait_ident, &method_ident).unwrap();

        assert_eq!(
            metadata,
            FrontendMetadata::AuthScript {
                method_path: "AuthComponent::whatever_name".into(),
                path: "miden::test_pkg::test_iface::whatever_name".into(),
            }
        );
    }

    #[test]
    fn account_procedure_frontend_metadata_stores_method_path() {
        let trait_ident = format_ident!("BasicWallet");
        let method_ident = format_ident!("receive_asset");
        let metadata =
            account_procedure_frontend_metadata(&test_namespace(), &trait_ident, &method_ident)
                .unwrap();

        assert_eq!(
            metadata,
            FrontendMetadata::AccountProcedure {
                method_path: "BasicWallet::receive_asset".into(),
                path: "miden::test_pkg::test_iface::receive_asset".into(),
            }
        );
    }

    #[test]
    fn account_procedure_marker_accepts_helper_attribute() {
        let method: TraitItemFn = parse_quote! {
            #[miden_account_procedure_requires_component]
            fn receive_asset(&mut self, asset: Asset);
        };

        assert!(has_account_procedure_marker_attr(&method.attrs));
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
