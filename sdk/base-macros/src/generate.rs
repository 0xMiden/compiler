use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use heck::{ToKebabCase, ToSnakeCase, ToUpperCamelCase};
use miden_mast_package::{Package, PackageExport};
use miden_protocol::utils::serde::Deserializable;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Error, File, FnArg, ImplItem, ImplItemFn, Item, ItemFn, ItemImpl, ItemStruct,
    LitStr, Pat, ReturnType, Token, TypePath,
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
    visit_mut::VisitMut,
};
use wit_bindgen_core::{
    WorldGenerator,
    wit_parser::{
        Docs, Function, FunctionKind, InterfaceId, PackageId, Param, Resolve, Span as WitSpan,
        Type as WitType, UnresolvedPackageGroup, WorldId, WorldItem,
    },
};
use wit_bindgen_rust::{Opts, WithOption};

use crate::{
    fpi, manifest_paths,
    wit_world::{ManifestPackage, MidenDependency},
};

/// Name of the wrapper struct generated to aggregate imported interface methods.
const WRAPPER_STRUCT_NAME: &str = "Account";

#[derive(Default)]
struct GenerateArgs {
    inline: Option<LitStr>,
    /// Custom `with` entries parsed from the macro input.
    /// Each entry maps a WIT interface/type to either `generate` or a Rust path.
    /// Stored directly as `(String, WithOption)` to avoid an intermediate representation.
    with_entries: Vec<(String, WithOption)>,
}

/// Parses a single `with` entry like `"miden:foo/bar": generate` or `"miden:foo/bar": ::my::Path`.
fn parse_with_entry(input: ParseStream<'_>) -> syn::Result<(String, WithOption)> {
    let key: LitStr = input.parse()?;
    input.parse::<Token![:]>()?;
    let path: syn::Path = input.parse()?;

    // Check if the path is the special `generate` keyword
    let option = if path.leading_colon.is_none()
        && path.segments.len() == 1
        && path.segments.first().is_some_and(|seg| seg.ident == "generate")
    {
        WithOption::Generate
    } else {
        // Convert syn::Path to string, removing spaces for consistency
        let path_str = path.to_token_stream().to_string().replace(' ', "");
        WithOption::Path(path_str)
    };

    Ok((key.value(), option))
}

impl Parse for GenerateArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut args = GenerateArgs::default();

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let name = ident.to_string();
            input.parse::<Token![=]>()?;

            if name == "inline" {
                if args.inline.is_some() {
                    return Err(syn::Error::new(ident.span(), "duplicate `inline` argument"));
                }
                args.inline = Some(input.parse()?);
            } else if name == "with" {
                if !args.with_entries.is_empty() {
                    return Err(syn::Error::new(ident.span(), "duplicate `with` argument"));
                }
                let content;
                syn::braced!(content in input);
                // Parse comma-separated with entries directly into (String, WithOption) pairs
                while !content.is_empty() {
                    args.with_entries.push(parse_with_entry(&content)?);
                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    }
                }
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("unsupported generate! argument `{name}`"),
                ));
            }

            if input.peek(Token![,]) {
                let _ = input.parse::<Token![,]>()?;
            }
        }

        Ok(args)
    }
}

/// Implements the expansion logic for the `generate!` macro.
pub(crate) fn expand(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input_tokens: proc_macro2::TokenStream = input.into();
    let args = if input_tokens.is_empty() {
        GenerateArgs::default()
    } else {
        match syn::parse2::<GenerateArgs>(input_tokens) {
            Ok(parsed) => parsed,
            Err(err) => return err.to_compile_error().into(),
        }
    };

    let resolve_opts = manifest_paths::ResolveOptions {
        allow_missing_local_wit: args.inline.is_some(),
    };

    match manifest_paths::resolve_wit_paths(resolve_opts) {
        Ok(config) => {
            if config.paths.is_empty() {
                return Error::new(
                    Span::call_site(),
                    "no WIT dependencies declared under \
                     [package.metadata.component.target.dependencies]",
                )
                .to_compile_error()
                .into();
            }

            let inline_world = args
                .inline
                .as_ref()
                .and_then(|src| manifest_paths::extract_world_name(&src.value()));
            let world_value = inline_world.or_else(|| config.world.clone());

            if args.inline.is_some() && world_value.is_none() {
                return Error::new(
                    Span::call_site(),
                    "failed to detect world name for inline WIT provided to generate!",
                )
                .to_compile_error()
                .into();
            }

            match generate_bindings(&args, &config, world_value.as_deref()) {
                Ok(raw_bindings) => match augment_generated_bindings(raw_bindings) {
                    Ok(augmented) => {
                        quote! {
                            // Wrap the bindings in the `bindings` module since `generate!` makes a top level
                            // module named after the package namespace which is `miden` for all our projects
                            // so it conflicts with the `miden` crate (SDK)
                            #[doc(hidden)]
                            #[allow(dead_code)]
                            pub mod bindings {
                                #augmented
                            }
                        }
                        .into()
                    }
                    Err(err) => err.to_compile_error().into(),
                },
                Err(err) => err.to_compile_error().into(),
            }
        }
        Err(err) => err.to_compile_error().into(),
    }
}

