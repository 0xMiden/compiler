use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[cfg(feature = "test-flag")]
fn is_test() -> bool {
    true
}

#[cfg(not(feature = "test-flag"))]
fn is_test() -> bool {
    // true // Uncomment to debug.
    false
}

fn load_account(function: &mut syn::ItemFn) {
    let mut found_packages_vars = Vec::new();

    for arg in function.sig.inputs.iter() {
        let syn::FnArg::Typed(arg) = arg else {
            continue;
        };
        let syn::Type::Path(syn::TypePath { path, .. }) = *arg.ty.clone() else {
            continue;
        };
        // The last token in the segments vector is the actual type, the rest
        // are just path specifiers.
        let Some(maybe_package) = path.segments.last() else {
            continue;
        };

        if maybe_package.ident != "Package" {
            continue;
        }

        let syn::Pat::Ident(package_var_binding) = arg.pat.as_ref() else {
            panic!("Couldn't find binding for package")
        };
        found_packages_vars.push(package_var_binding.ident.clone());
    }

    if found_packages_vars.len() > 1 {
        let identifiers = found_packages_vars
            .iter()
            .map(|ident| ident.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        panic!(
            "
Detected that all of the following variables are `Package`s: {identifiers}

#[miden_test] only supports having a single `Package` in its argument list."
        )
    }

    let Some(package_binding_name) = found_packages_vars.first() else {
        // If there are no variables with `Package` as its value, simply ignore it.
        return;
    };

    // This is env var is set by `cargo miden test`.
    let package_path = std::env::var("CREATED_PACKAGE").unwrap();

    let load_package: Vec<syn::Stmt> = syn::parse_quote! {
        let path = #package_path;
        let bytes = std::fs::read(path).unwrap();
        let #package_binding_name = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
    };

    // We add the the lines required to load the generated Package.
    for (i, package) in load_package.iter().enumerate() {
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
    let fn_name = fn_ident.clone().span().source_text().unwrap();

    load_account(&mut input_fn);

    input_fn.sig.inputs.clear();

    let function = quote! {
        miden_test_harness_lib::miden_test_submit!(
            miden_test_harness_lib::MidenTest {
                name: #fn_name,
                test_fn: #fn_ident,
            }
        );

        #[cfg(test)]
        #input_fn
    };

    TokenStream::from(function)
}

#[proc_macro_attribute]
pub fn miden_test_block(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut input_module = parse_macro_input!(item as syn::ItemMod);

    // We add an internal "use" here in order for the tests inside the `mod tests`
    // block to use the `miden_test` macro without needing to pass the full path.
    let internal_use = syn::parse_quote! {
        use miden_test_harness_macros::miden_test;
    };
    input_module.content.as_mut().unwrap().1.insert(0, internal_use);

    let module = if is_test() {
        quote! {
            #input_module
        }
    } else {
        quote! {}
    };

    let main_function = if is_test() {
        quote! {
            use miden_test_harness_lib;

            fn main() {
                let args = miden_test_harness_lib::MidenTestArguments::from_args();

                miden_test_harness_lib::run(args);
            }
        }
    } else {
        quote! {}
    };

    let block = quote! {
        #module

        #main_function
    };

    block.into()
}
