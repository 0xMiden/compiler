use std::{collections::HashSet, env, fs, path::PathBuf};

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use syn::{
    Error, FnArg, Item, ItemFn, LitStr, Pat, Token, TypePath,
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
    visit_mut::VisitMut,
};
use wit_bindgen_core::{
    WorldGenerator,
    wit_parser::{
        Function, Handle, InterfaceId, PackageId, Resolve, Type as WitType, TypeDefKind, TypeId,
        TypeOwner, UnresolvedPackageGroup, WorldId, WorldItem,
    },
};
use wit_bindgen_rust::{Opts, WithOption};

use crate::{fpi, manifest_paths};

/// Fully-qualified WIT interface path for Miden SDK core types.
pub(crate) const CORE_TYPES_INTERFACE: &str = "miden:base/core-types@1.0.0";

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
                Ok(raw_bindings) => quote! {
                    // Wrap the bindings in the `bindings` module since `generate!` makes a top level
                    // module named after the package namespace which is `miden` for all our projects
                    // so it conflicts with the `miden` crate (SDK)
                    #[doc(hidden)]
                    #[allow(dead_code)]
                    pub mod bindings {
                        #raw_bindings
                    }
                }
                .into(),
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
    generate_bindings_from_sources(
        &config.paths,
        args.inline.as_ref().map(|src| src.value()).as_deref(),
        world,
        &args.with_entries,
        &[],
        false,
    )
}

/// Generates inline WIT bindings and populates private FPI imports for selected dependencies.
pub(crate) fn generate_inline_fpi_bindings(
    config: &manifest_paths::ResolvedWit,
    inline_source: &str,
    world: &str,
    fpi_imports: &[fpi::FpiImportSpec],
    with_entries: &[(String, WithOption)],
) -> Result<TokenStream2, Error> {
    generate_bindings_from_sources(
        &config.paths,
        Some(inline_source),
        Some(world),
        with_entries,
        fpi_imports,
        true,
    )
}

/// Generates inline bindings for an import-only world without injecting FPI variants.
///
/// Used by the `#[component]` sibling generator: the imported dependency functions are kept
/// as-is and lower to direct cross-context calls, so no `fpi-*` companions are synthesized.
pub(crate) fn generate_inline_import_bindings(
    config: &manifest_paths::ResolvedWit,
    inline_source: &str,
    world: &str,
    with_entries: &[(String, WithOption)],
) -> Result<TokenStream2, Error> {
    generate_bindings_from_sources(
        &config.paths,
        Some(inline_source),
        Some(world),
        with_entries,
        &[],
        false,
    )
}

/// Generates WIT bindings from resolved source paths and optional inline source.
fn generate_bindings_from_sources(
    paths: &[String],
    inline_source: Option<&str>,
    world: Option<&str>,
    with_entries: &[(String, WithOption)],
    fpi_imports: &[fpi::FpiImportSpec],
    scope_component_type_sections: bool,
) -> Result<TokenStream2, Error> {
    let mut wit_sources = load_wit_sources(paths, inline_source)?;

    let world_id = wit_sources
        .resolve
        .select_world(&wit_sources.packages, world)
        .map_err(|err| Error::new(Span::call_site(), err.to_string()))?;
    fpi::inject_imports(&mut wit_sources.resolve, world_id, fpi_imports)?;
    #[cfg(feature = "internal-wit-emit")]
    if inline_source.is_some() {
        maybe_emit_inline_wit(&wit_sources.resolve, world_id, fpi_imports)?;
    }

    let mut opts = Opts {
        generate_all: true,
        runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
        default_bindings_module: Some("bindings".to_string()),
        ..Opts::default()
    };
    push_custom_with_entries(&mut opts, with_entries);
    if world_uses_miden_core_types(&wit_sources.resolve, world_id) {
        push_default_with_entries(&mut opts);
    }

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
    let mut file = syn::parse_file(src)
        .map_err(|err| Error::new(Span::call_site(), format!("failed to parse bindings: {err}")))?;
    if scope_component_type_sections {
        append_module_path_to_component_type_sections(&mut file)?;
    }
    let mut tokens = file.into_token_stream();

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
    #[cfg(feature = "internal-wit-emit")]
    if inline_source.is_some() {
        // Make Cargo invalidate cached macro output when inline-WIT emission is toggled.
        tokens.extend(quote! {
            const _: Option<&str> = option_env!("MIDENC_EMIT_WIT");
        });
    }

    Ok(tokens)
}

