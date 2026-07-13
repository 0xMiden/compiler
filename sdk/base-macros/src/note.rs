use std::collections::BTreeSet;

use heck::{ToKebabCase, ToSnakeCase};
use midenc_frontend_wasm_metadata::FrontendMetadata;
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, Item, ItemImpl, ItemStruct, PathArguments, Type,
    parse_macro_input, spanned::Spanned,
};

use crate::{
    boilerplate::runtime_boilerplate,
    component_macro::generate_wit::write_component_wit_file,
    types::{TypeRef, map_type_to_type_ref, registered_export_type_map},
    util::generate_frontend_link_section,
    wit_builder::WitBuilder,
    wit_world::{ManifestPackage, write_world_block},
};

const NOTE_SCRIPT_ATTR: &str = "note_script";
const NOTE_SCRIPT_MARKER_ATTR: &str = "miden_note_script_requires_note";
const NOTE_SCRIPT_DOC_MARKER: &str = "__miden_note_script_marker";
const NOTE_CONSTRUCTOR_ATTR: &str = "note_constructor";
const NOTE_CONSTRUCTOR_MARKER_ATTR: &str = "miden_note_constructor_requires_note";
const NOTE_CONSTRUCTOR_DOC_MARKER: &str = "__miden_note_constructor_marker";
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
    expand_method_marker_attr(attr.into(), item.into(), NOTE_SCRIPT_ATTR, NOTE_SCRIPT_MARKER_ATTR)
        .into()
}

/// Expands `#[note_constructor]`.
///
/// This attribute must be applied to a method inside an inherent `impl` block annotated with
/// `#[note]`. It acts as a marker for `#[note]` to export the method through the note's WIT
/// interface as a note constructor.
pub(crate) fn expand_note_constructor(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    expand_method_marker_attr(
        attr.into(),
        item.into(),
        NOTE_CONSTRUCTOR_ATTR,
        NOTE_CONSTRUCTOR_MARKER_ATTR,
    )
    .into()
}