/// Generates WIT bindings using `wit-bindgen` directly instead of the `generate!` macro.
///
/// The `world` parameter specifies which world to generate bindings for. This should already
/// be resolved by the caller (either from inline WIT or from the local wit/ directory).
/// If `None`, wit-bindgen will attempt to select a default world from the loaded packages.
fn generate_bindings(
    args: &GenerateArgs,
    config: &manifest_paths::ResolvedWit,
    world: Option<&str>,
) -> Result<TokenStream2, Error> {
    let inline_src = args.inline.as_ref().map(|src| src.value());
    let inline_ref = inline_src.as_deref();
    let mut wit_sources = load_wit_sources(&config.paths, inline_ref)?;

    let world_id = wit_sources
        .resolve
        .select_world(&wit_sources.packages, world)
        .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;
    let fpi_imports = ManifestPackage::load(Span::call_site())?
        .collect_miden_dependency_imports(Span::call_site())?;
    inject_fpi_imports(&mut wit_sources.resolve, world_id, &fpi_imports)?;

    let mut opts = Opts {
        generate_all: true,
        runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
        default_bindings_module: Some("bindings".to_string()),
        ..Opts::default()
    };
    push_custom_with_entries(&mut opts, &args.with_entries);
    push_default_with_entries(&mut opts);

    let mut generated_files = wit_bindgen_core::Files::default();
    let mut generator = opts.build();
    generator
        .generate(&mut wit_sources.resolve, world_id, &mut generated_files)
        .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;

    let (_, src_bytes) = generated_files
        .iter()
        .next()
        .ok_or_else(|| Error::new(Span::call_site(), "wit-bindgen emitted no bindings"))?;
    let src = std::str::from_utf8(src_bytes)
        .map_err(|err| Error::new(Span::call_site(), format!("invalid UTF-8: {err}")))?;
    let mut tokens: TokenStream2 = src
        .parse()
        .map_err(|err| Error::new(Span::call_site(), format!("failed to parse bindings: {err}")))?;

    // Include a dummy `include_bytes!` for any files we read so rustc knows that
    // we depend on the contents of those files.
    for path in wit_sources.files_read {
        let utf8_path = path.to_str().ok_or_else(|| {
            Error::new(
                Span::call_site(),
                format!("path '{}' contains invalid UTF-8", path.display()),
            )
        })?;
        tokens.extend(quote! {
            const _: &[u8] = include_bytes!(#utf8_path);
        });
    }

    Ok(tokens)
}

/// Post-processes wit-bindgen output to inject wrapper structs for imported interfaces.
///
/// This transforms the raw bindings by walking all modules and injecting an `Account` wrapper
/// struct at the bindings root level. The struct has methods that delegate to the generated
/// free functions in leaf modules. This provides a more ergonomic API
/// (e.g., `Account::default().receive_asset(asset)` instead of
/// `miden::basic_wallet::basic_wallet::receive_asset(asset)`).
///
/// ## Leaf Module Selection
///
/// Only "leaf" modules (those containing no nested modules) contribute methods to the
/// wrapper struct. This is because wit-bindgen generates a module hierarchy where:
/// - Non-leaf modules represent WIT package namespaces (e.g., `miden::basic_wallet`)
/// - Leaf modules represent actual WIT interfaces with callable functions
///
/// For example, given the module structure:
/// ```text
/// miden/
///   basic_wallet/
///     basic_wallet/     <- leaf module, methods collected here
///       receive_asset()
///       send_asset()
/// ```
/// Only functions from `miden::basic_wallet::basic_wallet` are wrapped.
fn augment_generated_bindings(tokens: TokenStream2) -> syn::Result<TokenStream2> {
    let mut file: File = syn::parse2(tokens)?;
    let mut collected_methods = Vec::new();
    collect_wrapper_methods(&file.items, &mut Vec::new(), &mut collected_methods)?;
    let mut fpi_modules = Vec::new();
    collect_fpi_modules(&file.items, &mut Vec::new(), &mut fpi_modules)?;

    // Check for method name collisions across different interfaces
    check_method_name_collisions(&collected_methods)?;
    check_fpi_struct_name_collisions(&fpi_modules)?;

    if !collected_methods.is_empty() {
        let struct_ident = syn::Ident::new(WRAPPER_STRUCT_NAME, Span::call_site());
        let struct_item: ItemStruct = parse_quote! {
            /// Wrapper struct that contains all the methods from all the account component
            /// dependencies of this script. Each account component dependency method is "merged"
            /// into this struct.
            #[derive(Default)]
            pub struct #struct_ident;
        };

        let mut impl_item: ItemImpl = parse_quote! {
            impl #struct_ident {}
        };
        impl_item
            .items
            .extend(collected_methods.into_iter().map(|cm| ImplItem::Fn(cm.method)));

        file.items.push(Item::Struct(struct_item));
        file.items.push(Item::Impl(impl_item));
    }

    if !fpi_modules.is_empty() {
        let dependencies = load_fpi_dependencies(Span::call_site())?;
        let mut include_paths = Vec::new();
        for module in fpi_modules {
            let Some(dependency) = dependencies
                .iter()
                .find(|dependency| dependency.module_path == module.path_string)
            else {
                return Err(Error::new(
                    Span::call_site(),
                    format!(
                        "failed to resolve FPI dependency metadata for generated module `{}`",
                        module.path_string
                    ),
                ));
            };

            include_paths.push(dependency.package_path.clone());
            let (struct_item, impl_item) = build_fpi_struct(&module, dependency)?;
            file.items.push(Item::Struct(struct_item));
            file.items.push(Item::Impl(impl_item));
        }

        for path in include_paths {
            let utf8_path = path.to_str().ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!("path '{}' contains invalid UTF-8", path.display()),
                )
            })?;
            file.items.push(parse_quote! {
                const _: &[u8] = include_bytes!(#utf8_path);
            });
        }
    }

    Ok(file.into_token_stream())
}

/// Result of loading and parsing WIT sources from file paths and optional inline content.
struct LoadedWitSources {
    /// The resolved WIT definitions containing all types, interfaces, and worlds.
    resolve: Resolve,
    /// Package IDs to use for world selection. When inline source is provided, this contains
    /// only the inline package; otherwise it contains all packages from file paths.
    packages: Vec<PackageId>,
    /// File paths that were read during WIT parsing. Used to generate dummy `include_bytes!`
    /// calls so rustc knows to recompile when these files change.
    files_read: Vec<PathBuf>,
}

/// Loads WIT sources from file paths and optionally an inline source.
fn load_wit_sources(
    paths: &[String],
    inline_source: Option<&str>,
) -> Result<LoadedWitSources, Error> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").map_err(|err| {
        Error::new(Span::call_site(), format!("failed to read CARGO_MANIFEST_DIR: {err}"))
    })?;
    let manifest_dir = PathBuf::from(manifest_dir);

    let mut resolve = Resolve::default();
    let mut packages = Vec::new();
    let mut files = Vec::new();

    // Load WIT definitions from file paths. These are always loaded to populate the resolver
    // with type definitions that the inline source may depend on.
    for path in paths {
        let path_buf = PathBuf::from(path);
        let absolute = if path_buf.is_absolute() {
            path_buf
        } else {
            manifest_dir.join(path_buf)
        };
        let normalized = fs::canonicalize(&absolute).unwrap_or(absolute);
        let (pkg, sources) = resolve.push_path(normalized.clone()).map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to load WIT from '{}': {err}", normalized.display()),
            )
        })?;
        packages.push(pkg);
        files.extend(sources.paths().map(|p| p.to_owned()));
    }

    if let Some(src) = inline_source {
        // When inline source is provided, it becomes the primary package for world selection.
        // We clear previously collected package IDs because the inline source defines the world
        // we want to generate bindings for. The file-based packages are still loaded above and
        // remain in the resolver - they provide type definitions that the inline world imports.
        packages.clear();
        let group = UnresolvedPackageGroup::parse("inline", src)
            .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;
        let pkg = resolve
            .push_group(group)
            .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;
        packages.push(pkg);
    }

    Ok(LoadedWitSources {
        resolve,
        packages,
        files_read: files,
    })
}

/// Core WIT types needed to synthesize caller-only FPI imports.
#[derive(Clone, Copy)]
struct FpiCoreTypes {
    felt: WitType,
    word: WitType,
}

/// Adds `fpi-` functions to imported Miden dependency interfaces in the selected world.
fn inject_fpi_imports(
    resolve: &mut Resolve,
    world_id: WorldId,
    dependency_imports: &[String],
) -> syn::Result<()> {
    if dependency_imports.is_empty() {
        return Ok(());
    }

    let dependency_imports = dependency_imports.iter().map(String::as_str).collect::<HashSet<_>>();
    let imported_interfaces = resolve.worlds[world_id]
        .imports
        .values()
        .filter_map(|item| match item {
            WorldItem::Interface { id, .. } => Some(*id),
            _ => None,
        })
        .filter(|id| {
            interface_import_path(resolve, *id)
                .as_ref()
                .is_some_and(|path| dependency_imports.contains(path.as_str()))
        })
        .collect::<Vec<_>>();

    if imported_interfaces.is_empty() {
        return Ok(());
    }

    let core_types = resolve_fpi_core_types(resolve)?;
    for interface_id in imported_interfaces {
        inject_fpi_functions_into_interface(resolve, interface_id, core_types);
    }

    Ok(())
}