/// Emits a resolved inline WIT world when `MIDENC_EMIT_WIT[=<path>]` is set.
#[cfg(feature = "internal-wit-emit")]
fn maybe_emit_inline_wit(
    resolve: &Resolve,
    world_id: WorldId,
    fpi_imports: &[fpi::FpiImportSpec],
) -> Result<(), Error> {
    let Some(value) = env::var_os("MIDENC_EMIT_WIT") else {
        return Ok(());
    };
    let out_dir = if value.is_empty() || value == std::ffi::OsStr::new("1") {
        env::current_dir().map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to resolve the MIDENC_EMIT_WIT output directory: {err}"),
            )
        })?
    } else {
        PathBuf::from(value)
    };
    fs::create_dir_all(&out_dir).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!(
                "failed to create MIDENC_EMIT_WIT output directory '{}': {err}",
                out_dir.display()
            ),
        )
    })?;

    let source = render_resolved_inline_wit(resolve, world_id, fpi_imports)?;
    let package_name = env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "generated".to_string());
    let world_name = &resolve.worlds[world_id].name;
    let filename = format!(
        "{}.{}.inline.wit",
        sanitize_wit_filename_component(&package_name),
        sanitize_wit_filename_component(world_name)
    );
    let out_file = out_dir.join(filename);
    fs::write(&out_file, source).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to write inline WIT to '{}': {err}", out_file.display()),
        )
    })?;
    eprintln!("wrote inline WIT to '{}'", out_file.display());
    Ok(())
}

/// Renders the selected inline package and any synthetic FPI packages imported by its world.
#[cfg(any(test, feature = "internal-wit-emit"))]
fn render_resolved_inline_wit(
    resolve: &Resolve,
    world_id: WorldId,
    fpi_imports: &[fpi::FpiImportSpec],
) -> Result<String, Error> {
    let package_id = resolve.worlds[world_id].package.ok_or_else(|| {
        Error::new(Span::call_site(), "inline WIT world is not owned by a package")
    })?;
    let mut nested_packages = Vec::new();
    for import in fpi_imports {
        let synthetic_package = import.synthetic_package();
        let nested_package =
            resolve.package_names.get(synthetic_package).copied().ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!(
                        "synthetic FPI package `{synthetic_package}` is missing after injection"
                    ),
                )
            })?;
        if !nested_packages.contains(&nested_package) {
            nested_packages.push(nested_package);
        }
    }

    let mut printer = wit_component::WitPrinter::default();
    printer.print(resolve, package_id, &nested_packages).map_err(|err| {
        Error::new(Span::call_site(), format!("failed to render resolved inline WIT: {err}"))
    })?;
    Ok(printer.output.into())
}

/// Converts a package or world name into a portable filename component.
#[cfg(any(test, feature = "internal-wit-emit"))]
fn sanitize_wit_filename_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => out.push(ch),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        "generated".to_string()
    } else {
        out
    }
}

/// Appends the generated binding module path to every component-metadata section name.
fn append_module_path_to_component_type_sections(file: &mut syn::File) -> Result<(), Error> {
    /// Rewrites generated component-metadata attributes in place.
    struct SectionVisitor {
        rewritten: usize,
        error: Option<Error>,
    }

    impl VisitMut for SectionVisitor {
        fn visit_attribute_mut(&mut self, attribute: &mut syn::Attribute) {
            if self.error.is_some() || !attribute.path().is_ident("unsafe") {
                return;
            }
            let syn::Meta::List(unsafe_meta) = &attribute.meta else {
                return;
            };
            let Ok(syn::Meta::NameValue(link_section)) =
                syn::parse2::<syn::Meta>(unsafe_meta.tokens.clone())
            else {
                return;
            };
            if !link_section.path.is_ident("link_section") {
                return;
            }
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(section_name),
                ..
            }) = &link_section.value
            else {
                self.error = Some(Error::new_spanned(
                    &link_section.value,
                    "wit-bindgen component metadata section name must be a string literal",
                ));
                return;
            };
            if !section_name.value().starts_with("component-type") {
                return;
            }

            let section_name = section_name.clone();
            attribute.meta = parse_quote! {
                unsafe(link_section = concat!(
                    #section_name,
                    ":",
                    env!("CARGO_PKG_NAME"),
                    "@",
                    env!("CARGO_PKG_VERSION"),
                    ":",
                    module_path!(),
                ))
            };
            self.rewritten += 1;
        }
    }

    let mut visitor = SectionVisitor {
        rewritten: 0,
        error: None,
    };
    visitor.visit_file_mut(file);
    if let Some(error) = visitor.error {
        return Err(error);
    }
    if visitor.rewritten == 0 {
        return Err(Error::new(
            Span::call_site(),
            "wit-bindgen emitted no component metadata section to scope to the account binding",
        ));
    }
    Ok(())
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

/// Pushes user-provided `with` entries to the wit-bindgen options.
fn push_custom_with_entries(opts: &mut Opts, entries: &[(String, WithOption)]) {
    opts.with.extend(entries.iter().cloned());
}