/// Shared expansion of the method-marker attributes (`#[note_script]`, `#[note_constructor]`).
///
/// Appends `marker_attr_name` as a helper attribute for the surrounding `#[note]` impl block to
/// consume. If that impl block forgets `#[note]`, rustc rejects the unknown helper attribute
/// instead of silently compiling a method that is never exported. Free functions parse as
/// [`ImplItemFn`] too and are caught the same way: their marker survives to rustc unconsumed.
fn expand_method_marker_attr(
    attr: TokenStream2,
    item: TokenStream2,
    attr_name: &str,
    marker_attr_name: &str,
) -> TokenStream2 {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "this attribute does not accept arguments")
            .into_compile_error();
    }

    let mut item_fn: ImplItemFn = match syn::parse2(item.clone()) {
        Ok(item_fn) => item_fn,
        Err(_) => {
            // Reached for non-method items only; point at the signature when there is one.
            let span = syn::parse2::<syn::TraitItemFn>(item)
                .map(|item_fn| item_fn.sig.span())
                .unwrap_or_else(|_| Span::call_site());
            return syn::Error::new(
                span,
                format!(
                    "`#[{attr_name}]` must be applied to a method inside a `#[note]` `impl` block"
                ),
            )
            .into_compile_error();
        }
    };

    let marker_attr = format_ident!("{}", marker_attr_name);
    item_fn.attrs.push(syn::parse_quote!(#[#marker_attr]));
    quote!(#item_fn)
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

    let to_felt_repr_impl = note_storage_encoding(&item_struct);
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
        #to_felt_repr_impl
    }
}

/// Generates the note-storage encoding (`ToFeltRepr`) for a note input struct.
///
/// The encoding mirrors the field order of the generated `TryFrom<&[Felt]>` decoding, so a note
/// constructor can serialize the inputs it commits to in the note recipient and the note script
/// can decode them back during execution.
fn note_storage_encoding(item_struct: &ItemStruct) -> TokenStream2 {
    let struct_ident = &item_struct.ident;

    let field_writes: Vec<TokenStream2> = match &item_struct.fields {
        syn::Fields::Unit => Vec::new(),
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .map(|field| {
                let ident = field.ident.as_ref().expect("named fields must have identifiers");
                quote! {
                    ::miden::felt_repr::ToFeltRepr::write_felt_repr(&self.#ident, writer);
                }
            })
            .collect(),
        syn::Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let index = syn::Index::from(index);
                quote! {
                    ::miden::felt_repr::ToFeltRepr::write_felt_repr(&self.#index, writer);
                }
            })
            .collect(),
    };

    let writer_ident = if field_writes.is_empty() {
        quote!(_writer)
    } else {
        quote!(writer)
    };

    quote! {
        impl ::miden::felt_repr::ToFeltRepr for #struct_ident {
            #[inline(always)]
            fn write_felt_repr(&self, #writer_ident: &mut ::miden::felt_repr::FeltWriter<'_>) {
                #(#field_writes)*
            }
        }
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

    let (entrypoint_fn, mut item_impl) = match extract_entrypoint(item_impl) {
        Ok(val) => val,
        Err(err) => return err.into_compile_error(),
    };

    let (arg_idx, account_param) = match parse_entrypoint_signature(&entrypoint_fn) {
        Ok(val) => val,
        Err(err) => return err.into_compile_error(),
    };

    let entrypoint_ident = &entrypoint_fn.sig.ident;
    let export_name = entrypoint_ident.to_string().to_kebab_case();
    let (constructors, constructor_type_imports) =
        match collect_note_constructors(&mut item_impl, entrypoint_ident, &export_name) {
            Ok(val) => val,
            Err(err) => return err.into_compile_error(),
        };
    if let Err(err) = reject_type_import_name_collisions(
        entrypoint_ident,
        &export_name,
        &constructors,
        &constructor_type_imports,
    ) {
        return err.into_compile_error();
    }
    let item_impl = item_impl;
    let note_ident = note_ty
        .path
        .segments
        .last()
        .expect("type path must have at least one segment")
        .ident
        .clone();
    let guest_struct_ident = quote::format_ident!("__MidenNoteScript_{note_ident}");

    let note_init = note_instantiation(&note_ty);
    // The account parameter is instantiated through the `AccountWrapper` marker trait, which is
    // implemented by `#[account(...)]`: this binds the parameter to the active account
    // and rejects types not generated by that macro with a trait-bound error.
    let (account_instantiation, account_arg) = match account_param {
        Some(AccountParam { ty, mut_ref }) => {
            let account_ident = quote::format_ident!("__miden_account");
            (
                quote! {
                    let mut #account_ident =
                        <#ty as ::miden::active_account::AccountWrapper>::active();
                },
                if mut_ref {
                    quote! { &mut #account_ident }
                } else {
                    quote! { &#account_ident }
                },
            )
        }
        None => (quote! {}, quote! {}),
    };

    let args = match build_entrypoint_call_args(entrypoint_fn.sig.span(), arg_idx, account_arg) {
        Ok(args) => args,
        Err(err) => return err.into_compile_error(),
    };
    let call = quote! { __miden_note.#entrypoint_ident(#(#args),*); };

    let metadata = match ManifestPackage::load_or_default(proc_macro::Span::call_site().into()) {
        Ok(metadata) => metadata,
        Err(err) => return err.to_compile_error(),
    };
    let component_package =
        format!("miden:{}", metadata.package.name().into_inner().to_kebab_case());
    let interface_name = component_package.to_kebab_case();
    let world_name = format!("{interface_name}-world");
    let interface_module = interface_name.to_snake_case();
    let manifest = match ManifestPackage::load(Span::call_site()) {
        Ok(manifest) => manifest,
        Err(err) => return err.into_compile_error(),
    };
    let dependency_imports = match manifest.collect_miden_dependency_imports(Span::call_site()) {
        Ok(imports) => imports,
        Err(err) => return err.to_compile_error(),
    };

    let inline_wit = build_note_script_wit(
        &component_package,
        metadata.package.version().inner(),
        &interface_name,
        &world_name,
        &export_name,
        &constructors,
        &constructor_type_imports,
        &dependency_imports,
    );
    // The public WIT file lets other crates declare this note package as a Miden dependency and
    // call its exported constructors. It stays export-only (no dependency imports) so dependents
    // don't have to materialize this note's transitive dependencies next to the generated WIT.
    let public_wit = build_note_script_wit(
        &component_package,
        metadata.package.version().inner(),
        &interface_name,
        &world_name,
        &export_name,
        &constructors,
        &constructor_type_imports,
        &[],
    );
    if let Err(err) =
        write_component_wit_file(proc_macro::Span::call_site(), &public_wit, &component_package)
    {
        return err.into_compile_error();
    }
    let inline_literal = Literal::string(&inline_wit);
    let guest_trait_path = match build_guest_trait_path(&component_package, &interface_module) {
        Ok(path) => path,
        Err(err) => return err.into_compile_error(),
    };
    let runtime_boilerplate = runtime_boilerplate();
    let frontend_metadata = note_script_frontend_metadata(&note_ty, entrypoint_ident, &export_name);
    let frontend_link_section = generate_frontend_link_section(&frontend_metadata);
    let constructor_guest_methods: Vec<TokenStream2> = constructors
        .iter()
        .map(|constructor| render_constructor_guest_method(constructor, &note_ty))
        .collect();
    let entrypoint_root_method = render_entrypoint_root_method(&note_ty);

    quote! {
        #runtime_boilerplate
        #item_impl

        #entrypoint_root_method

        ::miden::generate!(inline = #inline_literal);
        self::bindings::export!(#guest_struct_ident);

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

            #(#constructor_guest_methods)*
        }

        #frontend_link_section
    }
}

/// Renders the generated associated method exposing the note script root.
///
/// Emitted from the `#[note]` impl expansion — not the struct expansion — so the method exists
/// exactly when a `#[note_script]` entrypoint exists. `#[inline(always)]` keeps the compiled
/// output identical to calling the SDK plumbing directly, even in unoptimized builds.
fn render_entrypoint_root_method(note_ty: &syn::TypePath) -> TokenStream2 {
    quote! {
        impl #note_ty {
            /// Returns the MAST root digest of this note's script.
            ///
            /// The digest is the root of the `#[note_script]` entrypoint export as executed by
            /// the transaction kernel, resolved by the compiler at assembly time. Use it to
            /// build the note recipient (e.g. via `note::build_recipient`) in note
            /// constructors.
            ///
            /// Must not be called from code reachable from the `#[note_script]` entrypoint
            /// itself: the note script's MAST root would then depend on its own digest, and
            /// assembly fails with a call-graph cycle error. Inside a running note script, use
            /// `active_note::get_script_root()` instead.
            #[inline(always)]
            pub fn get_entrypoint_root() -> ::miden::Word {
                ::miden::note::__entrypoint_root()
            }
        }
    }
}

#[derive(Clone)]
struct AccountParam {
    ty: Type,
    mut_ref: bool,
}

/// A public associated function of the `#[note]` impl exported through the note's WIT interface.
///
/// Constructors let other Miden packages (e.g. transaction scripts) create this note by calling
/// into the compiled note package. Public functions without a receiver are exported; methods
/// taking `self` stay plain Rust helpers and are not exported.
struct NoteConstructor {
    fn_ident: syn::Ident,
    doc_attrs: Vec<Attribute>,
    params: Vec<ConstructorParam>,
    return_info: ConstructorReturn,
    wit_name: String,
}

/// A WIT function parameter generated from a note constructor argument.
struct ConstructorParam {
    ident: syn::Ident,
    user_ty: Type,
    wit_param_name: String,
    wit_type_name: String,
}

/// The return type of an exported note constructor.
enum ConstructorReturn {
    Unit,
    Type {
        user_ty: Box<Type>,
        wit_type_name: String,
    },
}

/// Collects the note constructors marked with `#[note_constructor]` from the `#[note]` impl
/// block, stripping the marker attributes from the emitted output.
///
/// Returns the constructors along with the set of core-type imports their signatures require.
fn collect_note_constructors(
    item_impl: &mut ItemImpl,
    entrypoint_ident: &syn::Ident,
    entrypoint_export_name: &str,
) -> syn::Result<(Vec<NoteConstructor>, BTreeSet<String>)> {
    let exported_types = registered_export_type_map();
    let mut constructors = Vec::new();
    let mut type_imports = BTreeSet::new();
    let mut wit_names = BTreeSet::new();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        if !method.attrs.iter().any(is_note_constructor_marker_attr) {
            continue;
        }
        // Remove constructor markers so they don't reach the output.
        method.attrs.retain(|attr| !is_note_constructor_marker_attr(attr));

        if &method.sig.ident == entrypoint_ident {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a method cannot be both the `#[note_script]` entrypoint and a \
                 `#[note_constructor]`",
            ));
        }
        if !matches!(method.vis, syn::Visibility::Public(_)) {
            return Err(syn::Error::new(
                method.sig.span(),
                "`#[note_constructor]` methods must be `pub`: they are exported from the compiled \
                 note package",
            ));
        }
        if let Some(receiver) = method.sig.receiver() {
            return Err(syn::Error::new(
                receiver.span(),
                "`#[note_constructor]` methods cannot take `self`: constructors run before the \
                 note exists",
            ));
        }

        let sig = &method.sig;
        if let Some(constness) = sig.constness {
            return Err(syn::Error::new(constness.span(), "note constructors cannot be `const`"));
        }
        if let Some(asyncness) = sig.asyncness {
            return Err(syn::Error::new(asyncness.span(), "note constructors cannot be `async`"));
        }
        if let Some(unsafety) = sig.unsafety {
            return Err(syn::Error::new(unsafety.span(), "note constructors cannot be `unsafe`"));
        }
        if let Some(abi) = &sig.abi {
            return Err(syn::Error::new(
                abi.span(),
                "note constructors cannot specify an `extern` ABI",
            ));
        }
        if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
            return Err(syn::Error::new(
                sig.generics.span(),
                "note constructors cannot be generic",
            ));
        }
        if let Some(variadic) = &sig.variadic {
            return Err(syn::Error::new(variadic.span(), "note constructors cannot be variadic"));
        }

        // The generated bindings implement a trait whose method name wit-bindgen derives by
        // snake-casing the WIT export name; a non-snake-case Rust name would make the generated
        // impl miss the trait method (E0407/E0046 deep inside generated code).
        let ident_string = sig.ident.to_string();
        if ident_string != ident_string.to_snake_case() {
            return Err(syn::Error::new(
                sig.ident.span(),
                "note constructor names must be snake_case: the WIT export name and the generated \
                 bindings derive from the method name",
            ));
        }

        let mut params = Vec::new();
        let mut wit_param_names = BTreeSet::new();
        for arg in &sig.inputs {
            let FnArg::Typed(pat_type) = arg else {
                unreachable!("receiver arguments are rejected above");
            };
            let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
                return Err(syn::Error::new(
                    pat_type.pat.span(),
                    "note constructor parameters must be simple identifiers",
                ));
            };
            let type_ref = map_type_to_type_ref(&pat_type.ty, &exported_types)?;
            reject_custom_type_ref(&type_ref, pat_type.ty.span())?;
            type_ref.add_required_core_type_imports(&mut type_imports);
            // WIT parameter names are kebab-cased, so distinct Rust identifiers can collide;
            // catch that here instead of surfacing a WIT parse error from the generated bindings.
            let wit_param_name = pat_ident.ident.to_string().to_kebab_case();
            if !wit_param_names.insert(wit_param_name.clone()) {
                return Err(syn::Error::new(
                    pat_ident.ident.span(),
                    format!(
                        "note constructor parameter `{}` produces the WIT parameter name \
                         '{wit_param_name}', which is already used by another parameter",
                        pat_ident.ident
                    ),
                ));
            }
            params.push(ConstructorParam {
                wit_param_name,
                ident: pat_ident.ident.clone(),
                user_ty: (*pat_type.ty).clone(),
                wit_type_name: type_ref.wit_name.clone(),
            });
        }

        let return_info = match &sig.output {
            syn::ReturnType::Default => ConstructorReturn::Unit,
            syn::ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Tuple(t) if t.elems.is_empty()) => {
                ConstructorReturn::Unit
            }
            syn::ReturnType::Type(_, ty) => {
                let type_ref = map_type_to_type_ref(ty, &exported_types)?;
                reject_custom_type_ref(&type_ref, ty.span())?;
                type_ref.add_required_core_type_imports(&mut type_imports);
                ConstructorReturn::Type {
                    user_ty: ty.clone(),
                    wit_type_name: type_ref.wit_name.clone(),
                }
            }
        };

        let doc_attrs = method
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"))
            .cloned()
            .collect();

        // WIT export names must be unique across the interface: a constructor can collide with
        // the entrypoint export or with a duplicate method definition. Catch that here instead
        // of surfacing a WIT parse error from the generated bindings.
        let wit_name = sig.ident.to_string().to_kebab_case();
        if wit_name == entrypoint_export_name || !wit_names.insert(wit_name.clone()) {
            return Err(syn::Error::new(
                sig.ident.span(),
                format!(
                    "note constructor `{}` produces the WIT export name '{wit_name}', which is \
                     already used by another export of this note",
                    sig.ident
                ),
            ));
        }

        constructors.push(NoteConstructor {
            wit_name,
            fn_ident: sig.ident.clone(),
            doc_attrs,
            params,
            return_info,
        });
    }

    Ok((constructors, type_imports))
}