/// Returns the fully-qualified import path used for a resolved interface.
fn interface_import_path(resolve: &Resolve, interface_id: InterfaceId) -> Option<String> {
    let interface = &resolve.interfaces[interface_id];
    let interface_name = interface.name.as_deref()?;
    let package_id = interface.package?;
    Some(resolve.packages[package_id].name.interface_id(interface_name))
}

/// Resolves `felt` and `word` from `miden:base/core-types`.
fn resolve_fpi_core_types(resolve: &Resolve) -> syn::Result<FpiCoreTypes> {
    let core_types = resolve
        .packages
        .iter()
        .find_map(|(_, package)| {
            if package.name.namespace != "miden" || package.name.name != "base" {
                return None;
            }

            package.interfaces.get("core-types").map(|interface_id| {
                let interface = &resolve.interfaces[*interface_id];
                (interface.types.get("felt").copied(), interface.types.get("word").copied())
            })
        })
        .ok_or_else(|| {
            Error::new(
                Span::call_site(),
                "failed to resolve miden:base/core-types package for FPI imports",
            )
        })?;

    let (Some(felt), Some(word)) = core_types else {
        return Err(Error::new(
            Span::call_site(),
            "miden:base/core-types is missing felt or word type definitions",
        ));
    };

    Ok(FpiCoreTypes {
        felt: WitType::Id(felt),
        word: WitType::Id(word),
    })
}

/// Injects FPI variants of every freestanding function in an imported interface.
fn inject_fpi_functions_into_interface(
    resolve: &mut Resolve,
    interface_id: InterfaceId,
    core_types: FpiCoreTypes,
) {
    let interface = &mut resolve.interfaces[interface_id];
    let functions = interface
        .functions
        .values()
        .filter(|function| {
            matches!(function.kind, FunctionKind::Freestanding)
                && !function.name.starts_with(fpi::WIT_FUNCTION_PREFIX)
        })
        .cloned()
        .collect::<Vec<_>>();

    for function in functions {
        let fpi_name = format!("{}{}", fpi::WIT_FUNCTION_PREFIX, function.name);
        if interface.functions.contains_key(&fpi_name) {
            continue;
        }

        interface
            .functions
            .insert(fpi_name.clone(), build_fpi_import_function(function, fpi_name, core_types));
    }
}

/// Builds the caller-side WIT import that forwards through `execute_foreign_procedure`.
fn build_fpi_import_function(
    function: Function,
    fpi_name: String,
    core_types: FpiCoreTypes,
) -> Function {
    let mut params = Vec::with_capacity(function.params.len() + 3);
    params.push(Param {
        name: "account-id-prefix".to_string(),
        ty: core_types.felt,
        span: WitSpan::default(),
    });
    params.push(Param {
        name: "account-id-suffix".to_string(),
        ty: core_types.felt,
        span: WitSpan::default(),
    });
    params.push(Param {
        name: "foreign-proc-root".to_string(),
        ty: core_types.word,
        span: WitSpan::default(),
    });
    params.extend(function.params);

    Function {
        name: fpi_name,
        kind: FunctionKind::Freestanding,
        params,
        result: function.result,
        docs: Docs::default(),
        stability: function.stability,
        span: WitSpan::default(),
    }
}

/// Pushes user-provided `with` entries to the wit-bindgen options.
fn push_custom_with_entries(opts: &mut Opts, entries: &[(String, WithOption)]) {
    opts.with.extend(entries.iter().cloned());
}

/// Pushes default `with` entries that map Miden base types to SDK types.
fn push_default_with_entries(opts: &mut Opts) {
    opts.with
        .push(("miden:base/core-types@1.0.0".to_string(), WithOption::Generate));
    push_path_entry(opts, "miden:base/core-types@1.0.0/felt", "::miden::Felt");
    push_path_entry(opts, "miden:base/core-types@1.0.0/word", "::miden::Word");
    push_path_entry(opts, "miden:base/core-types@1.0.0/asset", "::miden::Asset");
    push_path_entry(opts, "miden:base/core-types@1.0.0/account-id", "::miden::AccountId");
    push_path_entry(opts, "miden:base/core-types@1.0.0/tag", "::miden::Tag");
    push_path_entry(opts, "miden:base/core-types@1.0.0/note-type", "::miden::NoteType");
    push_path_entry(opts, "miden:base/core-types@1.0.0/recipient", "::miden::Recipient");
    push_path_entry(opts, "miden:base/core-types@1.0.0/note-idx", "::miden::NoteIdx");
}

fn push_path_entry(opts: &mut Opts, key: &str, value: &str) {
    opts.with.push((key.to_string(), WithOption::Path(value.to_string())));
}

/// A collected wrapper method along with its source module path.
struct CollectedMethod {
    method: ImplItemFn,
    /// The module path where this method originated (e.g., "miden::basic_wallet::basic_wallet").
    source_path: String,
}

/// FPI-capable generated import module.
struct FpiModule {
    /// Rust module path where the generated `fpi_*` free functions live.
    module_path: Vec<syn::Ident>,
    /// String form of `module_path` used for dependency lookup.
    path_string: String,
    /// Generated FPI free functions from this import module.
    functions: Vec<ItemFn>,
}

/// Resolved package data needed to generate FPI caller wrappers.
struct FpiDependency {
    /// Rust module path generated by wit-bindgen for the dependency import.
    module_path: String,
    /// Miden package artifact read for procedure roots.
    package_path: PathBuf,
    /// Fully-qualified WIT import path.
    import: String,
    /// Procedure roots keyed by WIT function name.
    roots: HashMap<String, ProcedureRoot>,
}

/// Four field elements that make up a foreign procedure root.
#[derive(Clone, Copy)]
struct ProcedureRoot {
    felts: [u64; 4],
}

/// Recursively walks all modules and collects wrapper methods from leaf modules.
///
/// The `path` parameter tracks the current module path for generating correct call paths.
/// Collected methods are appended to `methods_out` and will be placed in the root `Account` struct.
fn collect_wrapper_methods(
    items: &[Item],
    path: &mut Vec<syn::Ident>,
    methods_out: &mut Vec<CollectedMethod>,
) -> syn::Result<()> {
    for item in items.iter() {
        if let Item::Mod(module) = item {
            path.push(module.ident.clone());
            if let Some((_, ref content)) = module.content {
                collect_wrapper_methods(content, path, methods_out)?;
                collect_methods_from_module(content, path, methods_out)?;
            }
            path.pop();
        }
    }

    Ok(())
}

/// Recursively walks all modules and collects FPI functions from leaf import modules.
fn collect_fpi_modules(
    items: &[Item],
    path: &mut Vec<syn::Ident>,
    modules_out: &mut Vec<FpiModule>,
) -> syn::Result<()> {
    for item in items.iter() {
        if let Item::Mod(module) = item {
            path.push(module.ident.clone());
            if let Some((_, ref content)) = module.content {
                collect_fpi_modules(content, path, modules_out)?;
                collect_fpi_functions_from_module(content, path, modules_out);
            }
            path.pop();
        }
    }

    Ok(())
}

