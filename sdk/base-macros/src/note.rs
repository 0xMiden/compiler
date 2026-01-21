use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, Item, ItemImpl, ItemStruct, PathArguments, Type,
    parse_macro_input, spanned::Spanned,
};

use crate::{boilerplate::runtime_boilerplate, script::build_script_wit};

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

/// Expands `#[entrypoint]`.
///
/// This macro intentionally performs minimal validation: the associated `#[note]` macro is
/// responsible for enforcing the supported entrypoint signature and generating the guest wrapper.
pub(crate) fn expand_entrypoint(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "this attribute does not accept arguments")
            .into_compile_error()
            .into();
    }

    // No transformation is required here; `#[note]` uses this attribute as a marker.
    item
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
                impl ::core::convert::From<&[::miden::Felt]> for #struct_ident {
                    #[inline(always)]
                    fn from(felts: &[::miden::Felt]) -> Self {
                        debug_assert!(
                            felts.is_empty(),
                            "unit note input struct must be decoded from an empty slice"
                        );
                        Self
                    }
                }
            }
        }
        syn::Fields::Named(fields) => {
            let field_inits = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named fields must have identifiers");
                let ty = &field.ty;
                quote! {
                    #ident: <#ty as ::miden::felt_repr::FromFeltRepr>::from_felt_repr(&mut reader)
                }
            });

            quote! {
                impl ::core::convert::From<&[::miden::Felt]> for #struct_ident {
                    #[inline(always)]
                    fn from(felts: &[::miden::Felt]) -> Self {
                        let mut reader = ::miden::felt_repr::FeltReader::new(felts);
                        Self { #(#field_inits),* }
                    }
                }
            }
        }
        syn::Fields::Unnamed(fields) => {
            let field_inits = fields.unnamed.iter().map(|field| {
                let ty = &field.ty;
                quote! {
                    <#ty as ::miden::felt_repr::FromFeltRepr>::from_felt_repr(&mut reader)
                }
            });

            quote! {
                impl ::core::convert::From<&[::miden::Felt]> for #struct_ident {
                    #[inline(always)]
                    fn from(felts: &[::miden::Felt]) -> Self {
                        let mut reader = ::miden::felt_repr::FeltReader::new(felts);
                        Self(#(#field_inits),*)
                    }
                }
            }
        }
    };

    let load_impl = match &item_struct.fields {
        syn::Fields::Unit => quote! {
            impl #struct_ident {
                #[doc(hidden)]
                #[inline(always)]
                pub(crate) fn __miden_load_from_active_note() -> Self {
                    Self
                }
            }
        },
        _ => quote! {
            impl #struct_ident {
                #[doc(hidden)]
                #[inline(always)]
                pub(crate) fn __miden_load_from_active_note() -> Self {
                    let inputs = ::miden::active_note::get_inputs();
                    inputs.as_slice().into()
                }
            }
        },
    };

    quote! {
        #item_struct
        #from_impl
        #load_impl
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

    // Build the bindings world and guest wrapper.
    let inline_wit = match build_script_wit(Span::call_site(), "miden:base/note-script@1.0.0") {
        Ok(wit) => wit,
        Err(err) => return err.into_compile_error(),
    };
    let inline_literal = Literal::string(&inline_wit);

    let export_path: syn::Path =
        match syn::parse_str("self::bindings::exports::miden::base::note_script::Guest") {
            Ok(path) => path,
            Err(err) => {
                return syn::Error::new(
                    Span::call_site(),
                    format!("failed to parse guest trait path: {err}"),
                )
                .into_compile_error();
            }
        };

    let runtime_boilerplate = runtime_boilerplate();

    let entrypoint_ident = &entrypoint_fn.sig.ident;
    let note_ident = note_ty
        .path
        .segments
        .last()
        .expect("type path must have at least one segment")
        .ident
        .clone();
    let guest_struct_ident = quote::format_ident!("__MidenNoteScript_{note_ident}");

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

    let args = build_entrypoint_call_args(arg_idx, account_arg);
    let call = quote! { __miden_note.#entrypoint_ident(#(#args),*); };

    quote! {
        #runtime_boilerplate

        #item_impl

        ::miden::generate!(inline = #inline_literal);
        self::bindings::export!(#guest_struct_ident);

        #account_trait_impl

        // Bring ActiveAccount trait into scope so users can call account.get_id(), etc.
        #[allow(unused_imports)]
        use ::miden::active_account::ActiveAccount as _;

        /// Guest entry point generated by the Miden note script macros.
        pub struct #guest_struct_ident;

        impl #export_path for #guest_struct_ident {
            fn run(arg: ::miden::Word) {
                #note_init
                #account_instantiation
                #call
            }
        }
    }
}

#[derive(Clone)]
struct AccountParam {
    ty: Type,
    mut_ref: bool,
}

fn note_instantiation(note_ty: &syn::TypePath) -> TokenStream2 {
    let create = quote! {
        let note: #note_ty = <#note_ty>::__miden_load_from_active_note();
    };

    quote! {
        #create
        let __miden_note = note;
    }
}

fn extract_entrypoint(mut item_impl: ItemImpl) -> syn::Result<(ImplItemFn, ItemImpl)> {
    let mut entrypoints = Vec::new();
    let mut run_fns = Vec::new();

    for item in &mut item_impl.items {
        let ImplItem::Fn(item_fn) = item else {
            continue;
        };

        if item_fn.sig.ident == "run" {
            run_fns.push(item_fn.clone());
        }

        if has_marker_attr(&item_fn.attrs, "entrypoint") {
            entrypoints.push(item_fn.clone());
            // Remove the marker to avoid requiring it to expand after `#[note]`.
            item_fn.attrs.retain(|attr| !is_attr_named(attr, "entrypoint"));
        }
    }

    // Robustness: if `#[entrypoint]` expanded before `#[note]`, the marker may have already been
    // removed. In that case we fall back to selecting `fn run` as long as it is unambiguous.
    let candidates = if entrypoints.is_empty() {
        run_fns
    } else {
        entrypoints
    };

    match candidates.as_slice() {
        [only] => Ok((only.clone(), item_impl)),
        [] => Err(syn::Error::new(
            item_impl.span(),
            "`#[note]` requires an entrypoint method (annotate `fn run` with `#[entrypoint]`)",
        )),
        _ => Err(syn::Error::new(
            item_impl.span(),
            "`#[note]` requires a single entrypoint method (only one `fn run` is allowed)",
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

    if sig.ident != "run" {
        return Err(syn::Error::new(
            sig.ident.span(),
            "`#[entrypoint]` must be applied to `fn run`",
        ));
    }

    let receiver = sig.receiver().ok_or_else(|| {
        syn::Error::new(sig.span(), "`#[entrypoint]` cannot target free functions")
    })?;

    if receiver.reference.is_some() {
        return Err(syn::Error::new(
            receiver.span(),
            "entrypoint receiver must be `self` (by value); `&self` / `&mut self` are not \
             supported",
        ));
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
            "entrypoint with two parameters must include an account reference (`&T` or `&mut T`)",
        ));
    }

    Ok((*word_idx, account))
}

fn build_entrypoint_call_args(arg_word_idx: usize, account_arg: TokenStream2) -> Vec<TokenStream2> {
    let arg = quote! { arg };

    if account_arg.is_empty() {
        return vec![arg];
    }

    match arg_word_idx {
        0 => vec![arg, account_arg],
        1 => vec![account_arg, arg],
        _ => vec![arg],
    }
}

fn parse_account_ref_type(ty: &Type) -> Option<(Type, bool)> {
    let Type::Reference(type_ref) = ty else {
        return None;
    };
    Some(((*type_ref.elem).clone(), type_ref.mutability.is_some()))
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

fn has_marker_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| is_attr_named(attr, name))
}

fn is_attr_named(attr: &Attribute, name: &str) -> bool {
    attr.path().is_ident(name)
}