/// Rejects exported function names that collide with the interface's imported core type names.
///
/// The generated interface imports core types via `use core-types.{...}`, which places the type
/// names in the same WIT namespace as the exported functions; a collision would surface as a
/// "name defined more than once" parse error inside the generated bindings, so catch it here
/// with a span on the offending Rust identifier.
fn reject_type_import_name_collisions(
    entrypoint_ident: &syn::Ident,
    entrypoint_export_name: &str,
    constructors: &[NoteConstructor],
    constructor_type_imports: &BTreeSet<String>,
) -> syn::Result<()> {
    // Mirrors `build_note_script_wit`: `word` is always imported for the entrypoint parameter.
    let mut imports = constructor_type_imports.clone();
    imports.insert("word".to_string());

    if imports.contains(entrypoint_export_name) {
        return Err(syn::Error::new(
            entrypoint_ident.span(),
            format!(
                "the `#[note_script]` entrypoint `{entrypoint_ident}` produces the WIT export \
                 name '{entrypoint_export_name}', which collides with a core type imported by the \
                 note's interface",
            ),
        ));
    }
    for constructor in constructors {
        if imports.contains(&constructor.wit_name) {
            return Err(syn::Error::new(
                constructor.fn_ident.span(),
                format!(
                    "note constructor `{}` produces the WIT export name '{}', which collides with \
                     a core type imported by the note's interface",
                    constructor.fn_ident, constructor.wit_name
                ),
            ));
        }
    }
    Ok(())
}

