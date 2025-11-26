use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use heck::ToUpperCamelCase;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
    Attribute, Error, File, FnArg, ImplItem, ImplItemFn, Item, ItemFn, ItemImpl, ItemStruct,
    LitStr, Pat, ReturnType, Token,
};
use wit_bindgen_core::wit_parser::{PackageId, Resolve, UnresolvedPackageGroup};
use wit_bindgen_rust::{Opts, WithOption};

/// File name for the embedded Miden SDK WIT .
const SDK_WIT_FILE_NAME: &str = "miden.wit";
/// Embedded Miden SDK WIT source.
pub(crate) const SDK_WIT_SOURCE: &str = include_str!("../wit/miden.wit");

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

fn generate_bindings(
    args: &GenerateArgs,
    config: &manifest_paths::ResolvedWit,
    world_override: Option<&str>,
) -> Result<TokenStream2, Error> {
    let inline_src = args.inline.as_ref().map(|src| src.value());
    let inline_ref = inline_src.as_deref();
    let wit_sources = load_wit_sources(&config.paths, inline_ref)?;

    let world_spec = world_override.or(config.world.as_deref());
    let world = wit_sources
        .resolve
        .select_world(&wit_sources.packages, world_spec)
        .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;

    let mut opts = Opts {
        generate_all: true,
        runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
        default_bindings_module: Some("bindings".to_string()),
        ..Opts::default()
    };
    apply_with_entries(&mut opts, &args.with_entries);
    push_default_with_entries(&mut opts);

    let mut generated_files = wit_bindgen_core::Files::default();
    let mut generator = opts.build();
    generator
        .generate(&wit_sources.resolve, world, &mut generated_files)
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

fn augment_generated_bindings(tokens: TokenStream2) -> syn::Result<TokenStream2> {
    let mut file: File = syn::parse2(tokens)?;
    transform_modules(&mut file.items, &mut Vec::new())?;
    Ok(file.into_token_stream())
}

/// Result of loading WIT sources.
struct LoadedWitSources {
    /// The resolved WIT definitions.
    resolve: Resolve,
    /// Package IDs to use for world selection.
    packages: Vec<PackageId>,
    /// File paths that were read to include a dummy `include_bytes!` so rustc knows that we depend
    /// on the contents of those files.
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
        let (pkg, sources) = resolve
            .push_path(normalized.clone())
            .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;
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

/// Applies user-provided `with` entries to the wit-bindgen options.
fn apply_with_entries(opts: &mut Opts, entries: &[(String, WithOption)]) {
    opts.with.extend(entries.iter().cloned());
}

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

fn transform_modules(items: &mut [Item], path: &mut Vec<syn::Ident>) -> syn::Result<()> {
    for item in items.iter_mut() {
        if let Item::Mod(module) = item {
            path.push(module.ident.clone());
            if let Some((_, ref mut content)) = module.content {
                transform_modules(content, path)?;
                maybe_inject_struct_wrapper(content, path)?;
            }
            path.pop();
        }
    }

    Ok(())
}

/// Injects a wrapper struct and impl block for public functions in a module.
///
/// Note: We need `&mut Vec<Item>` here (not `&mut [Item]`) because we push new items
/// (the struct and impl block) to the vector.
fn maybe_inject_struct_wrapper(items: &mut Vec<Item>, path: &[syn::Ident]) -> syn::Result<()> {
    if !should_generate_struct(path) {
        return Ok(());
    }

    let functions: Vec<ItemFn> = items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(func) if is_target_function(func) => Some(func.clone()),
            _ => None,
        })
        .collect();

    if functions.is_empty() {
        return Ok(());
    }

    let module_ident =
        path.last().ok_or_else(|| Error::new(Span::call_site(), "empty module path"))?;
    let struct_ident =
        syn::Ident::new(&module_ident.to_string().to_upper_camel_case(), module_ident.span());

    if items
        .iter()
        .any(|item| matches!(item, Item::Struct(existing) if existing.ident == struct_ident))
    {
        return Ok(());
    }

    let struct_doc =
        format!("Wrapper for functions defined in module `{}`.", format_module_path(path));
    let struct_item: ItemStruct = parse_quote! {
        #[doc = #struct_doc]
        #[derive(Clone, Copy, Default)]
        pub struct #struct_ident;
    };

    let mut methods = Vec::new();
    for func in functions {
        methods.push(build_wrapper_method(&func, path)?);
    }

    if methods.is_empty() {
        return Ok(());
    }

    let mut impl_item: ItemImpl = parse_quote! {
        impl #struct_ident {}
    };
    impl_item.items.extend(methods.into_iter().map(ImplItem::Fn));

    items.push(Item::Struct(struct_item));
    items.push(Item::Impl(impl_item));

    Ok(())
}