/// Collects generated `fpi_*` free functions from a leaf import module.
fn collect_fpi_functions_from_module(
    items: &[Item],
    path: &[syn::Ident],
    modules_out: &mut Vec<FpiModule>,
) {
    if !should_generate_struct(path, items) {
        return;
    }

    let functions = items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(func) if is_fpi_function(func) => Some(func.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if functions.is_empty() {
        return;
    }

    modules_out.push(FpiModule {
        module_path: path.to_vec(),
        path_string: format_module_path(path),
        functions,
    });
}

/// Collects wrapper methods from a leaf module's public functions.
///
/// A leaf module is one that contains no nested modules. Only leaf modules contribute
/// methods, as non-leaf modules typically represent namespace hierarchy rather than
/// concrete interfaces.
fn collect_methods_from_module(
    items: &[Item],
    path: &[syn::Ident],
    methods_out: &mut Vec<CollectedMethod>,
) -> syn::Result<()> {
    if !should_generate_struct(path, items) {
        return Ok(());
    }

    let functions: Vec<&ItemFn> = items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(func) if is_target_function(func) && !is_fpi_function(func) => Some(func),
            _ => None,
        })
        .collect();

    let source_path = format_module_path(path);
    for func in functions {
        methods_out.push(CollectedMethod {
            method: build_wrapper_method(func, path)?,
            source_path: source_path.clone(),
        });
    }

    Ok(())
}