/// Rejects `#[export_type]` custom types in note constructor signatures.
fn reject_custom_type_ref(type_ref: &TypeRef, span: Span) -> syn::Result<()> {
    if type_ref.is_custom {
        return Err(syn::Error::new(
            span,
            "custom exported types are not supported in note constructor signatures; use SDK core \
             types (e.g. `Felt`, `Word`, `AccountId`, `Tag`, `NoteType`, `NoteIdx`) or primitives",
        ));
    }
    for dependency in &type_ref.dependencies {
        reject_custom_type_ref(dependency, span)?;
    }
    Ok(())
}

/// Renders the guest trait method forwarding an exported constructor to the user's function.
fn render_constructor_guest_method(
    constructor: &NoteConstructor,
    note_ty: &syn::TypePath,
) -> TokenStream2 {
    let fn_ident = &constructor.fn_ident;
    let doc_attrs = &constructor.doc_attrs;
    let params: Vec<TokenStream2> = constructor
        .params
        .iter()
        .map(|param| {
            let ident = &param.ident;
            let user_ty = &param.user_ty;
            quote!(#ident: #user_ty)
        })
        .collect();
    let args: Vec<TokenStream2> = constructor
        .params
        .iter()
        .map(|param| {
            let ident = &param.ident;
            quote!(#ident)
        })
        .collect();

    match &constructor.return_info {
        ConstructorReturn::Unit => quote! {
            #(#doc_attrs)*
            fn #fn_ident(#(#params),*) {
                #note_ty::#fn_ident(#(#args),*);
            }
        },
        ConstructorReturn::Type { user_ty, .. } => quote! {
            #(#doc_attrs)*
            fn #fn_ident(#(#params),*) -> #user_ty {
                #note_ty::#fn_ident(#(#args),*)
            }
        },
    }
}