/// Pushes default `with` entries that map Miden base types to SDK types.
fn push_default_with_entries(opts: &mut Opts) {
    opts.with.push((CORE_TYPES_INTERFACE.to_string(), WithOption::Generate));
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/felt"), "::miden::Felt");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/word"), "::miden::Word");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/asset"), "::miden::Asset");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/asset-amount"), "::miden::AssetAmount");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/account-id"), "::miden::AccountId");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/tag"), "::miden::Tag");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/note-type"), "::miden::NoteType");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/recipient"), "::miden::Recipient");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/note-idx"), "::miden::NoteIdx");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/nonce"), "::miden::Nonce");
    push_path_entry(opts, &format!("{CORE_TYPES_INTERFACE}/block-number"), "::miden::BlockNumber");
}

fn push_path_entry(opts: &mut Opts, key: &str, value: &str) {
    opts.with.push((key.to_string(), WithOption::Path(value.to_string())));
}

/// Returns true when the selected world references Miden SDK core types.
fn world_uses_miden_core_types(resolve: &Resolve, world_id: WorldId) -> bool {
    let world = &resolve.worlds[world_id];
    world
        .imports
        .values()
        .chain(world.exports.values())
        .any(|item| world_item_uses_interface(resolve, item, CORE_TYPES_INTERFACE))
}

/// Returns true when a world item references a type from `interface_path`.
fn world_item_uses_interface(resolve: &Resolve, item: &WorldItem, interface_path: &str) -> bool {
    match item {
        WorldItem::Interface { id, .. } => interface_uses_interface(resolve, *id, interface_path),
        WorldItem::Function(function) => function_uses_interface(resolve, function, interface_path),
        WorldItem::Type { id, .. } => {
            type_id_uses_interface(resolve, *id, interface_path, &mut HashSet::new())
        }
    }
}

/// Returns true when an interface or any of its signatures/types references `interface_path`.
fn interface_uses_interface(
    resolve: &Resolve,
    interface_id: InterfaceId,
    interface_path: &str,
) -> bool {
    let interface = &resolve.interfaces[interface_id];
    if interface_matches(resolve, interface_id, interface_path) {
        return true;
    }

    let mut visited = HashSet::new();
    interface.functions.values().any(|function| {
        function_uses_interface_with_visited(resolve, function, interface_path, &mut visited)
    }) || interface
        .types
        .values()
        .any(|id| type_id_uses_interface(resolve, *id, interface_path, &mut visited))
}

/// Returns true when a function signature references `interface_path`.
fn function_uses_interface(resolve: &Resolve, function: &Function, interface_path: &str) -> bool {
    function_uses_interface_with_visited(resolve, function, interface_path, &mut HashSet::new())
}

/// Returns true when a function signature references `interface_path`.
fn function_uses_interface_with_visited(
    resolve: &Resolve,
    function: &Function,
    interface_path: &str,
    visited: &mut HashSet<TypeId>,
) -> bool {
    function
        .params
        .iter()
        .any(|param| type_uses_interface(resolve, &param.ty, interface_path, visited))
        || function
            .result
            .as_ref()
            .is_some_and(|ty| type_uses_interface(resolve, ty, interface_path, visited))
}

/// Returns true when a WIT type references `interface_path`.
fn type_uses_interface(
    resolve: &Resolve,
    ty: &WitType,
    interface_path: &str,
    visited: &mut HashSet<TypeId>,
) -> bool {
    match ty {
        WitType::Id(id) => type_id_uses_interface(resolve, *id, interface_path, visited),
        _ => false,
    }
}

/// Returns true when a WIT type definition references `interface_path`.
fn type_id_uses_interface(
    resolve: &Resolve,
    type_id: TypeId,
    interface_path: &str,
    visited: &mut HashSet<TypeId>,
) -> bool {
    if !visited.insert(type_id) {
        return false;
    }

    let def = &resolve.types[type_id];
    if type_owner_uses_interface(resolve, def.owner, interface_path) {
        return true;
    }

    match &def.kind {
        TypeDefKind::Record(record) => record
            .fields
            .iter()
            .any(|field| type_uses_interface(resolve, &field.ty, interface_path, visited)),
        TypeDefKind::Tuple(tuple) => tuple
            .types
            .iter()
            .any(|ty| type_uses_interface(resolve, ty, interface_path, visited)),
        TypeDefKind::Variant(variant) => variant
            .cases
            .iter()
            .filter_map(|case| case.ty.as_ref())
            .any(|ty| type_uses_interface(resolve, ty, interface_path, visited)),
        TypeDefKind::Option(ty)
        | TypeDefKind::List(ty)
        | TypeDefKind::FixedLengthList(ty, _)
        | TypeDefKind::Type(ty)
        | TypeDefKind::Future(Some(ty))
        | TypeDefKind::Stream(Some(ty)) => {
            type_uses_interface(resolve, ty, interface_path, visited)
        }
        TypeDefKind::Result(result) => result
            .ok
            .as_ref()
            .into_iter()
            .chain(result.err.as_ref())
            .any(|ty| type_uses_interface(resolve, ty, interface_path, visited)),
        TypeDefKind::Map(key, value) => {
            type_uses_interface(resolve, key, interface_path, visited)
                || type_uses_interface(resolve, value, interface_path, visited)
        }
        TypeDefKind::Handle(Handle::Own(id) | Handle::Borrow(id)) => {
            type_id_uses_interface(resolve, *id, interface_path, visited)
        }
        TypeDefKind::Resource
        | TypeDefKind::Flags(_)
        | TypeDefKind::Enum(_)
        | TypeDefKind::Future(None)
        | TypeDefKind::Stream(None)
        | TypeDefKind::Unknown => false,
    }
}

