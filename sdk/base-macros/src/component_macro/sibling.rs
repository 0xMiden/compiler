//! Sibling component call support for the `#[component]` trait expansion.
//!
//! An account may be deployed with several components; `#[component(pkg::Interface, ...)]` on the
//! component trait declares the other ("sibling") components this one calls into. Each reference
//! expands to a generated Rust trait named after the interface whose default methods call the
//! wit-bindgen imports of the sibling's WIT interface. Those imports lower to direct
//! cross-context `call`s — the same mechanism note scripts use to call the account — and resolve
//! at link time against the dependency package, so unlike FPI no `.masp` artifact is read at
//! macro expansion time.
//!
//! The generated traits attach to the component's storage struct through an empty blanket impl
//! bound on [`NativeAccount`](https://docs.rs/miden), which `#[component_storage]` implements:
//! only code acting as the transaction's native account may make intra-account sibling calls.
//! Method bodies live in the trait as defaults, so the blanket impl stays empty and the storage
//! struct picks the methods up without any user-written glue.

use heck::{ToKebabCase, ToSnakeCase};
use proc_macro2::{Ident, Span as Span2, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{Error, ItemFn, ReturnType, TraitItemFn, parse_quote};

use crate::{
    dependency_ref::{DependencyRef, select_dependencies},
    fpi, generate, manifest_paths,
    wit_world::{ManifestPackage, SelectedDependency, import_world_wit},
};

/// Name of the inline WIT world (and package) generated for sibling component imports.
const SIBLING_BINDINGS_WORLD: &str = "sibling-bindings";

/// Expands the sibling references of a `#[component(...)]` trait into generated sibling traits.
///
/// Emits one hidden bindings module holding the wit-bindgen imports for all referenced sibling
/// interfaces, then per reference a `pub trait <Interface>` with default methods forwarding to
/// those imports, attached to storage types via an empty blanket impl over `NativeAccount`.
pub(super) fn expand_sibling_traits(
    metadata: &ManifestPackage,
    component_trait_ident: &Ident,
    refs: &[DependencyRef],
) -> syn::Result<TokenStream2> {
    reject_aliases(refs)?;
    reject_duplicate_trait_names(refs)?;
    reject_component_trait_name_collision(component_trait_ident, refs)?;

    let dependencies = select_dependencies(metadata, refs, Span2::call_site())?;
    let imports = dependencies
        .iter()
        .map(|dependency| dependency.import().to_owned())
        .collect::<Vec<_>>();
    let with_entries = fpi::dependency_type_with_entries(&dependencies);
    let inline_wit = import_world_wit(SIBLING_BINDINGS_WORLD, &imports);
    let wit_config = manifest_paths::resolve_wit_paths(manifest_paths::ResolveOptions {
        allow_missing_local_wit: true,
    })?;
    let bindings = generate::generate_inline_import_bindings(
        &wit_config,
        &inline_wit,
        SIBLING_BINDINGS_WORLD,
        &with_entries,
    )
    .map_err(|err| augment_missing_sibling_wit(err, &dependencies))?;

    let file: syn::File = syn::parse2(bindings)?;
    let modules = fpi::collect_import_modules(&file.items, &fpi::is_plain_import_function)?;
    let hidden_module_ident = sibling_bindings_module_ident(component_trait_ident);

    let mut traits = Vec::with_capacity(refs.len());
    for (reference, dependency) in refs.iter().zip(&dependencies) {
        let module_path_string = fpi::import_module_path(dependency.import());
        let module = modules
            .iter()
            .find(|module| module.path_string == module_path_string)
            .ok_or_else(|| {
                Error::new(
                    reference.span,
                    format!(
                        "sibling component interface `{}` of dependency `{}` has no callable \
                         exports",
                        reference.interface, reference.package
                    ),
                )
            })?;
        traits.push(build_sibling_trait(reference, dependency, module, &hidden_module_ident)?);
    }

    let bindings_tokens = file_tokens(file);
    Ok(quote! {
        #[doc(hidden)]
        #[allow(dead_code)]
        pub mod #hidden_module_ident {
            #bindings_tokens
        }

        #(#traits)*
    })
}