/// Renders the WIT function signature for an exported note constructor.
fn constructor_wit_signature(constructor: &NoteConstructor) -> String {
    let params = constructor
        .params
        .iter()
        .map(|param| format!("{}: {}", param.wit_param_name, param.wit_type_name))
        .collect::<Vec<_>>()
        .join(", ");
    match &constructor.return_info {
        ConstructorReturn::Unit => format!("{}: func({params});", constructor.wit_name),
        ConstructorReturn::Type { wit_type_name, .. } => {
            format!("{}: func({params}) -> {wit_type_name};", constructor.wit_name)
        }
    }
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

    // The generated bindings implement a trait whose method name wit-bindgen derives by
    // snake-casing the WIT export name; a non-snake-case Rust name would make the generated
    // impl miss the trait method (E0407/E0046 deep inside generated code).
    let ident_string = sig.ident.to_string();
    if ident_string != ident_string.to_snake_case() {
        return Err(syn::Error::new(
            sig.ident.span(),
            "entrypoint method names must be snake_case: the WIT export name and the generated \
             bindings derive from the method name",
        ));
    }

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
            "entrypoint must accept 1 or 2 arguments (excluding `self`): `Word` and an optional \
             reference to an `#[account(...)]` type",
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
            "unsupported entrypoint parameter type; expected `Word` and an optional reference to \
             an `#[account(...)]` type",
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
            "entrypoint with two parameters must include a reference to an `#[account(...)]` type",
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
    // Any reference to a concrete path type other than `Word` is treated as the account
    // parameter. The generated glue instantiates it through the `AccountWrapper` trait, so
    // types not generated by `#[account(...)]` are rejected by the trait bound.
    if !matches!(type_ref.elem.as_ref(), Type::Path(_)) {
        return None;
    }
    if is_type_named(type_ref.elem.as_ref(), "Word") {
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
    // Only the bare-path form (`#[name]`, `#[miden::name]`) is a marker. An arguments-carrying
    // form must not be recognized here: `#[note]` would strip it before the standalone attribute
    // macro gets the chance to reject the arguments.
    if !matches!(attr.meta, syn::Meta::Path(_)) {
        return false;
    }
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

/// Returns true if an attribute marks a method as an exported note constructor.
fn is_note_constructor_marker_attr(attr: &Attribute) -> bool {
    is_attr_named(attr, NOTE_CONSTRUCTOR_ATTR)
        || is_attr_named(attr, NOTE_CONSTRUCTOR_MARKER_ATTR)
        || is_doc_marker_attr(attr, NOTE_CONSTRUCTOR_DOC_MARKER)
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
///
/// The interface exports the note-script entrypoint plus any note constructors collected from
/// the `#[note]` impl block.
#[allow(clippy::too_many_arguments)]
fn build_note_script_wit(
    component_package: &str,
    component_version: &semver::Version,
    interface_name: &str,
    world_name: &str,
    export_name: &str,
    constructors: &[NoteConstructor],
    constructor_type_imports: &BTreeSet<String>,
    dependency_imports: &[String],
) -> String {
    let mut wit = WitBuilder::new("#[note]", component_package, component_version);
    wit.use_path(CORE_TYPES_PACKAGE);
    wit.blank_line();
    wit.interface(interface_name, |interface| {
        // `word` is always required by the entrypoint's `arg` parameter
        let mut type_imports = constructor_type_imports.clone();
        type_imports.insert("word".to_string());
        let imports = type_imports.iter().cloned().collect::<Vec<_>>().join(", ");
        interface.line(&format!("use core-types.{{{imports}}};"));
        interface.blank_line();
        interface.line(&format!("{export_name}: func(arg: word);"));
        for constructor in constructors {
            interface.line(&constructor_wit_signature(constructor));
        }
    });
    wit.blank_line();
    let exports = [interface_name.to_string()];
    write_world_block(&mut wit, world_name, dependency_imports, &exports);

    wit.finish()
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

/// Builds frontend metadata for the `#[note_script]` method exported by a note.
fn note_script_frontend_metadata(
    note_ty: &syn::TypePath,
    entrypoint_ident: &syn::Ident,
    export_name: &str,
) -> FrontendMetadata {
    FrontendMetadata::NoteScript {
        method_path: render_method_path(note_ty, entrypoint_ident),
        export_name: export_name.to_owned(),
    }
}

/// Renders a Rust method path for frontend metadata diagnostics.
fn render_method_path(note_ty: &syn::TypePath, entrypoint_ident: &syn::Ident) -> String {
    let note_path = note_ty.to_token_stream().to_string().replace(" :: ", "::");
    format!("{note_path}::{entrypoint_ident}")
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
    fn entrypoint_signature_accepts_account_wrapper_reference() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn execute(self, _arg: Word, account: &mut BasicWallet) {}
        };

        assert!(parse_entrypoint_signature(&item_fn).is_ok());
    }

    #[test]
    fn entrypoint_signature_accepts_account_named_wrapper_type() {
        // A user-defined `#[account(...)]` struct may be named `Account`; whether the type
        // really is an account wrapper is enforced by the `AccountWrapper` bound, not by name.
        let item_fn: ImplItemFn = parse_quote! {
            pub fn execute(self, _arg: Word, account: &mut Account) {}
        };

        assert!(parse_entrypoint_signature(&item_fn).is_ok());
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
        let note_ty: syn::TypePath = parse_quote!(crate::notes::PaymentNote);
        let entrypoint_ident = format_ident!("execute");
        let metadata = note_script_frontend_metadata(&note_ty, &entrypoint_ident, "execute");
        let tokens = generate_frontend_link_section(&metadata).to_string();

        assert!(tokens.contains(crate::util::FRONTEND_METADATA_UNIQUENESS_GUARD_SYMBOL));
        assert!(tokens.contains("execute"));
    }

    #[test]
    fn note_script_frontend_metadata_stores_method_path() {
        let note_ty: syn::TypePath = parse_quote!(crate::notes::PaymentNote);
        let entrypoint_ident = format_ident!("execute");

        let metadata = note_script_frontend_metadata(&note_ty, &entrypoint_ident, "execute");

        assert_eq!(
            metadata,
            FrontendMetadata::NoteScript {
                method_path: "crate::notes::PaymentNote::execute".into(),
                export_name: "execute".into(),
            }
        );
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
            &BTreeSet::new(),
            &[],
        );

        assert!(wit.contains("execute: func(arg: word);"));
        assert!(!wit.contains("run: func(arg: word);"));
    }

    #[test]
    fn note_script_wit_exports_marked_constructors() {
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                /// Creates the note.
                #[note_constructor]
                pub fn create(target: AccountId, tag: Tag, note_type: NoteType, serial_num: Word) -> NoteIdx {
                    unimplemented!()
                }

                pub fn execute(self, _arg: Word) {}

                // Not exported: not marked with `#[note_constructor]`.
                pub fn helper(x: Felt) -> Felt { x }

                fn internal(x: Felt) -> Felt { x }
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let (constructors, type_imports) =
            collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute").unwrap();

        assert_eq!(constructors.len(), 1);
        let wit = build_note_script_wit(
            "miden:my-note",
            &semver::Version::new(1, 0, 0),
            "my-note",
            "my-note-world",
            "execute",
            &constructors,
            &type_imports,
            &[],
        );

        assert!(wit.contains(
            "create: func(target: account-id, tag: tag, note-type: note-type, serial-num: word) \
             -> note-idx;"
        ));
        assert!(wit.contains("execute: func(arg: word);"));
        assert!(
            wit.contains("use core-types.{account-id, note-idx, note-type, tag, word};"),
            "unexpected core-types imports in: {wit}"
        );
        assert!(!wit.contains("helper"));
        assert!(!wit.contains("internal"));

        // The marker attributes must not survive into the emitted impl block.
        for item in &item_impl.items {
            let ImplItem::Fn(method) = item else {
                continue;
            };
            assert!(
                method.attrs.iter().all(|attr| !is_note_constructor_marker_attr(attr)),
                "constructor markers must be removed from output"
            );
        }
    }

    #[test]
    fn note_constructors_reject_references() {
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn create(target: &AccountId) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("references must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("references are not supported"));
    }

    #[test]
    fn note_constructors_require_pub() {
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                fn create(target: AccountId) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("non-pub constructors must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must be `pub`"));
    }

    #[test]
    fn note_constructors_reject_receivers() {
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn create(&self) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("receiver-taking constructors must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("cannot take `self`"));
    }

    #[test]
    fn note_constructors_reject_duplicate_wit_names() {
        // Duplicate method definitions are a later rustc error, but the macro sees them first
        // and must not render a WIT interface with two same-named exports.
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn make_note(serial_num: Word) {}
                #[note_constructor]
                pub fn make_note(tag: Tag) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("duplicate WIT export names must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("already used by another export"));
    }

    #[test]
    fn note_constructors_reject_non_snake_case_names() {
        // wit-bindgen names the generated trait method by snake-casing the WIT export name, so a
        // camelCase constructor would not match its trait item.
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn makeNote(serial_num: Word) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("non-snake-case constructor names must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must be snake_case"));
    }

    #[test]
    fn entrypoint_signature_rejects_non_snake_case_names() {
        let item_fn: ImplItemFn = parse_quote! {
            pub fn runNote(self, _arg: Word) {}
        };

        let err = match parse_entrypoint_signature(&item_fn) {
            Ok(_) => panic!("non-snake-case entrypoint names must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("must be snake_case"));
    }

    #[test]
    fn note_constructors_reject_duplicate_wit_param_names() {
        // Kebab-casing maps distinct Rust parameter identifiers to the same WIT parameter name.
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn create(note_type: Word, noteType: Word) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute") {
            Ok(_) => panic!("duplicate WIT parameter names must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("already used by another parameter"));
    }

    #[test]
    fn note_exports_reject_core_type_import_name_collisions() {
        // The constructor `tag` exports the WIT name 'tag' while its `Tag` parameter imports the
        // core type of the same name into the interface namespace.
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn tag(value: Tag) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");
        let (constructors, type_imports) =
            collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute").unwrap();

        let err = match reject_type_import_name_collisions(
            &entrypoint_ident,
            "execute",
            &constructors,
            &type_imports,
        ) {
            Ok(_) => panic!("export names colliding with imported type names must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("collides with a core type"));

        // The entrypoint always imports `word`, so an entrypoint exporting the name 'word'
        // collides even without constructors.
        let word_ident = format_ident!("word");
        let err =
            match reject_type_import_name_collisions(&word_ident, "word", &[], &BTreeSet::new()) {
                Ok(_) => panic!("entrypoint name colliding with the word import must be rejected"),
                Err(err) => err,
            };
        assert!(err.to_string().contains("collides with a core type"));

        // No collision when the type of the same name is never imported.
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn tag(value: Word) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let (constructors, type_imports) =
            collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute").unwrap();
        reject_type_import_name_collisions(
            &entrypoint_ident,
            "execute",
            &constructors,
            &type_imports,
        )
        .expect("an export name matching a non-imported core type is legal WIT");
    }

    #[test]
    fn marker_attrs_with_arguments_are_not_recognized() {
        // An arguments-carrying form must survive `#[note]` unrecognized so the standalone
        // attribute macro can reject the arguments itself.
        let constructor_attr: Attribute = parse_quote!(#[note_constructor(unexpected)]);
        assert!(!is_note_constructor_marker_attr(&constructor_attr));
        let script_attr: Attribute = parse_quote!(#[note_script(unexpected)]);
        assert!(!is_entrypoint_marker_attr(&script_attr));

        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor(unexpected)]
                pub fn create(serial_num: Word) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");
        let (constructors, _) =
            collect_note_constructors(&mut item_impl, &entrypoint_ident, "execute").unwrap();
        assert!(constructors.is_empty(), "the arguments-carrying form must not be exported");

        let ImplItem::Fn(method) = item_impl.items.first().expect("method must exist") else {
            panic!("expected function method");
        };
        assert!(
            method.attrs.iter().any(|attr| attr.path().is_ident("note_constructor")),
            "the attribute must be left in place for the standalone macro to reject"
        );
    }

    #[test]
    fn note_constructors_reject_entrypoint_name_collision() {
        let mut item_impl: ItemImpl = parse_quote! {
            impl MyNote {
                #[note_constructor]
                pub fn run(serial_num: Word) {}
                pub fn execute(self, _arg: Word) {}
            }
        };
        let entrypoint_ident = format_ident!("execute");

        let err = match collect_note_constructors(&mut item_impl, &entrypoint_ident, "run") {
            Ok(_) => panic!("collision with the entrypoint export name must be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("already used by another export"));
    }

    #[test]
    fn note_constructor_marker_accepts_helper_and_qualified_attributes() {
        let helper: ImplItemFn = parse_quote! {
            #[miden_note_constructor_requires_note]
            pub fn create(serial_num: Word) {}
        };
        assert!(helper.attrs.iter().any(is_note_constructor_marker_attr));

        let qualified: ImplItemFn = parse_quote! {
            #[miden::note_constructor]
            pub fn create(serial_num: Word) {}
        };
        assert!(qualified.attrs.iter().any(is_note_constructor_marker_attr));
    }

    #[test]
    fn entrypoint_root_method_calls_the_sdk_plumbing() {
        let note_ty: syn::TypePath = parse_quote!(crate::notes::PaymentNote);

        let rendered = render_entrypoint_root_method(&note_ty).to_string();

        assert!(rendered.contains("get_entrypoint_root"));
        assert!(
            rendered.contains("__entrypoint_root"),
            "the generated method must delegate to the hidden SDK plumbing: {rendered}"
        );
    }

    #[test]
    fn method_marker_expansion_appends_helper_and_rejects_misuse() {
        let expanded = expand_method_marker_attr(
            TokenStream2::new(),
            quote!(
                pub fn create(serial_num: Word) {}
            ),
            NOTE_CONSTRUCTOR_ATTR,
            NOTE_CONSTRUCTOR_MARKER_ATTR,
        );
        let expanded_fn: ImplItemFn =
            syn::parse2(expanded).expect("expansion must remain a method");
        assert!(
            expanded_fn.attrs.iter().any(is_note_constructor_marker_attr),
            "the helper marker must be appended for `#[note]` to consume"
        );

        let rejected_args = expand_method_marker_attr(
            quote!(unexpected),
            quote!(
                pub fn create(serial_num: Word) {}
            ),
            NOTE_CONSTRUCTOR_ATTR,
            NOTE_CONSTRUCTOR_MARKER_ATTR,
        )
        .to_string();
        assert!(rejected_args.contains("does not accept arguments"), "got: {rejected_args}");

        let rejected_item = expand_method_marker_attr(
            TokenStream2::new(),
            quote!(
                struct NotAMethod;
            ),
            NOTE_SCRIPT_ATTR,
            NOTE_SCRIPT_MARKER_ATTR,
        )
        .to_string();
        assert!(rejected_item.contains("must be applied to a method"), "got: {rejected_item}");
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
