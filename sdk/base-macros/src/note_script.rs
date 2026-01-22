use proc_macro2::Span;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatIdent, Type, parse_macro_input, spanned::Spanned};

/// Expands `#[note_script]` as a compatibility shim over `#[note]` + `#[entrypoint]`.
///
/// This allows older note scripts written as a free `fn run(...)` to continue to work, while the
/// underlying implementation is handled by the `#[note]` macro.
pub(crate) fn expand_note_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "this attribute does not accept arguments")
            .into_compile_error()
            .into();
    }

    let input_fn = parse_macro_input!(item as ItemFn);

    if input_fn.sig.receiver().is_some() {
        return syn::Error::new(input_fn.sig.span(), "this attribute cannot target methods")
            .into_compile_error()
            .into();
    }

    let (params, call_args) = match parse_note_script_params(&input_fn) {
        Ok(val) => val,
        Err(err) => return err.into_compile_error().into(),
    };

    let fn_ident = &input_fn.sig.ident;
    let note_ident = quote::format_ident!("__MidenNoteScript");

    let expanded = quote! {
        #input_fn

        #[miden::note]
        struct #note_ident;

        #[miden::note]
        impl #note_ident {
            #[miden::entrypoint]
            pub fn run(self, #(#params),*) {
                let _ = #fn_ident(#(#call_args),*);
            }
        }
    };

    expanded.into()
}

fn parse_note_script_params(input_fn: &ItemFn) -> syn::Result<(Vec<FnArg>, Vec<syn::Ident>)> {
    if input_fn.sig.inputs.is_empty() || input_fn.sig.inputs.len() > 2 {
        return Err(syn::Error::new(
            input_fn.sig.span(),
            "note script entrypoint must accept 1 or 2 arguments: `Word` and optional \
             `&Account`/`&mut Account`",
        ));
    }

    let mut params = Vec::new();
    let mut call_args = Vec::new();
    let mut word_count = 0usize;
    let mut account_count = 0usize;

    for arg in &input_fn.sig.inputs {
        let FnArg::Typed(pat_type) = arg else {
            return Err(syn::Error::new(
                arg.span(),
                "unexpected receiver in note script entrypoint",
            ));
        };

        let ident = match pat_type.pat.as_ref() {
            Pat::Ident(PatIdent { ident, .. }) => ident.clone(),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "note script arguments must be simple identifiers",
                ));
            }
        };

        if is_type_named(pat_type.ty.as_ref(), "Word") {
            word_count += 1;
        } else if parse_account_ref_type(pat_type.ty.as_ref()).is_some() {
            account_count += 1;
        } else {
            return Err(syn::Error::new(
                pat_type.ty.span(),
                "unsupported note script parameter type; expected `Word` and optional \
                 `&Account`/`&mut Account`",
            ));
        }

        params.push(arg.clone());
        call_args.push(ident);
    }

    if word_count != 1 {
        return Err(syn::Error::new(
            input_fn.sig.span(),
            "note script entrypoint must declare exactly one `Word` parameter",
        ));
    }

    if account_count > 1 {
        return Err(syn::Error::new(
            input_fn.sig.span(),
            "note script entrypoint may only declare a single account parameter",
        ));
    }

    if input_fn.sig.inputs.len() == 2 && account_count == 0 {
        return Err(syn::Error::new(
            input_fn.sig.span(),
            "note script entrypoint with two parameters must include an account reference \
             (`&Account` or `&mut Account`)",
        ));
    }

    Ok((params, call_args))
}

fn parse_account_ref_type(ty: &Type) -> Option<()> {
    let Type::Reference(type_ref) = ty else {
        return None;
    };
    if !is_type_named(type_ref.elem.as_ref(), "Account") {
        return None;
    }
    Some(())
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
        .is_some_and(|seg| seg.ident == name && seg.arguments.is_empty())
}
