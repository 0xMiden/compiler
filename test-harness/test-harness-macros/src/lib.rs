use std::path::PathBuf;

use miden_mast_package::Package;
use miden_testing::MockChainBuilder;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

// Returns the identifier for a specific FnArg
fn get_binding_and_type(fn_arg: &syn::FnArg) -> Option<(&syn::PatIdent, &syn::PathSegment)> {
    let syn::FnArg::Typed(arg) = fn_arg else {
        return None;
    };

    let syn::Type::Path(syn::TypePath { path, .. }) = arg.ty.as_ref() else {
        return None;
    };

    // The last token in the segments vector is the actual type, the rest
    // are just path specifiers.
    let path_segment = path.segments.last()?;

    let syn::Pat::Ident(binding) = arg.pat.as_ref() else {
        return None;
    };

    Some((binding, path_segment))
}

/// Function that parses and consumes types T from `function`. `max_args`
/// represents the maximum amount of arguments of type T that `function` may
/// have.
fn process_arguments<T>(
    function: &mut syn::ItemFn,
    max_args: usize,
) -> Result<Vec<syn::Ident>, String> {
    //  "T"'s name as used in the argument list. We skip the whole path
    let struct_name = std::any::type_name::<T>()
        .split("::")
        .last()
        .unwrap_or_else(|| panic!("Failed to split the {}'s", ::core::any::type_name::<T>()));

    let mut found_vars = Vec::new();

    let fn_args = &mut function.sig.inputs;

    *fn_args = fn_args
        .iter()
        .filter(|&fn_arg| {
            let Some((binding, var_type)) = get_binding_and_type(fn_arg) else {
                return true;
            };

            if var_type.ident != struct_name {
                return true;
            }

            found_vars.push(binding.ident.clone());
            false
        })
        .cloned()
        .collect();

    if found_vars.len() > max_args {
        let identifiers = found_vars
            .iter()
            .map(|ident| ident.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        let err = format!(
            "
Detected that all of the following variables are `{struct_name}`s: {identifiers}

#[miden_test] only supports having {max_args} `{struct_name}` in its argument list."
        );
        return Err(err);
    }

    Ok(found_vars)
}

/// Returns the PathBuf containing the `.masp` file of the generated Package.
fn get_package_path() -> PathBuf {
    // This env var is set by `cargo miden test`.
    std::env::var("CARGO_MIDEN_TEST_PACKAGE_PATH")
        .expect("Failed to obtain CARGO_MIDEN_TEST_PACKAGE_PATH environment variable.")
        .into()
}

/// Parse the arguments of a `#[miden-test]` function and check for `Package`s.
///
/// If the function has a single `Package` as argument, then it is removed from
/// the argument list and the boilerplate code to load the generated `Package`
/// into a variable will be generated. The name of the variable will match the
/// one used as argument.
///
/// This will "consume" all the tokens that are of type `Package`.
fn load_package(function: &mut syn::ItemFn) {
    let found_packages_vars =
        process_arguments::<Package>(function, 1).unwrap_or_else(|err| panic!("{err}"));

    let Some(package_binding_name) = found_packages_vars.first() else {
        // If there are no variables with `Package` as its type, then don't load
        // the `Package`.
        return;
    };

    let package_path = get_package_path();

    let package_bytes = std::fs::read(&package_path).unwrap_or_else(|err| {
        panic!("failed to read .masp Package file {} logger: {err}", package_path.display())
    });

    let load_package: Vec<syn::Stmt> = syn::parse_quote! {
        // Inserts every byte from the package_bytes vector separated by a
        // comma.  For more information see:
        // https://docs.rs/quote/latest/quote/macro.quote.html#interpolation
        let bytes = [#(#package_bytes),*];

        let #package_binding_name =
            <::miden_objects::vm::Package as ::miden_objects::utils::Deserializable>::read_from_bytes(&bytes).unwrap();
    };

    // We add the required lines to load the generated Package right at the
    // beginning of the function.
    for (i, package) in load_package.iter().enumerate() {
        function.block.as_mut().stmts.insert(i, package.clone());
    }
}

fn load_mock_chain(function: &mut syn::ItemFn) {
    let found_mock_chain =
        process_arguments::<MockChainBuilder>(function, 1).unwrap_or_else(|err| panic!("{err}"));

    let Some(mock_chain_builder_name) = found_mock_chain.first() else {
        // If there are no variables with `MockChainBuilder` as its type, then don't load
        // the `MockChainBuilder`.
        return;
    };

    let load_mock_chain_builder: Vec<syn::Stmt> = syn::parse_quote! {
        let mut #mock_chain_builder_name = ::miden_test_harness::reexports::miden_testing::MockChainBuilder::new();
    };

    // We add the required lines to load the generated MockChainBuilder right at the
    // beginning of the function.
    for (i, package) in load_mock_chain_builder.iter().enumerate() {
        function.block.as_mut().stmts.insert(i, package.clone());
    }
}

#[proc_macro_attribute]
pub fn miden_test(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut input_fn = parse_macro_input!(item as ItemFn);

    let fn_ident = input_fn.sig.ident.clone();
    let fn_name =
        fn_ident.clone().span().source_text().unwrap_or_else(|| {
            panic!("Failed to obtain function name from identifier '{fn_ident}'.")
        });

    load_package(&mut input_fn);
    load_mock_chain(&mut input_fn);

    let function = quote! {
        ::miden_test_harness::reexports::miden_test_submit!(
           ::miden_test_harness::reexports::MidenTest {
                name: #fn_name,
                test_fn: #fn_ident,
            }
        );

        #[test]
        #[cfg(test)]
        #input_fn
    };

    TokenStream::from(function)
}

#[proc_macro_attribute]
pub fn miden_test_suite(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut input_module = parse_macro_input!(item as syn::ItemMod);

    {
        // We add an internal "use" here in order for the tests inside the `mod
        // tests` block to use the `miden_test` macro without needing to pass
        // the full path.
        let internal_use = syn::parse_quote! {
            use miden_test_harness_macros::miden_test;
        };
        input_module
            .content
            .as_mut()
            .expect("Failed to open 'mod test''s content as mut")
            .1
            .insert(0, internal_use);
    }

    {
        let cfg_test: syn::Attribute = syn::parse_quote!(#[cfg(test)]);

        // We add #[cfg(test)] so that it is only expanded with the test
        // profile. However, we want the code to always be present in order for
        // rust-analyzer to provide information.
        input_module.attrs.insert(0, cfg_test);
    }

    let main_function = {
        quote! {
            #[cfg(test)]
            fn main() {
                let args = ::miden_test_harness::reexports::MidenTestArguments::from_args();

                ::miden_test_harness::reexports::run(args);
            }
        }
    };

    let block = quote! {
        #input_module

        #main_function
    };

    block.into()
}