/// Returns true when a type owner belongs to `interface_path`.
fn type_owner_uses_interface(resolve: &Resolve, owner: TypeOwner, interface_path: &str) -> bool {
    match owner {
        TypeOwner::World(_) => false,
        TypeOwner::Interface(id) => interface_matches(resolve, id, interface_path),
        TypeOwner::None => false,
    }
}

/// Returns true when an interface ID matches a fully-qualified WIT interface path.
fn interface_matches(resolve: &Resolve, interface_id: InterfaceId, interface_path: &str) -> bool {
    let interface = &resolve.interfaces[interface_id];
    let (Some(package_id), Some(name)) = (interface.package, interface.name.as_deref()) else {
        return false;
    };
    resolve.packages[package_id].name.interface_id(name) == interface_path
}

/// Qualifies type paths in a function signature with the module path prefix.
///
/// This transforms simple type names (e.g., `StructA`) into fully qualified paths
/// (e.g., `miden::component::component::StructA`) so they resolve correctly when
/// the method is placed at the bindings root level.
pub(crate) fn qualify_signature_types(sig: &mut syn::Signature, module_path: &[syn::Ident]) {
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
                    syn::visit_mut::visit_type_path_mut(self, type_path);
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
pub(crate) fn collect_arg_idents(func: &ItemFn) -> syn::Result<Vec<syn::Ident>> {
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

/// Determines whether a wrapper struct should be generated for the given module.
///
/// Returns `false` for:
/// - Empty paths
/// - `exports` modules (these are user-implemented exports, not imports)
/// - Modules starting with underscore (internal/private modules)
/// - Non-leaf modules (modules that contain nested modules)
pub(crate) fn should_generate_struct(path: &[syn::Ident], items: &[Item]) -> bool {
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

/// Formats a module path as a `::` separated string for use in documentation.
pub(crate) fn format_module_path(path: &[syn::Ident]) -> String {
    path.iter().map(|ident| ident.to_string()).collect::<Vec<_>>().join("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Produces portable, non-empty inline-WIT artifact names.
    #[test]
    fn inline_wit_filename_components_are_sanitized() {
        assert_eq!(sanitize_wit_filename_component("template-test"), "template-test");
        assert_eq!(
            sanitize_wit_filename_component("miden:note/world@1.0.0"),
            "miden_note_world_1_0_0"
        );
        assert_eq!(sanitize_wit_filename_component(""), "generated");
    }

    /// Includes synthetic FPI interfaces when rendering the resolved inline binding world.
    #[test]
    fn resolved_inline_wit_contains_injected_fpi_functions() {
        const SOURCE_IMPORT: &str = "miden:wallet/api@1.0.0";

        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        let dependency = UnresolvedPackageGroup::parse(
            "wallet.wit",
            r#"
package miden:wallet@1.0.0;

interface api {
    ping: func(value: u32) -> u32;
}
"#,
        )
        .unwrap();
        resolve.push_group(dependency).unwrap();

        let specs = fpi::import_specs(&[SOURCE_IMPORT.to_string()]).unwrap();
        let inline = fpi::import_world_wit("foreign-account-bindings-test", &specs);
        let group = UnresolvedPackageGroup::parse("inline", &inline).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();
        fpi::inject_imports(&mut resolve, world, &specs).unwrap();

        let rendered = render_resolved_inline_wit(&resolve, world, &specs).unwrap();

        assert!(rendered.contains("import miden:fpi-v1-wallet/api@1.0.0;"));
        assert!(rendered.contains("package miden:fpi-v1-wallet@1.0.0 {"));
        assert!(rendered.contains("fpi-ping: func("));
        UnresolvedPackageGroup::parse("emitted.wit", &rendered).unwrap();
    }

    /// Rewrites component metadata to use semantic Rust module identity.
    #[test]
    fn component_type_sections_use_the_stable_rust_module_path() {
        let mut file: syn::File = syn::parse_quote! {
            #[unsafe(link_section = "component-type:wit-bindgen:test")]
            static COMPONENT_TYPE: [u8; 1] = [0];
        };

        append_module_path_to_component_type_sections(&mut file).unwrap();

        let Item::Static(item) = &file.items[0] else {
            panic!("fixture must remain a static item");
        };
        let syn::Meta::List(unsafe_meta) = &item.attrs[0].meta else {
            panic!("link section must remain wrapped in an unsafe attribute");
        };
        let syn::Meta::NameValue(link_section) =
            syn::parse2::<syn::Meta>(unsafe_meta.tokens.clone()).unwrap()
        else {
            panic!("unsafe attribute must contain link_section metadata");
        };
        let syn::Expr::Macro(section_name) = &link_section.value else {
            panic!("link section must be assembled by concat!");
        };
        assert!(section_name.mac.path.is_ident("concat"));
        let tokens = section_name.mac.tokens.to_string();
        assert!(tokens.contains("component-type:wit-bindgen:test"));
        assert!(tokens.contains("CARGO_PKG_NAME"));
        assert!(tokens.contains("CARGO_PKG_VERSION"));
        assert!(tokens.contains("module_path"));
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
    fn test_qualify_signature_types() {
        let mut sig: syn::Signature = syn::parse_quote! {
            fn test_fn(a: StructA, b: u64) -> StructB
        };
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("component", Span::call_site()),
        ];
        qualify_signature_types(&mut sig, &path);

        // Check that the custom types are qualified with the module path
        let sig_str = sig.to_token_stream().to_string();
        assert!(sig_str.contains("miden :: component :: StructA"));
        assert!(sig_str.contains("miden :: component :: StructB"));
        // Primitives should not be qualified
        assert!(sig_str.contains("u64"));
        assert!(!sig_str.contains("miden :: component :: u64"));
    }

    #[test]
    fn test_qualify_signature_types_inside_option() {
        let mut sig: syn::Signature = syn::parse_quote! {
            fn roundtrip(payload: Option<OptionPayload>) -> Option<OptionPayload>
        };
        let path = vec![
            syn::Ident::new("miden", Span::call_site()),
            syn::Ident::new("account", Span::call_site()),
            syn::Ident::new("interface", Span::call_site()),
        ];

        qualify_signature_types(&mut sig, &path);
        let signature = sig.to_token_stream().to_string().replace(' ', "");

        assert!(signature.contains("payload:Option<miden::account::interface::OptionPayload>"));
        assert!(signature.contains("->Option<miden::account::interface::OptionPayload>"));
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

    /// Parses a test WIT world with the bundled SDK WIT available in the resolver.
    fn parse_test_world(source: &str) -> (Resolve, WorldId) {
        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        let group = UnresolvedPackageGroup::parse("inline", source).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();
        (resolve, world)
    }

    #[test]
    fn test_world_uses_miden_core_types_rejects_primitive_only_world() {
        let (resolve, world) = parse_test_world(
            r#"
package miden:primitive-variant@0.1.0;

interface primitive-variant {
    variant request {
        tiny(u8),
        wide(u64),
    }

    roundtrip: func(request: request) -> request;
}

world primitive-variant-world {
    export primitive-variant;
}
"#,
        );

        assert!(!world_uses_miden_core_types(&resolve, world));
    }

    #[test]
    fn test_world_uses_miden_core_types_detects_imported_payload() {
        let (resolve, world) = parse_test_world(
            r#"
package miden:core-type-variant@0.1.0;

use miden:base/core-types@1.0.0;

interface core-type-variant {
    use core-types.{word};

    variant request {
        elements(word),
        amount(u64),
    }

    roundtrip: func(request: request) -> request;
}

world core-type-variant-world {
    export core-type-variant;
}
"#,
        );

        assert!(world_uses_miden_core_types(&resolve, world));
    }

    /// Preserves dependency-owned types across canonical source and synthetic FPI interfaces.
    #[test]
    fn synthetic_fpi_interface_preserves_source_types_and_generated_module_paths() {
        const SOURCE_IMPORT: &str = "miden:typed-dependency/api@1.2.3";
        const TYPE_PATH: &str = "crate::bindings::miden::typed_dependency::api::Payload";

        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        let dependency = UnresolvedPackageGroup::parse(
            "typed-dependency.wit",
            r#"
package miden:typed-dependency@1.2.3;

use miden:base/core-types@1.0.0;

interface api {
    use core-types.{felt, word};

    type scalar-alias = u32;

    record payload {
        value: scalar-alias,
        key: word,
    }

    variant request {
        none,
        payload(payload),
    }

    enum mode {
        fast,
        thorough,
    }

    flags permissions {
        read,
        write,
    }

    type maybe-request = option<request>;
    type nested-result = result<payload, mode>;

    primitive-roundtrip: func(value: u64) -> u32;
    roundtrip: func(value: payload) -> payload;
    choose: func(value: request) -> request;
    set-mode: func(value: mode) -> mode;
    set-permissions: func(value: permissions) -> permissions;
    core-roundtrip: func(value: word) -> felt;
    nested-roundtrip: func(value: maybe-request) -> nested-result;
}
"#,
        )
        .unwrap();
        resolve.push_group(dependency).unwrap();

        let specs = fpi::import_specs(&[SOURCE_IMPORT.to_string()]).unwrap();
        let inline = fpi::import_world_wit("fpi-type-test", &specs);
        let group = UnresolvedPackageGroup::parse("inline", &inline).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();

        let source_id = resolve.worlds[world]
            .imports
            .values()
            .filter_map(|item| match item {
                WorldItem::Interface { id, .. }
                    if interface_matches(&resolve, *id, SOURCE_IMPORT) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .next()
            .expect("source interface must be imported");
        let source_before = resolve.interfaces[source_id].clone();
        let payload = resolve.interfaces[source_id].types["payload"];

        fpi::inject_imports(&mut resolve, world, &specs).unwrap();
        assert_eq!(resolve.interfaces[source_id], source_before);

        let synthetic_id = resolve.worlds[world]
            .imports
            .values()
            .find_map(|item| match item {
                WorldItem::Interface { id, .. }
                    if interface_matches(&resolve, *id, specs[0].synthetic_import()) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .expect("synthetic interface must be imported");
        for (name, source_function) in &resolve.interfaces[source_id].functions {
            let synthetic_function = &resolve.interfaces[synthetic_id].functions
                [&format!("{}{}", fpi::WIT_FUNCTION_PREFIX, name)];
            assert_eq!(synthetic_function.params.len(), source_function.params.len() + 3);
            for (source, synthetic) in
                source_function.params.iter().zip(synthetic_function.params.iter().skip(3))
            {
                assert_synthetic_type_alias(&resolve, synthetic_id, source.ty, synthetic.ty);
            }
            match (source_function.result, synthetic_function.result) {
                (Some(source), Some(synthetic)) => {
                    assert_synthetic_type_alias(&resolve, synthetic_id, source, synthetic);
                }
                (None, None) => {}
                result => panic!("source and synthetic results differ: {result:?}"),
            }
        }
        let fpi_function = &resolve.interfaces[synthetic_id].functions["fpi-roundtrip"];
        let WitType::Id(payload_alias) = fpi_function.params[3].ty else {
            panic!("dependency-owned payload must be represented by a local alias");
        };
        assert_eq!(fpi_function.result, Some(WitType::Id(payload_alias)));
        assert_eq!(resolve.types[payload_alias].kind, TypeDefKind::Type(WitType::Id(payload)));
        assert_eq!(resolve.types[payload_alias].owner, TypeOwner::Interface(synthetic_id));
        assert!(
            resolve.interfaces[source_id]
                .functions
                .keys()
                .all(|name| !name.starts_with(fpi::WIT_FUNCTION_PREFIX))
        );
        resolve.assert_valid();

        let mut opts = Opts {
            generate_all: true,
            runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
            default_bindings_module: Some("bindings".to_string()),
            ..Opts::default()
        };
        opts.with
            .push((format!("{SOURCE_IMPORT}/payload"), WithOption::Path(TYPE_PATH.to_string())));
        push_default_with_entries(&mut opts);

        let mut generated_files = wit_bindgen_core::Files::default();
        opts.build().generate(&mut resolve, world, &mut generated_files).unwrap();
        let (_, source) = generated_files.iter().next().unwrap();
        let file: syn::File = syn::parse_str(std::str::from_utf8(source).unwrap()).unwrap();
        let native_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_plain_import_function).unwrap();
        let foreign_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_fpi_import_function).unwrap();
        let native = native_modules
            .iter()
            .find(|module| module.path_string == "miden::typed_dependency::api")
            .expect("real import must keep its canonical generated module path");
        let foreign = foreign_modules
            .iter()
            .find(|module| module.path_string == specs[0].synthetic_module_path())
            .expect("canonical synthetic import must determine its generated module path");

        let native_signature = native
            .functions
            .iter()
            .find(|function| function.sig.ident == "roundtrip")
            .unwrap()
            .sig
            .to_token_stream()
            .to_string();
        let foreign_signature = foreign
            .functions
            .iter()
            .find(|function| function.sig.ident == "fpi_roundtrip")
            .unwrap()
            .sig
            .to_token_stream()
            .to_string();
        let mapped_type = TYPE_PATH.replace("::", " :: ");
        assert!(native_signature.contains(&mapped_type), "signature: {native_signature}");
        assert!(foreign_signature.contains(&mapped_type), "signature: {foreign_signature}");
    }

    /// Rebuilds anonymous compound types without creating cross-owner aliases.
    #[test]
    fn synthetic_fpi_interface_preserves_anonymous_compound_types() {
        const SOURCE_IMPORT: &str = "miden:anonymous-dependency/api@1.0.0";

        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        let dependency = UnresolvedPackageGroup::parse(
            "anonymous-dependency.wit",
            r#"
package miden:anonymous-dependency@1.0.0;

use miden:base/core-types@1.0.0;

interface api {
    use core-types.{felt, word};

    record payload {
        value: u32,
        key: word,
    }

    many: func(values: list<payload>) -> list<payload>;
    find: func(key: word) -> option<payload>;
    try-get: func(flag: bool) -> result<payload, felt>;
    pair: func() -> tuple<felt, payload>;
    words: func(values: list<word>) -> u32;
}
"#,
        )
        .unwrap();
        resolve.push_group(dependency).unwrap();

        let specs = fpi::import_specs(&[SOURCE_IMPORT.to_string()]).unwrap();
        let inline = fpi::import_world_wit("fpi-anonymous-test", &specs);
        let group = UnresolvedPackageGroup::parse("inline", &inline).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();

        fpi::inject_imports(&mut resolve, world, &specs).unwrap();
        resolve.assert_valid();

        let mut opts = Opts {
            generate_all: true,
            runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
            default_bindings_module: Some("bindings".to_string()),
            ..Opts::default()
        };
        push_default_with_entries(&mut opts);

        let mut generated_files = wit_bindgen_core::Files::default();
        opts.build().generate(&mut resolve, world, &mut generated_files).unwrap();
        let (_, source) = generated_files.iter().next().unwrap();
        let file: syn::File = syn::parse_str(std::str::from_utf8(source).unwrap()).unwrap();
        let native_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_plain_import_function).unwrap();
        let foreign_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_fpi_import_function).unwrap();
        let native = native_modules
            .iter()
            .find(|module| module.path_string == "miden::anonymous_dependency::api")
            .expect("native anonymous-type bindings");
        let foreign = foreign_modules
            .iter()
            .find(|module| module.path_string == specs[0].synthetic_module_path())
            .expect("synthetic anonymous-type bindings");

        assert_eq!(native.functions.len(), 5);
        assert_eq!(foreign.functions.len(), native.functions.len());
    }

    /// Preserves aliases imported from a sibling WIT interface.
    #[test]
    fn synthetic_fpi_interface_preserves_cross_interface_use_aliases() {
        const SOURCE_IMPORT: &str = "miden:shared-types-dependency/api@1.0.0";

        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        let dependency = UnresolvedPackageGroup::parse(
            "shared-types-dependency.wit",
            r#"
package miden:shared-types-dependency@1.0.0;

interface types {
    record payload {
        value: u32,
    }
}

interface api {
    use types.{payload};
    roundtrip: func(value: payload) -> payload;
}
"#,
        )
        .unwrap();
        resolve.push_group(dependency).unwrap();

        let specs = fpi::import_specs(&[SOURCE_IMPORT.to_string()]).unwrap();
        let inline = fpi::import_world_wit("fpi-use-alias-test", &specs);
        let group = UnresolvedPackageGroup::parse("inline", &inline).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();

        fpi::inject_imports(&mut resolve, world, &specs).unwrap();
        resolve.assert_valid();

        let mut generated_files = wit_bindgen_core::Files::default();
        let mut opts = Opts {
            generate_all: true,
            runtime_path: Some("::miden::wit_bindgen::rt".to_string()),
            default_bindings_module: Some("bindings".to_string()),
            ..Opts::default()
        };
        push_default_with_entries(&mut opts);
        opts.build().generate(&mut resolve, world, &mut generated_files).unwrap();
        let (_, source) = generated_files.iter().next().unwrap();
        let file: syn::File = syn::parse_str(std::str::from_utf8(source).unwrap()).unwrap();
        let native_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_plain_import_function).unwrap();
        let foreign_modules =
            fpi::collect_import_modules(&file.items, &fpi::is_fpi_import_function).unwrap();

        let native = native_modules
            .iter()
            .find(|module| module.path_string == "miden::shared_types_dependency::api")
            .expect("native cross-interface alias bindings");
        let foreign = foreign_modules
            .iter()
            .find(|module| module.path_string == specs[0].synthetic_module_path())
            .expect("synthetic cross-interface alias bindings");
        let native_function = native
            .functions
            .iter()
            .find(|function| function.sig.ident == "roundtrip")
            .expect("native roundtrip binding");
        let foreign_function = foreign
            .functions
            .iter()
            .find(|function| function.sig.ident == "fpi_roundtrip")
            .expect("synthetic roundtrip binding");
        let FnArg::Typed(native_input) = &native_function.sig.inputs[0] else {
            panic!("generated WIT imports cannot contain receivers");
        };
        let FnArg::Typed(foreign_input) = &foreign_function.sig.inputs[3] else {
            panic!("generated WIT imports cannot contain receivers");
        };

        assert_ne!(
            native_input.ty.to_token_stream().to_string(),
            foreign_input.ty.to_token_stream().to_string(),
            "fixture must exercise semantically equal aliases with different Rust spellings"
        );
        fpi::validate_fpi_signature(native_function, foreign_function).unwrap();
    }

    /// Asserts that a synthetic function either reuses a primitive directly or owns an alias to
    /// the original dependency type.
    fn assert_synthetic_type_alias(
        resolve: &Resolve,
        synthetic_id: InterfaceId,
        source: WitType,
        synthetic: WitType,
    ) {
        let WitType::Id(source_id) = source else {
            assert_eq!(synthetic, source);
            return;
        };
        let WitType::Id(alias_id) = synthetic else {
            panic!("dependency type must be represented by a synthetic alias");
        };
        assert_eq!(resolve.types[alias_id].kind, TypeDefKind::Type(WitType::Id(source_id)));
        assert_eq!(resolve.types[alias_id].owner, TypeOwner::Interface(synthetic_id));
    }

    /// Merges plain and FPI worlds independently of order, repetition, package, or version.
    #[test]
    fn plain_repeated_distinct_and_versioned_fpi_worlds_merge_in_both_orders() {
        const FIRST_IMPORT: &str = "miden:first-dependency/api@1.0.0";
        const SECOND_IMPORT: &str = "miden:second-dependency/api@1.0.0";
        const VERSION_ONE_IMPORT: &str = "miden:versioned-dependency/api@1.0.0";
        const VERSION_TWO_IMPORT: &str = "miden:versioned-dependency/api@2.0.0";

        let cases = [
            (None, Some(vec![FIRST_IMPORT.to_string()])),
            (Some(vec![FIRST_IMPORT.to_string()]), Some(vec![FIRST_IMPORT.to_string()])),
            (Some(vec![FIRST_IMPORT.to_string()]), Some(vec![SECOND_IMPORT.to_string()])),
            (
                Some(vec![VERSION_ONE_IMPORT.to_string()]),
                Some(vec![VERSION_TWO_IMPORT.to_string()]),
            ),
        ];

        for (first, second) in cases {
            for reverse in [false, true] {
                let first = parse_merge_test_world(first.as_deref());
                let second = parse_merge_test_world(second.as_deref());
                let ((mut resolve, into), (other, from)) = if reverse {
                    (second, first)
                } else {
                    (first, second)
                };
                let remap = resolve.merge(other).unwrap();
                let from = remap.map_world(from, Default::default()).unwrap();
                let mut clone_maps = wit_bindgen_core::wit_parser::CloneMaps::default();
                resolve.merge_worlds(from, into, &mut clone_maps).unwrap();
                resolve.assert_valid();
            }
        }
    }

    /// Builds one independently encoded plain-or-FPI world for metadata merge tests.
    fn parse_merge_test_world(imports: Option<&[String]>) -> (Resolve, WorldId) {
        const FIRST_IMPORT: &str = "miden:first-dependency/api@1.0.0";

        let mut resolve = Resolve::default();
        let sdk_group =
            UnresolvedPackageGroup::parse("miden.wit", manifest_paths::SDK_WIT_SOURCE).unwrap();
        resolve.push_group(sdk_group).unwrap();
        for (name, source) in [
            (
                "first-dependency.wit",
                r#"
package miden:first-dependency@1.0.0;
interface api {
    record payload { value: u32 }
    roundtrip: func(value: payload) -> payload;
}
"#,
            ),
            (
                "second-dependency.wit",
                r#"
package miden:second-dependency@1.0.0;
interface api {
    variant payload { none, value(u64) }
    roundtrip: func(value: payload) -> payload;
}
"#,
            ),
            (
                "versioned-dependency-v1.wit",
                r#"
package miden:versioned-dependency@1.0.0;
interface api {
    record payload { value: u32 }
    roundtrip: func(value: payload) -> payload;
}
"#,
            ),
            (
                "versioned-dependency-v2.wit",
                r#"
package miden:versioned-dependency@2.0.0;
interface api {
    record payload { value: u64 }
    roundtrip: func(value: payload) -> payload;
}
"#,
            ),
        ] {
            let group = UnresolvedPackageGroup::parse(name, source).unwrap();
            resolve.push_group(group).unwrap();
        }

        let (source, specs) = match imports {
            Some(imports) => {
                let specs = fpi::import_specs(imports).unwrap();
                let world_name = fpi::import_world_name("foreign-account-bindings", &specs);
                (fpi::import_world_wit(&world_name, &specs), Some(specs))
            }
            None => (
                format!(
                    "package miden:plain-merge-world@1.0.0;\n\nworld plain-merge-world {{\n    \
                     import {FIRST_IMPORT};\n}}\n"
                ),
                None,
            ),
        };
        let group = UnresolvedPackageGroup::parse("inline", &source).unwrap();
        let package = resolve.push_group(group).unwrap();
        let world = resolve.select_world(&[package], None).unwrap();
        if let Some(specs) = specs {
            fpi::inject_imports(&mut resolve, world, &specs).unwrap();
        }

        (resolve, world)
    }
}
