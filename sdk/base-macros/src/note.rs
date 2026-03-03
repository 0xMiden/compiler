use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, Item, ItemImpl, ItemStruct, PathArguments, Type,
    parse_macro_input, spanned::Spanned,
};

use crate::script::{GuestWrapperConfig, expand_guest_wrapper};

const NOTE_SCRIPT_ATTR: &str = "note_script";
const NOTE_SCRIPT_DOC_MARKER: &str = "__miden_note_script_marker";

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
/// `#[note]`. It acts as a marker for `#[note]` to locate the entrypoint method.
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

    // `#[note]` uses `#[note_script]` as a marker. Since proc-macro attributes are consumed during
    // expansion, we also attach a stable marker attribute that `#[note]` can reliably detect.
    let marker = syn::LitStr::new(NOTE_SCRIPT_DOC_MARKER, Span::call_site());
    item_fn.attrs.push(syn::parse_quote!(#[doc = #marker]));
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

    match expand_guest_wrapper(
        Span::call_site(),
        GuestWrapperConfig {
            export_interface: "miden:base/note-script@1.0.0",
            guest_trait_path: "self::bindings::exports::miden::base::note_script::Guest",
            guest_struct_ident,
            guest_struct_doc: "Guest entry point generated by the Miden note script macros.",
        },
        quote! { #item_impl },
        account_trait_impl,
        quote! {
                #note_init
                #account_instantiation
                #call
        },
    ) {
        Ok(tokens) => tokens,
        Err(err) => err.into_compile_error(),
    }
}

#[derive(Clone)]
struct AccountParam {
    ty: Type,
    mut_ref: bool,
}

fn note_instantiation(note_ty: &syn::TypePath) -> TokenStream2 {
    // NOTE: Avoid calling `active_note::get_inputs()` for zero-sized note types so that "no input"
    // notes can execute without requiring a full active-note runtime context.
    quote! {
        let __miden_note: #note_ty = if ::core::mem::size_of::<#note_ty>() == 0 {
            match <#note_ty as ::core::convert::TryFrom<&[::miden::Felt]>>::try_from(&[]) {
                Ok(note) => note,
                Err(err) => ::core::panic!("failed to decode note inputs: {err:?}"),
            }
        } else {
            let inputs = ::miden::active_note::get_inputs();
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
    is_attr_named(attr, NOTE_SCRIPT_ATTR) || is_doc_marker_attr(attr, NOTE_SCRIPT_DOC_MARKER)
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
}