/// Builds a wrapper method that delegates to the original free function.
///
/// Type paths in the signature are qualified with the module path prefix so they
/// resolve correctly when the method is placed at the bindings root level.
fn build_wrapper_method(func: &ItemFn, module_path: &[syn::Ident]) -> syn::Result<ImplItemFn> {
    let mut sig = func.sig.clone();
    // Make every method `&mut self` as a temporary workaround until
    // https://github.com/0xMiden/compiler/issues/802 is resolved
    sig.inputs.insert(0, parse_quote!(&mut self));

    // Qualify type paths in the signature so they resolve from the bindings root
    qualify_signature_types(&mut sig, module_path);

    let arg_idents = collect_arg_idents(func)?;
    let call_expr = wrapper_call_tokens(module_path, &sig.ident, &arg_idents);

    let method_doc = format!("Calls `{}` from `{}`.", sig.ident, format_module_path(module_path));
    let doc_attr: Attribute = parse_quote!(#[doc = #method_doc]);
    let inline_attr: Attribute = parse_quote!(#[inline(always)]);

    let body_tokens = match &sig.output {
        ReturnType::Default => quote!({ #call_expr; }),
        _ => quote!({ #call_expr }),
    };
    let block = syn::parse2(body_tokens)?;

    Ok(ImplItemFn {
        attrs: vec![doc_attr, inline_attr],
        vis: func.vis.clone(),
        defaultness: None,
        sig,
        block,
    })
}

/// Builds a `CounterContract`-style FPI caller wrapper for a generated import module.
fn build_fpi_struct(
    module: &FpiModule,
    dependency: &FpiDependency,
) -> syn::Result<(ItemStruct, ItemImpl)> {
    let struct_ident = fpi_struct_ident(module)?;
    let struct_doc = format!("FPI caller for procedures imported from `{}`.", dependency.import);
    let struct_item: ItemStruct = parse_quote! {
        #[doc = #struct_doc]
        #[derive(Clone, Copy, Debug)]
        pub struct #struct_ident(::miden::AccountId);
    };

    let mut impl_item: ItemImpl = parse_quote! {
        impl #struct_ident {
            /// Creates a caller bound to the given foreign account.
            #[inline(always)]
            pub fn from_account(account_id: ::miden::AccountId) -> Self {
                Self(account_id)
            }
        }
    };

    for func in &module.functions {
        let wit_name = fpi_function_wit_name(func)?;
        let root = dependency.roots.get(&wit_name).ok_or_else(|| {
            Error::new(
                func.sig.ident.span(),
                format!(
                    "failed to find procedure root for `{wit_name}` in package '{}'",
                    dependency.package_path.display()
                ),
            )
        })?;
        impl_item.items.push(ImplItem::Fn(build_fpi_wrapper_method(
            func,
            &module.module_path,
            *root,
        )?));
    }

    Ok((struct_item, impl_item))
}

/// Builds a method on an FPI caller wrapper that delegates to the generated `fpi_*` import.
fn build_fpi_wrapper_method(
    func: &ItemFn,
    module_path: &[syn::Ident],
    procedure_root: ProcedureRoot,
) -> syn::Result<ImplItemFn> {
    let original_fn_ident = func.sig.ident.clone();
    let method_ident = fpi_method_ident(func)?;
    let mut sig = func.sig.clone();
    sig.ident = method_ident;

    let retained_inputs = sig.inputs.iter().skip(3).cloned().collect::<Vec<_>>();
    if sig.inputs.len() < 3 {
        return Err(Error::new(
            sig.ident.span(),
            "generated FPI function is missing account id and procedure root parameters",
        ));
    }

    sig.inputs.clear();
    sig.inputs.push(parse_quote!(&self));
    sig.inputs.extend(retained_inputs);
    qualify_signature_types(&mut sig, module_path);

    let arg_idents = collect_arg_idents(func)?.into_iter().skip(3).collect::<Vec<_>>();
    let root_tokens = procedure_root_tokens(procedure_root);
    let call_expr =
        fpi_wrapper_call_tokens(module_path, &original_fn_ident, root_tokens, &arg_idents);

    let method_doc = format!(
        "Invokes `{}` through `execute_foreign_procedure`.",
        sig.ident.to_string().to_kebab_case()
    );
    let doc_attr: Attribute = parse_quote!(#[doc = #method_doc]);
    let inline_attr: Attribute = parse_quote!(#[inline(always)]);

    let body_tokens = match &sig.output {
        ReturnType::Default => quote!({ #call_expr; }),
        _ => quote!({ #call_expr }),
    };
    let block = syn::parse2(body_tokens)?;

    Ok(ImplItemFn {
        attrs: vec![doc_attr, inline_attr],
        vis: func.vis.clone(),
        defaultness: None,
        sig,
        block,
    })
}

/// Qualifies type paths in a function signature with the module path prefix.
///
/// This transforms simple type names (e.g., `StructA`) into fully qualified paths
/// (e.g., `miden::component::component::StructA`) so they resolve correctly when
/// the method is placed at the bindings root level.
fn qualify_signature_types(sig: &mut syn::Signature, module_path: &[syn::Ident]) {
    struct TypeQualifier<'a> {
        module_path: &'a [syn::Ident],
    }

    impl VisitMut for TypeQualifier<'_> {
        fn visit_type_path_mut(&mut self, type_path: &mut TypePath) {
            // Only qualify paths that:
            // 1. Don't already have a leading colon (not absolute like `::foo`)
            // 2. Are simple single-segment paths (like `StructA`, not `foo::Bar`)
            // 3. Don't start with common primitive/std type names
            if type_path.qself.is_none()
                && type_path.path.leading_colon.is_none()
                && type_path.path.segments.len() == 1
            {
                let first_segment = &type_path.path.segments[0].ident;
                let name = first_segment.to_string();

                // Skip primitive types and common std types
                if is_primitive_or_std_type(&name) {
                    return;
                }

                // Build the qualified path: module_path::TypeName
                let mut new_segments = syn::punctuated::Punctuated::new();
                for ident in self.module_path {
                    new_segments.push(syn::PathSegment {
                        ident: ident.clone(),
                        arguments: syn::PathArguments::None,
                    });
                }
                // Add the original type segment (preserving generics)
                new_segments.push(type_path.path.segments[0].clone());

                type_path.path.segments = new_segments;
            }

            // Continue visiting nested types (e.g., generics)
            syn::visit_mut::visit_type_path_mut(self, type_path);
        }
    }

    let mut qualifier = TypeQualifier { module_path };
    qualifier.visit_signature_mut(sig);
}

/// Returns true if the name is a primitive type or common std type that shouldn't be qualified.
///
/// This list covers Rust primitives and common standard library types. WIT-generated bindings
/// only use a subset of these (primitives, String, Vec, Option, Result), but we include
/// additional common types for safety. Types like `Rc`, `Arc`, `RefCell` are not used by
/// wit-bindgen and are intentionally omitted.
fn is_primitive_or_std_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "String"
            | "Vec"
            | "Option"
            | "Result"
            | "Self"
    )
}

/// Extracts argument identifiers from a function signature.
///
/// Returns an error if the function contains a receiver (`self`) or uses
/// unsupported argument patterns (e.g., destructuring patterns).
fn collect_arg_idents(func: &ItemFn) -> syn::Result<Vec<syn::Ident>> {
    func.sig
        .inputs
        .iter()
        .map(|arg| match arg {
            FnArg::Receiver(_) => {
                Err(Error::new(func.sig.ident.span(), "unexpected receiver in generated function"))
            }
            FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
                Pat::Ident(pat_ident) => Ok(pat_ident.ident.clone()),
                other => Err(Error::new(
                    other.span(),
                    format!(
                        "unsupported argument pattern `{}` in generated function",
                        quote!(#other)
                    ),
                )),
            },
        })
        .collect()
}

/// Generates tokens for calling the original free function from the wrapper method.
fn wrapper_call_tokens(
    module_path: &[syn::Ident],
    fn_ident: &syn::Ident,
    args: &[syn::Ident],
) -> TokenStream2 {
    let mut path_tokens = quote! { crate::bindings };
    for ident in module_path {
        path_tokens = quote! { #path_tokens :: #ident };
    }

    quote! { #path_tokens :: #fn_ident(#(#args),*) }
}

/// Generates tokens for calling a generated FPI free function from its typed wrapper.
fn fpi_wrapper_call_tokens(
    module_path: &[syn::Ident],
    fn_ident: &syn::Ident,
    procedure_root: TokenStream2,
    args: &[syn::Ident],
) -> TokenStream2 {
    let mut path_tokens = quote! { crate::bindings };
    for ident in module_path {
        path_tokens = quote! { #path_tokens :: #ident };
    }

    quote! {
        #path_tokens :: #fn_ident(self.0.prefix, self.0.suffix, #procedure_root, #(#args),*)
    }
}

/// Returns the user-facing struct identifier for a generated FPI import module.
fn fpi_struct_ident(module: &FpiModule) -> syn::Result<syn::Ident> {
    let Some(last) = module.module_path.last() else {
        return Err(Error::new(Span::call_site(), "empty FPI module path"));
    };
    Ok(syn::Ident::new(&last.to_string().to_upper_camel_case(), last.span()))
}

/// Returns the wrapper method name for a generated FPI free function.
fn fpi_method_ident(func: &ItemFn) -> syn::Result<syn::Ident> {
    let fn_name = func.sig.ident.to_string();
    let Some(method_name) = fn_name.strip_prefix(fpi::RUST_FUNCTION_PREFIX) else {
        return Err(Error::new(
            func.sig.ident.span(),
            format!(
                "expected generated FPI function name to start with `{}`",
                fpi::RUST_FUNCTION_PREFIX
            ),
        ));
    };
    Ok(syn::Ident::new(method_name, func.sig.ident.span()))
}

/// Returns the original WIT function name represented by a generated FPI free function.
fn fpi_function_wit_name(func: &ItemFn) -> syn::Result<String> {
    Ok(fpi_method_ident(func)?.to_string().to_kebab_case())
}

/// Converts a procedure root into SDK `Word` construction tokens.
fn procedure_root_tokens(root: ProcedureRoot) -> TokenStream2 {
    let felts = root.felts.into_iter().map(|value| quote!(::miden::felt!(#value)));
    quote!(::miden::Word::new([#(#felts),*]))
}

/// Loads all Miden dependency packages needed by generated FPI wrappers.
fn load_fpi_dependencies(error_span: Span) -> syn::Result<Vec<FpiDependency>> {
    let manifest = ManifestPackage::load(error_span)?;
    manifest
        .collect_miden_dependencies(error_span)?
        .into_iter()
        .map(load_fpi_dependency)
        .collect()
}

/// Loads a single dependency package and extracts exported procedure roots.
fn load_fpi_dependency(dependency: MidenDependency) -> syn::Result<FpiDependency> {
    let module_path = import_module_path(&dependency.import);
    let package_path = resolve_dependency_package_path(&dependency)?;
    let package_bytes = fs::read(&package_path).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to read dependency package '{}': {err}", package_path.display()),
        )
    })?;
    let package = Package::read_from_bytes(&package_bytes).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to deserialize dependency package '{}': {err}", package_path.display()),
        )
    })?;

    let mut roots = HashMap::new();
    for export in package.manifest.exports() {
        let PackageExport::Procedure(proc_export) = export else {
            continue;
        };
        let Some(function_name) = proc_export.path.last() else {
            continue;
        };
        if !procedure_path_matches_import(proc_export.path.as_ref().as_str(), &dependency.import) {
            continue;
        }
        roots.insert(function_name.to_string(), procedure_root_from_digest(&proc_export.digest));
    }

    Ok(FpiDependency {
        module_path,
        package_path,
        import: dependency.import,
        roots,
    })
}

/// Converts a fully-qualified WIT import path into the Rust module path generated by wit-bindgen.
fn import_module_path(import: &str) -> String {
    let without_version = import.split('@').next().unwrap_or(import);
    without_version
        .split([':', '/'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_snake_case())
        .collect::<Vec<_>>()
        .join("::")
}

/// Finds the `.masp` package artifact corresponding to a manifest dependency entry.
fn resolve_dependency_package_path(dependency: &MidenDependency) -> syn::Result<PathBuf> {
    if dependency.root.is_file() {
        return Ok(dependency.root.clone());
    }

    let preferred_profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let mut profiles = vec![preferred_profile.clone()];
    if preferred_profile != "release" {
        profiles.push("release".to_string());
    }
    if preferred_profile != "debug" {
        profiles.push("debug".to_string());
    }

    let package_stems = dependency_package_stems(dependency);
    let output_dirs = dependency_output_dirs(dependency, &profiles);
    for dir in &output_dirs {
        if let Some(package) = find_dependency_package_in_dir(dir, &package_stems)? {
            return Ok(package.clone());
        }
    }

    let searched = output_dirs
        .iter()
        .map(|dir| format!("'{}'", dir.display()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(Error::new(
        Span::call_site(),
        format!(
            "failed to find built `.masp` artifact for dependency '{}'; searched {searched}; \
             build the dependency with `cargo miden build` first",
            dependency.name,
        ),
    ))
}

/// Returns candidate output directories where a dependency `.masp` may have been written.
fn dependency_output_dirs(dependency: &MidenDependency, profiles: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_profile_dirs(&mut dirs, dependency.root.join("target"), profiles);

    if let Ok(target_dir) = env::var("CARGO_TARGET_DIR") {
        push_profile_dirs(&mut dirs, PathBuf::from(target_dir), profiles);
    }

    if let Ok(out_dir) = env::var("OUT_DIR") {
        for ancestor in Path::new(&out_dir).ancestors() {
            push_profile_dirs(&mut dirs, ancestor.to_path_buf(), profiles);
        }
    }

    dirs
}

/// Adds `target/miden/<profile>` directories while preserving insertion order.
fn push_profile_dirs(dirs: &mut Vec<PathBuf>, target_root: PathBuf, profiles: &[String]) {
    for profile in profiles {
        let dir = target_root.join("miden").join(profile);
        if !dirs.iter().any(|existing| existing == &dir) {
            dirs.push(dir);
        }
    }
}

/// Finds a dependency package in `dir`, preferring filenames that match the package name.
fn find_dependency_package_in_dir(
    dir: &Path,
    package_stems: &[String],
) -> syn::Result<Option<PathBuf>> {
    if !dir.is_dir() {
        return Ok(None);
    }

    let mut packages = fs::read_dir(dir)
        .map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to read dependency output directory '{}': {err}", dir.display()),
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to iterate dependency output directory '{}': {err}", dir.display()),
            )
        })?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "masp"))
        .collect::<Vec<_>>();
    packages.sort();

    for stem in package_stems {
        if let Some(package) = packages.iter().find(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|file_stem| file_stem == stem)
        }) {
            return Ok(Some(package.clone()));
        }
    }

    Ok((packages.len() == 1).then(|| packages[0].clone()))
}

/// Returns likely `.masp` filename stems for a dependency.
fn dependency_package_stems(dependency: &MidenDependency) -> Vec<String> {
    let mut stems = Vec::new();

    if let Some(package_name) = dependency_manifest_package_name(&dependency.root) {
        push_dependency_stem(&mut stems, &package_name);
    }

    if let Some(name) = dependency.name.split([':', '/']).next_back() {
        push_dependency_stem(&mut stems, name);
    }

    if let Some(name) = dependency.root.file_name().and_then(|name| name.to_str()) {
        push_dependency_stem(&mut stems, name);
    }

    stems
}

/// Reads the Cargo package name for dependency directories.
fn dependency_manifest_package_name(root: &Path) -> Option<String> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(manifest_path).ok()?;
    let manifest = manifest.parse::<toml::Table>().ok()?;
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Adds a normalized Miden package stem if it has not already been added.
fn push_dependency_stem(stems: &mut Vec<String>, name: &str) {
    let stem = name.replace('-', "_");
    if !stem.is_empty() && !stems.iter().any(|existing| existing == &stem) {
        stems.push(stem);
    }
}

/// Returns true when an exported procedure belongs to the dependency import interface.
fn procedure_path_matches_import(path: &str, import: &str) -> bool {
    path.contains(import)
}

/// Converts a MAST digest word into literal field elements.
fn procedure_root_from_digest(digest: &miden_protocol::Word) -> ProcedureRoot {
    let elements = digest.as_elements();
    ProcedureRoot {
        felts: [
            elements[0].as_canonical_u64(),
            elements[1].as_canonical_u64(),
            elements[2].as_canonical_u64(),
            elements[3].as_canonical_u64(),
        ],
    }
}

/// Determines whether a wrapper struct should be generated for the given module.
///
/// Returns `false` for:
/// - Empty paths
/// - `exports` modules (these are user-implemented exports, not imports)
/// - Modules starting with underscore (internal/private modules)
/// - Non-leaf modules (modules that contain nested modules)
fn should_generate_struct(path: &[syn::Ident], items: &[Item]) -> bool {
    if path.is_empty() {
        return false;
    }
    let first = path[0].to_string();
    if first == "exports" {
        return false;
    }
    if first.starts_with('_') {
        return false;
    }
    let last = path.last().unwrap().to_string();
    if last.starts_with('_') {
        return false;
    }
    // Only generate for leaf modules (no nested modules)
    !items.iter().any(|item| matches!(item, Item::Mod(_)))
}

/// Determines whether a function should have a wrapper method generated.
///
/// Returns `true` for public, safe functions that don't start with underscore.
fn is_target_function(func: &ItemFn) -> bool {
    matches!(func.vis, syn::Visibility::Public(_))
        && func.sig.unsafety.is_none()
        && !func.sig.ident.to_string().starts_with('_')
}

/// Determines whether a generated free function represents an FPI WIT import.
fn is_fpi_function(func: &ItemFn) -> bool {
    matches!(func.vis, syn::Visibility::Public(_))
        && func.sig.unsafety.is_none()
        && func.sig.ident.to_string().starts_with(fpi::RUST_FUNCTION_PREFIX)
}

/// Formats a module path as a `::` separated string for use in documentation.
fn format_module_path(path: &[syn::Ident]) -> String {
    path.iter().map(|ident| ident.to_string()).collect::<Vec<_>>().join("::")
}

/// Checks for method name collisions across collected wrapper methods.
///
/// If multiple imported interfaces define functions with the same name, they would all be
/// added to the `Account` struct, causing a compilation error. This function detects such
/// collisions early and provides a clear error message indicating which interfaces conflict.
fn check_method_name_collisions(methods: &[CollectedMethod]) -> syn::Result<()> {
    let mut seen: HashMap<String, &str> = HashMap::new();

    for collected in methods {
        let method_name = collected.method.sig.ident.to_string();

        if let Some(existing_path) = seen.get(&method_name) {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "method name collision in generated `{WRAPPER_STRUCT_NAME}` struct: \
                     `{method_name}` is defined in both `{existing_path}` and `{}`. Consider \
                     using the original module paths directly instead of the wrapper struct.",
                    collected.source_path
                ),
            ));
        }

        seen.insert(method_name, &collected.source_path);
    }

    Ok(())
}

/// Checks for wrapper struct name collisions across FPI import modules.
fn check_fpi_struct_name_collisions(modules: &[FpiModule]) -> syn::Result<()> {
    let mut seen: HashMap<String, &str> = HashMap::new();

    for module in modules {
        let struct_name = fpi_struct_ident(module)?.to_string();
        if let Some(existing_path) = seen.get(&struct_name) {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "FPI wrapper struct name collision: `{struct_name}` is generated for both \
                     `{existing_path}` and `{}`",
                    module.path_string
                ),
            ));
        }

        seen.insert(struct_name, &module.path_string);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse Rust source into a syn::File.
    fn parse_file(src: &str) -> File {
        syn::parse_str(src).unwrap_or_else(|e| panic!("failed to parse test source: {e}\n{src}"))
    }

    #[test]
    fn test_should_generate_struct_empty_path() {
        let empty_items: Vec<Item> = vec![];
        assert!(!should_generate_struct(&[], &empty_items));
    }

    #[test]
    fn test_should_generate_struct_exports_excluded() {
        let empty_items: Vec<Item> = vec![];
        let path = vec![syn::Ident::new("exports", Span::call_site())];
        assert!(!should_generate_struct(&path, &empty_items));

        let path = vec![
            syn::Ident::new("exports", Span::call_site()),
            syn::Ident::new("foo", Span::call_site()),
        ];
        assert!(!should_generate_struct(&path, &empty_items));
    }

    #[test]
    fn test_should_generate_struct_underscore_excluded() {
        let empty_items: Vec<Item> = vec![];
        let path = vec![syn::Ident::new("_private", Span::call_site())];
        assert!(!should_generate_struct(&path, &empty_items));

        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("_internal", Span::call_site()),
        ];
        assert!(!should_generate_struct(&path, &empty_items));
    }

    #[test]
    fn test_should_generate_struct_valid_leaf_modules() {
        let empty_items: Vec<Item> = vec![];
        let path = vec![syn::Ident::new("miden", Span::call_site())];
        assert!(should_generate_struct(&path, &empty_items));

        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        assert!(should_generate_struct(&path, &empty_items));
    }

    #[test]
    fn test_should_generate_struct_non_leaf_excluded() {
        let path = vec![syn::Ident::new("miden", Span::call_site())];
        // Items containing a nested module
        let items_with_mod: Vec<Item> = vec![syn::parse_quote! { mod nested {} }];
        assert!(!should_generate_struct(&path, &items_with_mod));

        // Items with only functions (leaf module) should be allowed
        let items_with_fn: Vec<Item> = vec![syn::parse_quote! { pub fn foo() {} }];
        assert!(should_generate_struct(&path, &items_with_fn));
    }

    #[test]
    fn test_is_target_function_public() {
        let func: ItemFn = syn::parse_quote! {
            pub fn receive_asset(asset: u64) {}
        };
        assert!(is_target_function(&func));
    }

    #[test]
    fn test_is_target_function_private_excluded() {
        let func: ItemFn = syn::parse_quote! {
            fn private_fn() {}
        };
        assert!(!is_target_function(&func));
    }

    #[test]
    fn test_is_target_function_unsafe_excluded() {
        let func: ItemFn = syn::parse_quote! {
            pub unsafe fn unsafe_fn() {}
        };
        assert!(!is_target_function(&func));
    }

    #[test]
    fn test_is_target_function_underscore_excluded() {
        let func: ItemFn = syn::parse_quote! {
            pub fn _internal() {}
        };
        assert!(!is_target_function(&func));
    }

    #[test]
    fn test_format_module_path() {
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        assert_eq!(format_module_path(&path), "miden::basic_wallet");
    }

    #[test]
    fn test_format_module_path_empty() {
        assert_eq!(format_module_path(&[]), "");
    }

    #[test]
    fn test_collect_arg_idents() {
        let func: ItemFn = syn::parse_quote! {
            pub fn foo(a: u32, b: String, c: Vec<u8>) {}
        };
        let idents = collect_arg_idents(&func).unwrap();
        let names: Vec<_> = idents.iter().map(|i| i.to_string()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_collect_arg_idents_empty() {
        let func: ItemFn = syn::parse_quote! {
            pub fn no_args() {}
        };
        let idents = collect_arg_idents(&func).unwrap();
        assert!(idents.is_empty());
    }

    #[test]
    fn test_collect_wrapper_methods_from_leaf_module() {
        let src = r#"
            mod miden {
                mod basic_wallet {
                    mod basic_wallet {
                        pub fn receive_asset(asset: u64) {}
                        pub fn send_asset(asset: u64) {}
                    }
                }
            }
        "#;
        let file = parse_file(src);
        let mut methods = Vec::new();
        collect_wrapper_methods(&file.items, &mut Vec::new(), &mut methods).unwrap();

        // Should have collected 2 methods from the leaf module
        assert_eq!(methods.len(), 2);

        // Check method names
        let method_names: Vec<_> = methods.iter().map(|m| m.method.sig.ident.to_string()).collect();
        assert!(method_names.contains(&"receive_asset".to_string()));
        assert!(method_names.contains(&"send_asset".to_string()));
    }

    #[test]
    fn test_collect_wrapper_methods_skips_exports() {
        let src = r#"
            mod exports {
                mod my_component {
                    pub fn exported_fn() {}
                }
            }
        "#;
        let file = parse_file(src);
        let mut methods = Vec::new();
        collect_wrapper_methods(&file.items, &mut Vec::new(), &mut methods).unwrap();

        // exports module should not contribute any methods
        assert!(methods.is_empty());
    }

    #[test]
    fn test_collect_wrapper_methods_skips_empty_modules() {
        let src = r#"
            mod miden {
                mod empty_module {
                }
            }
        "#;
        let file = parse_file(src);
        let mut methods = Vec::new();
        collect_wrapper_methods(&file.items, &mut Vec::new(), &mut methods).unwrap();

        // No methods should be collected from empty module
        assert!(methods.is_empty());
    }

    #[test]
    fn test_qualify_signature_types() {
        let func: ItemFn = syn::parse_quote! {
            pub fn test_fn(a: StructA, b: u64) -> StructB {}
        };
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("component", Span::call_site()),
        ];
        let method = build_wrapper_method(&func, &path).unwrap();

        // Check that the types are qualified
        let sig_str = method.sig.to_token_stream().to_string();
        assert!(sig_str.contains("miden :: component :: StructA"));
        assert!(sig_str.contains("miden :: component :: StructB"));
        // Primitives should not be qualified
        assert!(sig_str.contains("u64"));
        assert!(!sig_str.contains("miden :: component :: u64"));
    }

    #[test]
    fn test_build_wrapper_method_signature() {
        let func: ItemFn = syn::parse_quote! {
            pub fn receive_asset(asset: u64) {}
        };
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        let method = build_wrapper_method(&func, &path).unwrap();

        // Method should have &mut self as first parameter
        assert_eq!(method.sig.inputs.len(), 2);
        assert!(
            matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some())
        );

        // Should be public
        assert!(matches!(method.vis, syn::Visibility::Public(_)));

        // Should have inline attribute
        assert!(method.attrs.iter().any(|attr| { attr.path().is_ident("inline") }));
    }

    #[test]
    fn test_build_wrapper_method_with_return_type() {
        let func: ItemFn = syn::parse_quote! {
            pub fn get_value() -> u32 { 42 }
        };
        let path = vec![syn::Ident::new("test_mod", Span::call_site())];
        let method = build_wrapper_method(&func, &path).unwrap();

        // Return type should be preserved
        assert!(matches!(method.sig.output, ReturnType::Type(_, _)));
    }

    #[test]
    fn test_augment_generated_bindings_adds_account_struct() {
        let src = r#"
            mod miden {
                mod basic_wallet {
                    mod basic_wallet {
                        pub fn receive_asset(asset: u64) {}
                        pub fn send_asset(to: u32, amount: u64) -> bool { true }
                    }
                }
            }
        "#;
        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens).unwrap();
        let result_str = result.to_string();

        // Should contain the Account struct
        assert!(result_str.contains("struct Account"));
        assert!(result_str.contains("impl Account"));

        // Should contain wrapper methods
        assert!(result_str.contains("fn receive_asset"));
        assert!(result_str.contains("fn send_asset"));

        // Methods should have &mut self parameter
        assert!(result_str.contains("& mut self"));
    }

    #[test]
    fn test_augment_generated_bindings_empty_input() {
        let src = "";
        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens).unwrap();
        let result_str = result.to_string();

        // Should not add Account struct when there are no methods
        assert!(!result_str.contains("struct Account"));
    }

    #[test]
    fn test_augment_generated_bindings_exports_only() {
        let src = r#"
            mod exports {
                mod my_component {
                    pub fn exported_fn() {}
                }
            }
        "#;
        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens).unwrap();
        let result_str = result.to_string();

        // Should not add Account struct for exports-only bindings
        assert!(!result_str.contains("struct Account"));
    }

    #[test]
    fn test_augment_generated_bindings_preserves_original_modules() {
        let src = r#"
            mod miden {
                mod wallet {
                    pub fn get_balance() -> u64 { 0 }
                }
            }
        "#;
        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens).unwrap();
        let result_str = result.to_string();

        // Original module structure should be preserved
        assert!(result_str.contains("mod miden"));
        assert!(result_str.contains("mod wallet"));
        assert!(result_str.contains("fn get_balance"));
    }

    #[test]
    fn test_wrapper_call_tokens_generates_correct_path() {
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        let fn_ident = syn::Ident::new("receive_asset", Span::call_site());
        let args = vec![syn::Ident::new("asset", Span::call_site())];

        let tokens = wrapper_call_tokens(&path, &fn_ident, &args);
        let result = tokens.to_string();

        assert!(result.contains("crate :: bindings :: miden :: basic_wallet :: receive_asset"));
        assert!(result.contains("asset"));
    }

    #[test]
    fn test_parse_with_entry_generate() {
        let input: TokenStream2 = quote! { "miden:foo/bar": generate };
        let parsed = syn::parse2::<GenerateArgs>(quote! { with = { #input } }).unwrap();

        assert_eq!(parsed.with_entries.len(), 1);
        assert_eq!(parsed.with_entries[0].0, "miden:foo/bar");
        assert!(matches!(parsed.with_entries[0].1, WithOption::Generate));
    }

    #[test]
    fn test_parse_with_entry_path() {
        let input: TokenStream2 = quote! { "miden:foo/bar": ::my::custom::Type };
        let parsed = syn::parse2::<GenerateArgs>(quote! { with = { #input } }).unwrap();

        assert_eq!(parsed.with_entries.len(), 1);
        assert_eq!(parsed.with_entries[0].0, "miden:foo/bar");
        match &parsed.with_entries[0].1 {
            WithOption::Path(p) => assert_eq!(p, "::my::custom::Type"),
            _ => panic!("expected Path variant"),
        }
    }

    #[test]
    fn test_parse_multiple_with_entries() {
        let parsed = syn::parse2::<GenerateArgs>(quote! {
            with = {
                "miden:a/b": generate,
                "miden:c/d": ::foo::Bar
            }
        })
        .unwrap();

        assert_eq!(parsed.with_entries.len(), 2);
        assert_eq!(parsed.with_entries[0].0, "miden:a/b");
        assert_eq!(parsed.with_entries[1].0, "miden:c/d");
    }

    /// Integration test verifying that `augment_generated_bindings` produces valid Rust code.
    ///
    /// This test simulates realistic wit-bindgen output with custom types, multiple methods,
    /// and verifies the augmented output parses as valid Rust and contains the expected
    /// wrapper struct with properly qualified type paths.
    #[test]
    fn test_augment_generated_bindings_integration() {
        // Simulate more realistic wit-bindgen output with types and multiple leaf modules
        let src = r#"
            mod miden {
                mod basic_wallet {
                    mod basic_wallet {
                        pub struct AssetInfo {
                            pub amount: u64,
                        }

                        pub fn receive_asset(asset: AssetInfo) {}
                        pub fn move_asset_to_note(asset: AssetInfo, note_idx: u32) -> bool { true }
                        fn _internal_helper() {}  // Should be skipped (underscore prefix)
                    }
                }
                mod other_component {
                    mod other_component {
                        pub fn do_something(value: u64) -> u64 { value }
                    }
                }
            }
            mod exports {
                mod my_export {
                    pub fn exported_fn() {}  // Should be skipped (exports module)
                }
            }
        "#;

        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens).unwrap();

        // Verify the output parses as valid Rust
        let parsed: File =
            syn::parse2(result.clone()).expect("augmented bindings should be valid Rust syntax");

        // Find the Account struct and impl
        let has_account_struct = parsed
            .items
            .iter()
            .any(|item| matches!(item, Item::Struct(s) if s.ident == "Account"));
        let has_account_impl = parsed.items.iter().any(|item| {
            matches!(item, Item::Impl(i) if i.self_ty.to_token_stream().to_string() == "Account")
        });

        assert!(has_account_struct, "should generate Account struct");
        assert!(has_account_impl, "should generate Account impl");

        // Find the impl block and verify methods
        let impl_block = parsed
            .items
            .iter()
            .find_map(|item| match item {
                Item::Impl(i) if i.self_ty.to_token_stream().to_string() == "Account" => Some(i),
                _ => None,
            })
            .expect("Account impl should exist");

        let method_names: Vec<String> = impl_block
            .items
            .iter()
            .filter_map(|item| match item {
                ImplItem::Fn(f) => Some(f.sig.ident.to_string()),
                _ => None,
            })
            .collect();

        // Should include methods from both leaf modules
        assert!(method_names.contains(&"receive_asset".to_string()));
        assert!(method_names.contains(&"move_asset_to_note".to_string()));
        assert!(method_names.contains(&"do_something".to_string()));

        // Should NOT include internal helper or exported functions
        assert!(!method_names.contains(&"_internal_helper".to_string()));
        assert!(!method_names.contains(&"exported_fn".to_string()));

        // Verify type qualification in the result string
        let result_str = result.to_string();
        // AssetInfo should be qualified with its module path
        assert!(
            result_str.contains("miden :: basic_wallet :: basic_wallet :: AssetInfo"),
            "custom types should be qualified with module path"
        );
    }

    #[test]
    fn test_method_name_collision_detected() {
        // Two different interfaces with the same function name
        let src = r#"
            mod miden {
                mod interface_a {
                    mod interface_a {
                        pub fn transfer(amount: u64) {}
                    }
                }
                mod interface_b {
                    mod interface_b {
                        pub fn transfer(value: u32) {}
                    }
                }
            }
        "#;

        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens);

        assert!(result.is_err(), "should detect method name collision");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("method name collision"),
            "error should mention collision: {err_msg}"
        );
        assert!(err_msg.contains("transfer"), "error should mention the colliding method name");
    }

    #[test]
    fn test_no_collision_different_names() {
        let src = r#"
            mod miden {
                mod interface_a {
                    mod interface_a {
                        pub fn transfer_a(amount: u64) {}
                    }
                }
                mod interface_b {
                    mod interface_b {
                        pub fn transfer_b(value: u32) {}
                    }
                }
            }
        "#;

        let tokens: TokenStream2 = src.parse().unwrap();
        let result = augment_generated_bindings(tokens);

        assert!(result.is_ok(), "should not detect collision for different method names");
    }
}