fn build_wrapper_method(func: &ItemFn, module_path: &[syn::Ident]) -> syn::Result<ImplItemFn> {
    let mut sig = func.sig.clone();
    sig.inputs.insert(0, parse_quote!(&self));

    let arg_idents = collect_arg_idents(func)?;
    let call_expr = wrapper_call_tokens(module_path, &sig.ident, &arg_idents);

    let method_doc = format!("Calls `{}` from `{}`.", sig.ident, format_module_path(module_path));
    let doc_attr: Attribute = parse_quote!(#[doc = #method_doc]);
    let inline_attr: Attribute = parse_quote!(#[inline(always)]);
    let allow_unused_attr: Attribute = parse_quote!(#[allow(unused_variables)]);

    let body_tokens = match &sig.output {
        ReturnType::Default => quote!({ #call_expr; }),
        _ => quote!({ #call_expr }),
    };
    let block = syn::parse2(body_tokens)?;

    Ok(ImplItemFn {
        attrs: vec![doc_attr, inline_attr, allow_unused_attr],
        vis: func.vis.clone(),
        defaultness: None,
        sig,
        block,
    })
}

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
                    "unsupported argument pattern in generated function",
                )),
            },
        })
        .collect()
}

fn wrapper_call_tokens(
    module_path: &[syn::Ident],
    fn_ident: &syn::Ident,
    args: &[syn::Ident],
) -> TokenStream2 {
    let mut path_tokens = quote! { crate::bindings };
    for ident in module_path {
        let current = ident.clone();
        path_tokens = quote! { #path_tokens :: #current };
    }

    quote! { #path_tokens :: #fn_ident(#(#args),*) }
}

fn should_generate_struct(path: &[syn::Ident]) -> bool {
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
    !last.starts_with('_')
}

fn is_target_function(func: &ItemFn) -> bool {
    matches!(func.vis, syn::Visibility::Public(_))
        && func.sig.unsafety.is_none()
        && !func.sig.ident.to_string().starts_with('_')
}

fn format_module_path(path: &[syn::Ident]) -> String {
    path.iter().map(|ident| ident.to_string()).collect::<Vec<_>>().join("::")
}

mod manifest_paths {
    use toml::Value;

    use super::*;
    use crate::util::bundled_wit_folder;

    /// WIT metadata extracted from the consuming crate.
    pub struct ResolvedWit {
        pub paths: Vec<String>,
        pub world: Option<String>,
    }

    #[derive(Default)]
    pub struct ResolveOptions {
        pub allow_missing_local_wit: bool,
    }

    /// Collects WIT search paths and the target world from `Cargo.toml` + local files.
    pub fn resolve_wit_paths(options: ResolveOptions) -> Result<ResolvedWit, Error> {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").map_err(|err| {
            Error::new(Span::call_site(), format!("failed to read CARGO_MANIFEST_DIR: {err}"))
        })?;
        let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");

        let manifest_content = fs::read_to_string(&manifest_path).map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to read manifest '{}': {err}", manifest_path.display()),
            )
        })?;

        let manifest: Value = manifest_content.parse().map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to parse manifest '{}': {err}", manifest_path.display()),
            )
        })?;

        let canonical_prelude_dir = ensure_sdk_wit()?;

        let mut resolved = Vec::new();

        let prelude_dir = canonical_prelude_dir
            .to_str()
            .ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!("path '{}' contains invalid UTF-8", canonical_prelude_dir.display()),
                )
            })?
            .to_owned();

        resolved.push(prelude_dir);

        if let Some(dependencies) = manifest
            .get("package")
            .and_then(Value::as_table)
            .and_then(|package| package.get("metadata"))
            .and_then(Value::as_table)
            .and_then(|metadata| metadata.get("component"))
            .and_then(Value::as_table)
            .and_then(|component| component.get("target"))
            .and_then(Value::as_table)
            .and_then(|target| target.get("dependencies"))
            .and_then(Value::as_table)
        {
            for (name, dependency) in dependencies.iter() {
                let table = dependency.as_table().ok_or_else(|| {
                    Error::new(
                        Span::call_site(),
                        format!(
                            "dependency '{name}' under \
                             [package.metadata.component.target.dependencies] must be a table"
                        ),
                    )
                })?;

                let path_value = table.get("path").and_then(Value::as_str).ok_or_else(|| {
                    Error::new(
                        Span::call_site(),
                        format!("dependency '{name}' is missing a 'path' entry"),
                    )
                })?;

                let raw_path = PathBuf::from(path_value);
                let absolute = if raw_path.is_absolute() {
                    raw_path
                } else {
                    Path::new(&manifest_dir).join(&raw_path)
                };

                let canonical = fs::canonicalize(&absolute).unwrap_or_else(|_| absolute.clone());

                let metadata = fs::metadata(&canonical).map_err(|err| {
                    Error::new(
                        Span::call_site(),
                        format!(
                            "failed to read metadata for dependency '{name}' path '{}': {err}",
                            canonical.display()
                        ),
                    )
                })?;

                let search_path = if metadata.is_dir() {
                    canonical
                } else if let Some(parent) = canonical.parent() {
                    parent.to_path_buf()
                } else {
                    return Err(Error::new(
                        Span::call_site(),
                        format!(
                            "dependency '{name}' path '{}' does not have a parent directory",
                            canonical.display()
                        ),
                    ));
                };

                let path_str = search_path.to_str().ok_or_else(|| {
                    Error::new(
                        Span::call_site(),
                        format!("dependency '{name}' path contains invalid UTF-8"),
                    )
                })?;

                if !resolved.iter().any(|existing| existing == path_str) {
                    resolved.push(path_str.to_owned());
                }
            }
        }

        let local_wit_root = Path::new(&manifest_dir).join("wit");
        let mut world = None;

        if local_wit_root.exists() && !options.allow_missing_local_wit {
            let local_root = fs::canonicalize(&local_wit_root).unwrap_or(local_wit_root);
            let local_root_str = local_root.to_str().ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!("path '{}' contains invalid UTF-8", local_root.display()),
                )
            })?;
            if !resolved.iter().any(|existing| existing == local_root_str) {
                resolved.push(local_root_str.to_owned());
            }
            world = detect_world_name(&local_root)?;
        }

        Ok(ResolvedWit {
            paths: resolved,
            world,
        })
    }

    /// Ensures the embedded Miden SDK WIT is materialized in the project's folder.
    fn ensure_sdk_wit() -> Result<PathBuf, Error> {
        let autogenerated_wit_folder = bundled_wit_folder()?;

        let sdk_wit_path = autogenerated_wit_folder.join(super::SDK_WIT_FILE_NAME);
        let sdk_version: &str = env!("CARGO_PKG_VERSION");
        let expected_source = format!(
            "/// NOTE: This file is auto-generated from the Miden SDK.\n/// Version: \
             v{sdk_version}\n/// Any manual edits will be overwritten.\n\n{SDK_WIT_SOURCE}"
        );
        let should_write_wit = match fs::read_to_string(&sdk_wit_path) {
            Ok(existing) => existing != expected_source,
            Err(err) if err.kind() == ErrorKind::NotFound => true,
            Err(err) => {
                return Err(Error::new(
                    Span::call_site(),
                    format!("failed to read '{}': {err}", sdk_wit_path.display()),
                ));
            }
        };

        if should_write_wit {
            fs::write(&sdk_wit_path, expected_source).map_err(|err| {
                Error::new(
                    Span::call_site(),
                    format!("failed to write '{}': {err}", sdk_wit_path.display()),
                )
            })?;
        }

        Ok(fs::canonicalize(&autogenerated_wit_folder).unwrap_or(autogenerated_wit_folder))
    }

    /// Scans the component's `wit` directory to find the default world.
    fn detect_world_name(wit_root: &Path) -> Result<Option<String>, Error> {
        let mut entries = fs::read_dir(wit_root)
            .map_err(|err| {
                Error::new(
                    Span::call_site(),
                    format!("failed to read '{}': {err}", wit_root.display()),
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                Error::new(
                    Span::call_site(),
                    format!("failed to iterate '{}': {err}", wit_root.display()),
                )
            })?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == "deps") {
                continue;
            }
            if path.is_dir() {
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
                continue;
            }

            if let Some((package, world)) = parse_package_and_world(&path)? {
                return Ok(Some(format!("{package}/{world}")));
            }
        }

        Ok(None)
    }

    /// Parses a WIT source file for its package declaration and first world definition.
    fn parse_package_and_world(path: &Path) -> Result<Option<(String, String)>, Error> {
        let contents = fs::read_to_string(path).map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to read WIT file '{}': {err}", path.display()),
            )
        })?;

        let package = extract_package_name(&contents);
        let world = extract_world_name(&contents);

        match (package, world) {
            (Some(package), Some(world)) => Ok(Some((package, world))),
            _ => Ok(None),
        }
    }

    /// Returns the package identifier from a WIT source string, if present.
    fn extract_package_name(contents: &str) -> Option<String> {
        for line in contents.lines() {
            let trimmed = strip_comment(line).trim_start();
            if let Some(rest) = trimmed.strip_prefix("package ") {
                let mut token = rest.trim();
                if let Some(idx) = token.find(';') {
                    token = &token[..idx];
                }
                let mut name = token.trim();
                if let Some(idx) = name.find('@') {
                    name = &name[..idx];
                }
                return Some(name.trim().to_string());
            }
        }
        None
    }

    /// Returns the first world identifier from a WIT source string, if present.
    pub(super) fn extract_world_name(contents: &str) -> Option<String> {
        for line in contents.lines() {
            let trimmed = strip_comment(line).trim_start();
            if let Some(rest) = trimmed.strip_prefix("world ") {
                let mut name = String::new();
                for ch in rest.trim().chars() {
                    if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                        name.push(ch);
                    } else {
                        break;
                    }
                }
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
        None
    }

    /// Strips line comments starting with `//` from the provided source line.
    fn strip_comment(line: &str) -> &str {
        match line.split_once("//") {
            Some((before, _)) => before,
            None => line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse Rust source into a syn::File.
    fn parse_file(src: &str) -> File {
        syn::parse_str(src).expect("failed to parse test source")
    }

    #[test]
    fn test_should_generate_struct_empty_path() {
        assert!(!should_generate_struct(&[]));
    }

    #[test]
    fn test_should_generate_struct_exports_excluded() {
        let path = vec![syn::Ident::new("exports", Span::call_site())];
        assert!(!should_generate_struct(&path));

        let path = vec![
            syn::Ident::new("exports", Span::call_site()),
            syn::Ident::new("foo", Span::call_site()),
        ];
        assert!(!should_generate_struct(&path));
    }

    #[test]
    fn test_should_generate_struct_underscore_excluded() {
        let path = vec![syn::Ident::new("_private", Span::call_site())];
        assert!(!should_generate_struct(&path));

        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("_internal", Span::call_site()),
        ];
        assert!(!should_generate_struct(&path));
    }

    #[test]
    fn test_should_generate_struct_valid_paths() {
        let path = vec![syn::Ident::new("miden", Span::call_site())];
        assert!(should_generate_struct(&path));

        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        assert!(should_generate_struct(&path));
    }

    #[test]
    fn test_is_target_function_public() {
        let func: ItemFn = syn::parse_quote! {
            pub fn receive_asset(asset: Asset) {}
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
    fn test_transform_modules_injects_struct() {
        let src = r#"
            mod miden {
                mod basic_wallet {
                    mod basic_wallet {
                        pub fn receive_asset(asset: Asset) {}
                        pub fn send_asset(asset: Asset) {}
                    }
                }
            }
        "#;
        let mut file = parse_file(src);
        transform_modules(&mut file.items, &mut Vec::new()).unwrap();

        // Check that the innermost module now contains a struct and impl
        let miden_mod = match &file.items[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let basic_wallet_outer = match &miden_mod.content.as_ref().unwrap().1[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let basic_wallet_inner = match &basic_wallet_outer.content.as_ref().unwrap().1[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let inner_items = &basic_wallet_inner.content.as_ref().unwrap().1;

        // Should have: 2 functions + 1 struct + 1 impl = 4 items
        assert_eq!(inner_items.len(), 4);

        // Check struct exists and has correct name
        let struct_item = inner_items.iter().find_map(|item| match item {
            Item::Struct(s) => Some(s),
            _ => None,
        });
        assert!(struct_item.is_some());
        assert_eq!(struct_item.unwrap().ident.to_string(), "BasicWallet");

        // Check impl exists
        let impl_item = inner_items.iter().find_map(|item| match item {
            Item::Impl(i) => Some(i),
            _ => None,
        });
        assert!(impl_item.is_some());
        let impl_block = impl_item.unwrap();
        // Should have 2 methods
        assert_eq!(impl_block.items.len(), 2);
    }

    #[test]
    fn test_transform_modules_skips_exports() {
        let src = r#"
            mod exports {
                mod my_component {
                    pub fn exported_fn() {}
                }
            }
        "#;
        let mut file = parse_file(src);
        transform_modules(&mut file.items, &mut Vec::new()).unwrap();

        // exports module should not have any struct injected
        let exports_mod = match &file.items[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let my_component = match &exports_mod.content.as_ref().unwrap().1[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let items = &my_component.content.as_ref().unwrap().1;

        // Should only have the original function, no struct added
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], Item::Fn(_)));
    }

    #[test]
    fn test_transform_modules_skips_empty_modules() {
        let src = r#"
            mod miden {
                mod empty_module {
                }
            }
        "#;
        let mut file = parse_file(src);
        transform_modules(&mut file.items, &mut Vec::new()).unwrap();

        let miden_mod = match &file.items[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let empty_module = match &miden_mod.content.as_ref().unwrap().1[0] {
            Item::Mod(m) => m,
            _ => panic!("expected mod"),
        };
        let items = &empty_module.content.as_ref().unwrap().1;

        // Should remain empty
        assert!(items.is_empty());
    }

    #[test]
    fn test_build_wrapper_method_signature() {
        let func: ItemFn = syn::parse_quote! {
            pub fn receive_asset(asset: Asset) {}
        };
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("basic_wallet", Span::call_site()),
        ];
        let method = build_wrapper_method(&func, &path).unwrap();

        // Method should have &self as first parameter
        assert_eq!(method.sig.inputs.len(), 2);
        assert!(matches!(method.sig.inputs.first(), Some(FnArg::Receiver(_))));

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
}