/// Rewrites a missing-package failure from sibling binding generation into actionable guidance.
///
/// A sibling reference is selected by reading the dependency's generated WIT (which
/// `wit_world::collect_miden_dependencies` finds under `target/generated-wit`), but the inline
/// `generate!` resolves imports against `manifest_paths::resolve_wit_paths`, which only puts a
/// dependency's WIT on the search path when `[package.metadata.miden.dependencies].<name>.wit` is
/// declared (or a `wit/` directory sits at the dependency root). Without that manifest entry the
/// reference selects successfully and then fails here with a bare wit-parser "package not found".
/// This maps that case to a diagnostic naming the dependencies and the manifest entry to add.
fn augment_missing_sibling_wit(err: syn::Error, dependencies: &[SelectedDependency]) -> syn::Error {
    let message = err.to_string();
    if !message.contains("not found") {
        return err;
    }

    // Only the "package '<id>' not found" portion names the missing package; wit-parser appends a
    // `known packages:` list of the packages it *did* resolve. Matching against the whole message
    // would blame a resolved sibling that happens to appear in that list, so restrict the search to
    // the text before it. Within that, match up to the version boundary (`<pkg>@`) so a package id
    // that is a prefix of another (`miden:counter` vs `miden:counter-contract`) is not over-matched.
    let not_found = message.split("known packages").next().unwrap_or(message.as_str());
    let missing = dependencies
        .iter()
        .filter(|dependency| {
            let package = dependency.import().split('/').next().unwrap_or(dependency.import());
            not_found.contains(&format!("{package}@"))
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return err;
    }

    let hints = missing
        .iter()
        .map(|dependency| {
            format!(
                "  [package.metadata.miden.dependencies]\n  \"{}\" = {{ wit = \"{}\" }}",
                dependency.name,
                dependency.root.join("target/generated-wit").display(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Error::new(
        Span2::call_site(),
        format!(
            "could not resolve the WIT for sibling component dependencies; their generated WIT is \
             not on the macro's WIT search path. Declare each sibling dependency's generated WIT \
             in `miden-project.toml` so `#[component(...)]` can resolve \
             it:\n{hints}\n\nunderlying error: {message}"
        ),
    )
}

/// Rejects a sibling reference whose generated trait would shadow the component trait itself.
///
/// The generated `pub trait <Interface>` is emitted next to the user's component trait, so a
/// reference whose interface segment equals the component trait name would produce two traits with
/// the same name in one module — a raw `E0428` far from its cause. Catch it with a clear message.
fn reject_component_trait_name_collision(
    component_trait_ident: &Ident,
    refs: &[DependencyRef],
) -> syn::Result<()> {
    if let Some(reference) = refs
        .iter()
        .find(|reference| &reference.interface_ident == component_trait_ident)
    {
        return Err(Error::new(
            reference.span,
            format!(
                "sibling reference `{}::{}` would generate a trait named `{}`, which collides \
                 with the component trait `{}` being defined; rename the component trait so it \
                 differs from its sibling interface names",
                reference.package_ident,
                reference.interface_ident,
                reference.interface_ident,
                component_trait_ident,
            ),
        ));
    }
    Ok(())
}

/// Rejects `as` aliases on sibling references.
///
/// The `as Alias` override that renames a generated trait is an `#[account(...)]`-only affordance
/// (used there to avoid name clashes); sibling component traits are always named after their
/// interface. Rejecting it loudly avoids silently ignoring a user's alias.
fn reject_aliases(refs: &[DependencyRef]) -> syn::Result<()> {
    if let Some(reference) = refs.iter().find(|reference| reference.alias.is_some()) {
        return Err(Error::new(
            reference.span,
            format!(
                "sibling component reference `{}::{}` cannot use an `as` alias; sibling traits \
                 are named after their interface",
                reference.package_ident, reference.interface_ident,
            ),
        ));
    }
    Ok(())
}

/// Rejects references whose interface segments would generate identically named Rust traits.
fn reject_duplicate_trait_names(refs: &[DependencyRef]) -> syn::Result<()> {
    for (index, reference) in refs.iter().enumerate() {
        if let Some(previous) = refs[..index]
            .iter()
            .find(|previous| previous.interface_ident == reference.interface_ident)
        {
            return Err(Error::new(
                reference.span,
                format!(
                    "sibling references `{}::{}` and `{}::{}` would both generate a trait named \
                     `{}`; sibling interface names must be unique within one component trait",
                    previous.package_ident,
                    previous.interface_ident,
                    reference.package_ident,
                    reference.interface_ident,
                    reference.interface_ident,
                ),
            ));
        }
    }
    Ok(())
}

/// Builds one generated sibling trait and its blanket attachment impl.
fn build_sibling_trait(
    reference: &DependencyRef,
    dependency: &SelectedDependency,
    module: &fpi::Module,
    hidden_module_ident: &Ident,
) -> syn::Result<TokenStream2> {
    let trait_ident = &reference.interface_ident;
    let methods = module
        .functions
        .iter()
        .map(|func| {
            build_sibling_trait_method(func, dependency, &module.module_path, hidden_module_ident)
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let trait_doc = format!(
        "Generated sibling component API for the `{}` interface of package `{}`.\n\nMethods \
         perform intra-account cross-context calls into the sibling component, which must be \
         deployed on the same account as this component.",
        dependency.import(),
        reference.package,
    );

    Ok(quote! {
        #[doc = #trait_doc]
        pub trait #trait_ident {
            #(#methods)*
        }

        // Empty attachment impl: the default method bodies above are the implementation, and
        // `NativeAccount` is implemented exactly by `#[component_storage]` structs — only code
        // acting as the transaction's native account may make intra-account sibling calls.
        impl<T: ::miden::native_account::NativeAccount> #trait_ident for T {}
    })
}

/// Builds one default trait method forwarding to a generated sibling import function.
fn build_sibling_trait_method(
    func: &ItemFn,
    dependency: &SelectedDependency,
    call_module_path: &[Ident],
    hidden_module_ident: &Ident,
) -> syn::Result<TraitItemFn> {
    // Collect the argument identifiers before touching the signature: the generated import is a
    // free function, so a receiver in it is a hard error surfaced here.
    let arg_idents = generate::collect_arg_idents(func)?;

    let mut sig = func.sig.clone();
    let retained_inputs = sig.inputs.iter().cloned().collect::<Vec<_>>();
    sig.inputs.clear();
    sig.inputs.push(parse_quote!(&self));
    sig.inputs.extend(retained_inputs);

    let mut signature_module_path = Vec::with_capacity(call_module_path.len() + 1);
    signature_module_path.push(hidden_module_ident.clone());
    signature_module_path.extend(call_module_path.iter().cloned());
    generate::qualify_signature_types(&mut sig, &signature_module_path);

    let fn_ident = &func.sig.ident;
    let mut call_path = quote!(#hidden_module_ident);
    for ident in call_module_path {
        call_path = quote!(#call_path::#ident);
    }
    let call = quote!(#call_path::#fn_ident(#(#arg_idents),*));
    let body = match &sig.output {
        ReturnType::Default => quote!({ #call; }),
        _ => quote!({ #call }),
    };

    let method_doc = format!(
        "Calls `{}` on the sibling `{}` component of the active account (cross-context call).",
        fn_ident.to_string().to_kebab_case(),
        dependency.import(),
    );

    syn::parse2(quote! {
        #[doc = #method_doc]
        #[inline(always)]
        #sig #body
    })
}

/// Builds the hidden module name holding the sibling WIT bindings for one component trait.
fn sibling_bindings_module_ident(component_trait_ident: &Ident) -> Ident {
    format_ident!("__miden_sibling_bindings_{}", component_trait_ident.to_string().to_snake_case())
}

/// Re-emits a parsed bindings file as tokens.
fn file_tokens(file: syn::File) -> TokenStream2 {
    use quote::ToTokens;
    file.into_token_stream()
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    /// Builds a generated-import lookalike for token-level method tests.
    fn import_fn(tokens: TokenStream2) -> ItemFn {
        syn::parse2(tokens).expect("test function must parse")
    }

    fn test_dependency() -> SelectedDependency {
        SelectedDependency {
            name: "pausable".to_string(),
            root: std::path::PathBuf::from("/tmp/pausable"),
            interface: crate::wit_world::DependencyInterface {
                name: "pausable".to_string(),
                import: "miden:pausable/pausable@0.1.0".to_string(),
                types: Vec::new(),
            },
        }
    }

    #[test]
    fn builds_default_method_with_receiver_and_qualified_call() {
        let func = import_fn(quote! {
            pub fn is_paused() -> bool {
                unreachable!()
            }
        });
        let hidden = format_ident!("__miden_sibling_bindings_my_component");
        let module_path =
            vec![format_ident!("miden"), format_ident!("pausable"), format_ident!("pausable")];

        let method =
            build_sibling_trait_method(&func, &test_dependency(), &module_path, &hidden).unwrap();
        let rendered = quote!(#method).to_string();

        assert!(rendered.contains("fn is_paused (& self) -> bool"), "rendered: {rendered}");
        assert!(
            rendered.contains(
                "__miden_sibling_bindings_my_component :: miden :: pausable :: pausable :: \
                 is_paused ()"
            ),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn unit_returning_method_gets_trailing_semicolon() {
        let func = import_fn(quote! {
            pub fn pause() {
                unreachable!()
            }
        });
        let hidden = format_ident!("__miden_sibling_bindings_my_component");
        let module_path = vec![format_ident!("pausable")];

        let method =
            build_sibling_trait_method(&func, &test_dependency(), &module_path, &hidden).unwrap();
        let rendered = quote!(#method).to_string();

        assert!(
            rendered.contains("pausable :: pause () ;"),
            "unit-returning default body must discard the call result: {rendered}"
        );
    }

    #[test]
    fn forwards_arguments_in_order() {
        let func = import_fn(quote! {
            pub fn set_count(key: Word, value: u64) -> u64 {
                unreachable!()
            }
        });
        let hidden = format_ident!("__miden_sibling_bindings_counter");
        let module_path = vec![format_ident!("counter")];

        let method =
            build_sibling_trait_method(&func, &test_dependency(), &module_path, &hidden).unwrap();
        let rendered = quote!(#method).to_string();

        assert!(rendered.contains("set_count (key , value)"), "rendered: {rendered}");
        // `Word` is module-local in the generated bindings, so it gets qualified.
        assert!(
            rendered.contains("key : __miden_sibling_bindings_counter :: counter :: Word"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn rejects_duplicate_generated_trait_names() {
        let args = syn::parse2::<crate::dependency_ref::DependencyRefArgs>(quote! {
            pausable::Pausable, other_pausable::Pausable
        })
        .unwrap();
        let err = reject_duplicate_trait_names(&args.refs).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("would both generate a trait named `Pausable`"));
        assert!(message.contains("pausable::Pausable"));
        // The diagnostic echoes the verbatim spelling the user wrote, not the kebab-cased
        // manifest-lookup key.
        assert!(message.contains("other_pausable::Pausable"));
    }

    #[test]
    fn hidden_module_name_derives_from_component_trait() {
        let ident = sibling_bindings_module_ident(&format_ident!("MyComponent"));
        assert_eq!(ident.to_string(), "__miden_sibling_bindings_my_component");
    }

    #[test]
    fn augments_missing_wit_package_error_with_manifest_guidance() {
        let dependency = SelectedDependency {
            name: "counter-contract".to_string(),
            root: std::path::PathBuf::from("/tmp/counter"),
            interface: crate::wit_world::DependencyInterface {
                name: "counter-contract".to_string(),
                import: "miden:counter-contract/counter-contract@0.1.0".to_string(),
                types: Vec::new(),
            },
        };
        let raw = Error::new(
            Span2::call_site(),
            "package 'miden:counter-contract@0.1.0' not found. known packages: miden:base@1.0.0",
        );

        let message =
            augment_missing_sibling_wit(raw, std::slice::from_ref(&dependency)).to_string();

        assert!(message.contains("[package.metadata.miden.dependencies]"), "message: {message}");
        assert!(message.contains("\"counter-contract\""), "message: {message}");
        assert!(message.contains("target/generated-wit"), "message: {message}");
        // The original wit-parser detail is preserved, not masked.
        assert!(message.contains("underlying error"), "message: {message}");
    }

    #[test]
    fn leaves_unrelated_generation_errors_untouched() {
        let raw = Error::new(Span2::call_site(), "some unrelated macro error");
        let augmented = augment_missing_sibling_wit(raw, std::slice::from_ref(&test_dependency()));
        assert_eq!(augmented.to_string(), "some unrelated macro error");
    }

    #[test]
    fn does_not_over_match_a_prefix_package_id() {
        // A `miden:counter` dependency must not be flagged when the error names the distinct
        // `miden:counter-contract` package, even though the former id is a prefix of the latter.
        let dependency = SelectedDependency {
            name: "counter".to_string(),
            root: std::path::PathBuf::from("/tmp/counter"),
            interface: crate::wit_world::DependencyInterface {
                name: "counter".to_string(),
                import: "miden:counter/counter@0.1.0".to_string(),
                types: Vec::new(),
            },
        };
        let raw = Error::new(
            Span2::call_site(),
            "package 'miden:counter-contract@0.1.0' not found. known packages: miden:base@1.0.0",
        );

        // No dependency matches the error's package id, so the original error passes through.
        let augmented =
            augment_missing_sibling_wit(raw, std::slice::from_ref(&dependency)).to_string();
        assert!(augmented.starts_with("package 'miden:counter-contract@0.1.0' not found"));
        assert!(!augmented.contains("[package.metadata.miden.dependencies]"));
    }

    #[test]
    fn does_not_blame_a_sibling_listed_under_known_packages() {
        // `first` resolved (it appears in the error's `known packages` list); only `second` is
        // missing. The hint must name only `second`, not the healthy `first`.
        let first = SelectedDependency {
            name: "first-counter".to_string(),
            root: std::path::PathBuf::from("/tmp/first"),
            interface: crate::wit_world::DependencyInterface {
                name: "first-counter".to_string(),
                import: "miden:first-counter/first-counter@0.1.0".to_string(),
                types: Vec::new(),
            },
        };
        let second = SelectedDependency {
            name: "second-counter".to_string(),
            root: std::path::PathBuf::from("/tmp/second"),
            interface: crate::wit_world::DependencyInterface {
                name: "second-counter".to_string(),
                import: "miden:second-counter/second-counter@0.1.0".to_string(),
                types: Vec::new(),
            },
        };
        let raw = Error::new(
            Span2::call_site(),
            "package 'miden:second-counter@0.1.0' not found. known packages: miden:base@1.0.0, \
             miden:first-counter@0.1.0",
        );

        let deps = [first, second];
        let message = augment_missing_sibling_wit(raw, &deps).to_string();
        // The hint wraps the dependency name in quotes; the resolved sibling appears only in the
        // verbatim underlying error (unquoted), so a quoted match isolates the hint.
        assert!(message.contains("\"second-counter\""), "message: {message}");
        assert!(
            !message.contains("\"first-counter\""),
            "must not blame the resolved sibling: {message}"
        );
    }
}
