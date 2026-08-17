//! Note storage schema generation for `#[note]` structs.

use std::collections::{BTreeSet, HashMap, HashSet};

use heck::ToKebabCase;
use midenc_frontend_wasm_metadata::WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME;
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::quote;
use semver::Version;
use syn::{ItemStruct, Type, spanned::Spanned};
use wit_bindgen_core::wit_parser::Resolve;

use crate::{
    manifest_paths::SDK_WIT_SOURCE,
    types::{
        ExportedField, ExportedTypeDef, ExportedTypeKind, TypeRef, doc_comments,
        map_type_to_type_ref, registered_export_types, sdk_core_type_identity_guards,
    },
    util::NOTE_NAMED_FIELDS_ERROR,
    wit_builder::{WitBody, WitBuilder},
    wit_world::ManifestPackage,
};

const CORE_TYPES_PACKAGE: &str = "miden:base/core-types@1.0.0";
const CORE_TYPES_PACKAGE_NAME: &str = "miden:base";
const CORE_TYPES_INTERFACE: &str = "core-types";
const NOTE_STORAGE_SCHEMA_SOURCE_NAME: &str = "note-storage-schema.wit";
const NOTE_STORAGE_SUPPORTED_TYPES: &str = "`u64`, `u32`, `u8`, `bool`, SDK core-type records, \
                                            `#[export_type]` records or enums, and `Option<T>` \
                                            over a supported type";
/// Linker symbol used to reject multiple note storage schemas in one crate.
pub(crate) const NOTE_STORAGE_SCHEMA_UNIQUENESS_GUARD_SYMBOL: &str =
    "__MIDEN_NOTE_STORAGE_SCHEMA_UNIQUENESS_GUARD";

/// A rendered note storage schema and the Rust definitions that produced it.
struct RenderedNoteStorageSchema {
    span: Span,
    source: String,
    definitions: Vec<ExportedTypeDef>,
}

/// Generates the note storage schema custom-section static for a named-field note struct.
pub(crate) fn expand_note_storage_schema(
    item_struct: &ItemStruct,
) -> Result<TokenStream2, syn::Error> {
    let package = ManifestPackage::load_or_default(item_struct.ident.span())?;
    let registry = registered_export_types();
    let rendered = render_note_storage_schema_with_registry_model(
        item_struct,
        &package.component_package(),
        package.component_version(),
        &registry,
    )?;
    validate_rendered_note_storage_schema(&rendered)?;
    let identity_guards = rendered
        .definitions
        .iter()
        .map(|definition| sdk_core_type_identity_guards(definition, rendered.span))
        .collect::<Result<Vec<_>, _>>()?;

    let mut bytes = rendered.source.into_bytes();
    let padded_len = bytes.len().div_ceil(16) * 16;
    bytes.resize(padded_len, 0);

    let bytes_len = bytes.len();
    let encoded_bytes = Literal::byte_string(&bytes);

    Ok(quote! {
        #(#identity_guards)*

        // Mach-O limits section names to 16 bytes. Wasm uses the canonical section name below.
        #[cfg_attr(target_os = "macos", unsafe(link_section = "rodata,miden_note_schem"))]
        #[cfg_attr(
            not(target_os = "macos"),
            unsafe(link_section = #WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME)
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_NOTE_STORAGE_SCHEMA_BYTES: [u8; #bytes_len] = *#encoded_bytes;
    })
}

/// Emits a fixed linker symbol that permits one note struct per crate.
pub(crate) fn note_storage_schema_uniqueness_guard() -> TokenStream2 {
    quote! {
        const _: () = {
            // A crate may contain exactly one `#[note]` struct. Reusing a fixed symbol name lets
            // the linker reject duplicates across modules.
            #[doc(hidden)]
            #[used]
            #[unsafe(export_name = #NOTE_STORAGE_SCHEMA_UNIQUENESS_GUARD_SYMBOL)]
            static __miden_note_storage_schema_uniqueness_guard: u8 = 0;
        };
    }
}

/// Renders the note storage schema WIT document for a named-field note struct.
#[cfg(test)]
fn render_note_storage_schema(
    item_struct: &ItemStruct,
    component_package: &str,
    component_version: &Version,
) -> Result<String, syn::Error> {
    let registry = registered_export_types();
    Ok(render_note_storage_schema_with_registry_model(
        item_struct,
        component_package,
        component_version,
        &registry,
    )?
    .source)
}

/// Renders a note storage schema against one export-type registry snapshot.
#[cfg(test)]
fn render_note_storage_schema_with_registry(
    item_struct: &ItemStruct,
    component_package: &str,
    component_version: &Version,
    registry: &[ExportedTypeDef],
) -> Result<String, syn::Error> {
    Ok(render_note_storage_schema_with_registry_model(
        item_struct,
        component_package,
        component_version,
        registry,
    )?
    .source)
}

/// Renders a note storage schema and retains its source-definition context.
fn render_note_storage_schema_with_registry_model(
    item_struct: &ItemStruct,
    component_package: &str,
    component_version: &Version,
    registry: &[ExportedTypeDef],
) -> Result<RenderedNoteStorageSchema, syn::Error> {
    let registry_by_rust_name = registry
        .iter()
        .cloned()
        .map(|definition| (definition.rust_name.clone(), definition))
        .collect();
    let root = note_root_type(item_struct, &registry_by_rust_name)?;
    let custom_types = referenced_custom_types(&root, registry, item_struct.ident.span())?;
    for definition in custom_types.iter().chain([&root]) {
        validate_note_storage_definition(definition, item_struct.ident.span())?;
    }
    let core_imports = required_core_type_imports(&root, &custom_types);
    let schema_package = schema_package_name(component_package);

    let mut wit = WitBuilder::new("#[note]", &schema_package, component_version);
    if !core_imports.is_empty() {
        wit.use_path(CORE_TYPES_PACKAGE);
        wit.blank_line();
    }
    wit.interface("note-storage", |interface| {
        if !core_imports.is_empty() {
            interface.line(&format!(
                "use core-types.{{{}}};",
                core_imports.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            interface.blank_line();
        }

        for custom_type in &custom_types {
            render_type_definition(interface, custom_type);
            interface.blank_line();
        }
        render_type_definition(interface, &root);
        interface.blank_line();
        interface.line(&format!("type storage = {};", root.wit_name));
    });
    wit.blank_line();
    render_core_types_package(&mut wit)?;

    let mut definitions = custom_types;
    definitions.push(root);
    Ok(RenderedNoteStorageSchema {
        span: item_struct.ident.span(),
        source: wit.finish(),
        definitions,
    })
}

/// Builds the exported type definition for the note storage root.
fn note_root_type(
    item_struct: &ItemStruct,
    registry: &HashMap<String, ExportedTypeDef>,
) -> Result<ExportedTypeDef, syn::Error> {
    let syn::Fields::Named(named) = &item_struct.fields else {
        return Err(syn::Error::new(item_struct.fields.span(), NOTE_NAMED_FIELDS_ERROR));
    };

    let mut fields = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let ident = field.ident.as_ref().expect("named fields must have identifiers");
        let context = format!("field `{ident}` in type `{}`", item_struct.ident);
        let ty = map_note_field_type(&field.ty, registry, &context)?;
        validate_note_storage_type_ref(&ty, field.ty.span(), &context)?;
        fields.push(ExportedField {
            docs: doc_comments(&field.attrs),
            name: ident.to_string(),
            ty,
        });
    }

    Ok(ExportedTypeDef {
        docs: doc_comments(&item_struct.attrs),
        rust_name: item_struct.ident.to_string(),
        wit_name: item_struct.ident.to_string().to_kebab_case(),
        kind: ExportedTypeKind::Record { fields },
    })
}

/// Checks one record or variant against the supported note storage type surface.
fn validate_note_storage_definition(
    definition: &ExportedTypeDef,
    span: Span,
) -> Result<(), syn::Error> {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                validate_note_storage_type_ref(
                    &field.ty,
                    span,
                    &format!("field `{}` in type `{}`", field.name, definition.rust_name),
                )?;
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for variant in variants {
                if let Some(payload) = &variant.payload {
                    validate_note_storage_type_ref(
                        payload,
                        span,
                        &format!(
                            "variant `{}` in type `{}`",
                            variant.wit_name, definition.rust_name
                        ),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Checks one type reference against the supported note storage type surface.
fn validate_note_storage_type_ref(
    type_ref: &TypeRef,
    span: Span,
    context: &str,
) -> Result<(), syn::Error> {
    if type_ref.is_custom
        || type_ref.is_sdk_core_record()
        || matches!(type_ref.wit_name.as_str(), "u64" | "u32" | "u8" | "bool")
    {
        return Ok(());
    }

    if type_ref.path.last().is_some_and(|segment| segment == "Option") {
        let [inner] = type_ref.dependencies.as_slice() else {
            return Err(unsupported_note_storage_type(type_ref, span, context));
        };
        return validate_note_storage_type_ref(inner, span, context);
    }

    Err(unsupported_note_storage_type(type_ref, span, context))
}

/// Builds an actionable error for a type outside the note storage surface.
fn unsupported_note_storage_type(type_ref: &TypeRef, span: Span, context: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`#[note]` storage {context} uses unsupported WIT type `{}`; supported types are \
             {NOTE_STORAGE_SUPPORTED_TYPES}",
            type_ref.wit_name,
        ),
    )
}

/// Resolves the generated WIT before it is embedded in the guest binary.
fn validate_rendered_note_storage_schema(
    rendered: &RenderedNoteStorageSchema,
) -> Result<(), syn::Error> {
    let mut resolve = Resolve::default();
    resolve
        .push_str(NOTE_STORAGE_SCHEMA_SOURCE_NAME, &rendered.source)
        .map(|_| ())
        .map_err(|error| {
            let message = format!("{error:#}");
            let (span, context) = rendered_schema_error_context(rendered, &message);
            syn::Error::new(
                span,
                format!("failed to resolve note storage schema for {context}: {message}"),
            )
        })
}

/// Finds the Rust definition associated with a WIT parser diagnostic.
fn rendered_schema_error_context(
    rendered: &RenderedNoteStorageSchema,
    error: &str,
) -> (Span, String) {
    let fallback = rendered
        .definitions
        .last()
        .map(|definition| (rendered.span, format!("type `{}`", definition.rust_name)))
        .unwrap_or_else(|| (Span::call_site(), "the `#[note]` storage type".to_string()));
    let Some(error_line) = wit_error_line(error).and_then(|line| line.checked_sub(1)) else {
        return fallback;
    };
    let Some(error_line) = rendered.source.lines().nth(error_line) else {
        return fallback;
    };
    let error_line = error_line.trim();
    for definition in &rendered.definitions {
        let keyword = match &definition.kind {
            ExportedTypeKind::Record { .. } => "record",
            ExportedTypeKind::Variant { .. } => "variant",
        };
        let header = format!("{keyword} {} {{", definition.wit_name);
        if error_line == header {
            return (rendered.span, format!("type `{}`", definition.rust_name));
        }
        match &definition.kind {
            ExportedTypeKind::Record { fields } => {
                for field in fields {
                    let field_prefix = format!("{}:", field.name.to_kebab_case());
                    if error_line.starts_with(&field_prefix) {
                        return (
                            rendered.span,
                            format!(
                                "field `{}` of type `{}` in type `{}`",
                                field.name, field.ty.wit_name, definition.rust_name
                            ),
                        );
                    }
                }
            }
            ExportedTypeKind::Variant { variants } => {
                for variant in variants {
                    if error_line.starts_with(&variant.wit_name) {
                        let payload = variant
                            .payload
                            .as_ref()
                            .map(|payload| format!(" with payload type `{}`", payload.wit_name))
                            .unwrap_or_default();
                        return (
                            rendered.span,
                            format!(
                                "variant `{}`{payload} in type `{}`",
                                variant.wit_name, definition.rust_name
                            ),
                        );
                    }
                }
            }
        }
    }

    fallback
}

/// Reads the one-based source line from a highlighted WIT parser diagnostic.
fn wit_error_line(error: &str) -> Option<usize> {
    let marker = format!("{NOTE_STORAGE_SCHEMA_SOURCE_NAME}:");
    error.lines().find_map(|line| {
        let (_, location) = line.split_once(&marker)?;
        location.split(':').next()?.parse().ok()
    })
}

/// Maps a Rust field type to WIT syntax with note-specific diagnostics.
fn map_note_field_type(
    ty: &Type,
    registry: &HashMap<String, ExportedTypeDef>,
    context: &str,
) -> Result<TypeRef, syn::Error> {
    if contains_vec(ty) {
        return Err(syn::Error::new(
            ty.span(),
            "`Vec` is not supported in note storage schemas yet",
        ));
    }

    map_type_to_type_ref(ty, registry).map_err(|err| {
        syn::Error::new(
            ty.span(),
            format!(
                "`#[note]` storage {context} is not supported: {err}; supported types are \
                 {NOTE_STORAGE_SUPPORTED_TYPES}"
            ),
        )
    })
}

/// Returns true when a type contains `Vec` at any nesting depth.
fn contains_vec(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => contains_vec(&group.elem),
        Type::Paren(paren) => contains_vec(&paren.elem),
        Type::Path(path) => path.path.segments.iter().any(|segment| {
            if segment.ident == "Vec" {
                return true;
            }
            let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return false;
            };
            arguments.args.iter().any(
                |argument| matches!(argument, syn::GenericArgument::Type(ty) if contains_vec(ty)),
            )
        }),
        _ => false,
    }
}

/// Returns the registry definitions reachable from the root, in registry order.
fn referenced_custom_types(
    root: &ExportedTypeDef,
    registry: &[ExportedTypeDef],
    span: proc_macro2::Span,
) -> Result<Vec<ExportedTypeDef>, syn::Error> {
    let by_wit_name = registry
        .iter()
        .map(|definition| (definition.wit_name.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut referenced = HashSet::new();
    visit_definition_types(root, &by_wit_name, &mut referenced, span)?;

    Ok(registry
        .iter()
        .filter(|definition| referenced.contains(definition.wit_name.as_str()))
        .cloned()
        .collect())
}

/// Visits every type reference in an exported definition.
fn visit_definition_types(
    definition: &ExportedTypeDef,
    registry: &HashMap<&str, &ExportedTypeDef>,
    referenced: &mut HashSet<String>,
    span: proc_macro2::Span,
) -> Result<(), syn::Error> {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                visit_type_ref(&field.ty, registry, referenced, span)?;
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for payload in variants.iter().filter_map(|variant| variant.payload.as_ref()) {
                visit_type_ref(payload, registry, referenced, span)?;
            }
        }
    }
    Ok(())
}

/// Visits a type reference and its custom-type dependencies.
fn visit_type_ref(
    type_ref: &TypeRef,
    registry: &HashMap<&str, &ExportedTypeDef>,
    referenced: &mut HashSet<String>,
    span: proc_macro2::Span,
) -> Result<(), syn::Error> {
    for dependency in &type_ref.dependencies {
        visit_type_ref(dependency, registry, referenced, span)?;
    }
    if !type_ref.is_custom || referenced.contains(&type_ref.wit_name) {
        return Ok(());
    }

    let definition = registry.get(type_ref.wit_name.as_str()).ok_or_else(|| {
        let rust_name = type_ref.path.last().map(String::as_str).unwrap_or(&type_ref.wit_name);
        syn::Error::new(
            span,
            format!(
                "custom type `{rust_name}` in a note storage schema needs #[export_type] on its \
                 definition before the #[note] struct"
            ),
        )
    })?;
    referenced.insert(type_ref.wit_name.clone());
    visit_definition_types(definition, registry, referenced, span)
}

/// Returns the sorted SDK core types used by the rendered schema definitions.
fn required_core_type_imports(
    root: &ExportedTypeDef,
    custom_types: &[ExportedTypeDef],
) -> BTreeSet<String> {
    let mut imports = BTreeSet::new();
    add_definition_core_type_imports(root, &mut imports);
    for definition in custom_types {
        add_definition_core_type_imports(definition, &mut imports);
    }
    imports
}

/// Adds the SDK core types used by one definition to `imports`.
fn add_definition_core_type_imports(definition: &ExportedTypeDef, imports: &mut BTreeSet<String>) {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                field.ty.add_required_core_type_imports(imports);
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for payload in variants.iter().filter_map(|variant| variant.payload.as_ref()) {
                payload.add_required_core_type_imports(imports);
            }
        }
    }
}

/// Writes one record or variant definition to an interface body.
fn render_type_definition(interface: &mut WitBody, definition: &ExportedTypeDef) {
    render_docs(interface, &definition.docs);
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            interface.block(&format!("record {} {{", definition.wit_name), |record| {
                for field in fields {
                    render_docs(record, &field.docs);
                    record.line(&format!("{}: {},", field.name.to_kebab_case(), field.ty.wit_name));
                }
            });
        }
        ExportedTypeKind::Variant { variants } => {
            interface.block(&format!("variant {} {{", definition.wit_name), |variant_body| {
                for variant in variants {
                    render_docs(variant_body, &variant.docs);
                    match &variant.payload {
                        Some(payload) => variant_body
                            .line(&format!("{}({}),", variant.wit_name, payload.wit_name)),
                        None => variant_body.line(&format!("{},", variant.wit_name)),
                    }
                }
            });
        }
    }
}

/// Writes Rust doc attribute text as WIT doc comments.
fn render_docs(body: &mut WitBody, docs: &[String]) {
    for doc in docs {
        for line in doc.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let line = line.strip_prefix(' ').unwrap_or(line);
            if line.is_empty() {
                body.line("///");
            } else {
                body.line(&format!("/// {line}"));
            }
        }
    }
}

/// Appends `-schema` to a component package name before any version suffix.
fn schema_package_name(component_package: &str) -> String {
    match component_package.split_once('@') {
        Some((name, version)) => format!("{name}-schema@{version}"),
        None => format!("{component_package}-schema"),
    }
}

/// Writes the embedded SDK `core-types` interface as a braced dependency package.
fn render_core_types_package(wit: &mut WitBuilder) -> Result<(), syn::Error> {
    let body = extract_interface_body(SDK_WIT_SOURCE, CORE_TYPES_INTERFACE).ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "failed to find the core-types interface in the embedded SDK WIT",
        )
    })?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    let body = body.strip_suffix('\n').unwrap_or(body);

    wit.package_block(CORE_TYPES_PACKAGE_NAME, &Version::new(1, 0, 0), |package| {
        package.line("interface core-types {");
        for line in body.split('\n') {
            package.line(line);
        }
        package.line("}");
    });
    Ok(())
}

/// Extracts a brace-balanced interface body without changing its text.
fn extract_interface_body<'a>(source: &'a str, interface_name: &str) -> Option<&'a str> {
    let header = format!("interface {interface_name} {{");
    let body_start = source.find(&header)? + header.len();
    let mut depth = 1usize;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[body_start..body_start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use midenc_expect_test::expect;
    use syn::parse_quote;
    use wit_bindgen_core::wit_parser::{Resolve, Type as WitType, TypeDefKind};

    use super::*;
    use crate::types::{
        ExportedVariant, exported_type_from_struct, map_type_to_type_ref,
        reset_export_type_registry_for_tests,
    };

    /// Checks that the schema package contains the expected root alias.
    fn assert_schema_root(source: &str, expected_root: &str) {
        let mut resolve = Resolve::default();
        let package_id =
            resolve.push_str("note-schema.wit", source).expect("note schema must parse");
        let package = &resolve.packages[package_id];
        let interface_id = package.interfaces["note-storage"];
        let interface = &resolve.interfaces[interface_id];
        let storage_id = interface.types["storage"];
        let TypeDefKind::Type(WitType::Id(root_id)) = resolve.types[storage_id].kind else {
            panic!("storage must be a named type alias");
        };
        assert_eq!(resolve.types[root_id].name.as_deref(), Some(expected_root));
    }

    #[test]
    fn extracts_nested_interface_body_verbatim() {
        let source = "package test:a;\ninterface target {\n    record nested {\n        value: \
                      u32,\n    }\n}\n";
        assert_eq!(
            extract_interface_body(source, "target"),
            Some("\n    record nested {\n        value: u32,\n    }\n")
        );
        assert!(extract_interface_body(SDK_WIT_SOURCE, CORE_TYPES_INTERFACE).is_some());
    }

    #[test]
    fn uniqueness_guard_expansion_matches_golden() {
        let tokens = note_storage_schema_uniqueness_guard().to_string();

        expect![[r#"const _ : () = { # [doc (hidden)] # [used] # [unsafe (export_name = "__MIDEN_NOTE_STORAGE_SCHEMA_UNIQUENESS_GUARD")] static __miden_note_storage_schema_uniqueness_guard : u8 = 0 ; } ;"#]].assert_eq(&tokens);
    }

    #[test]
    fn renders_p2id_shaped_schema() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct P2idNote {
                target_account_id: AccountId,
            }
        };
        let source = render_note_storage_schema(&note, "miden:p2id", &Version::new(0, 1, 0))
            .expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[note]` macro.
            // Do not edit this file manually.

            package miden:p2id-schema@0.1.0;

            use miden:base/core-types@1.0.0;

            interface note-storage {
                use core-types.{account-id};

                record p2id-note {
                    target-account-id: account-id,
                }

                type storage = p2id-note;
            }

            package miden:base@1.0.0 {
                interface core-types {
                    /// Represents an on-chain felt.
                    ///
                    /// Field modulus M = 2^64 - 2^32 + 1.
                    record felt {
                        /// The backing type is `f32` which will be treated as a felt by the compiler.
                	/// We're basically hijacking the Wasm `f32` type and treat as felt.
                        inner: f32,
                    }


                    /// A group of four field elements in the Miden base field.
                    record word {
                        a: felt,
                	b: felt,
                	c: felt,
                	d: felt,
                    }

                    /// A cryptographic digest representing a 256-bit hash value.
                    /// This is a wrapper around `word` which contains 4 field elements.
                    record digest {
                        inner: word
                    }

                    /// Unique identifier of an account.
                    ///
                    /// # Layout
                    ///
                    /// An `AccountId` consists of two field elements, where the first is called the prefix and the
                    /// second is called the suffix. It is laid out as follows:
                    ///
                    /// prefix: [hash (56 bits) | storage mode (2 bits) | type (2 bits) | version (4 bits)]
                    /// suffix: [zero bit | hash (55 bits) | 8 zero bits]
                    record account-id {
                    	prefix: felt,
                	suffix: felt
                    }

                    /// Creates a new account ID from a field element.
                    //account-id-from-felt: func(felt: felt) -> account-id;

                    /// Recipient of the note, i.e., hash(hash(hash(serial_num, [0; 4]), note_script_hash), input_hash)
                    record recipient {
                        inner: word
                    }

                    record tag {
                        inner: felt
                    }

                    /// A fungible or a non-fungible asset.
                    ///
                    /// In protocol v0.14 assets are encoded as two words: an asset key and an asset value.
                    ///
                    /// The methodology for constructing fungible and non-fungible assets is described below.
                    ///
                    /// # Fungible assets
                    /// - `key`: `[0, 0, faucet_id_suffix, faucet_id_prefix]`
                    /// - `value`: `[amount, 0, 0, 0]`
                    ///
                    /// # Non-fungible assets
                    /// - `key`: `[hash0, hash1, faucet_id_suffix, faucet_id_prefix]`
                    /// - `value`: `DATA_HASH`
                    record asset {
                        key: word,
                        value: word,
                    }

                    /// A validated fungible asset amount, at most 2^63 - 2^31.
                    record asset-amount {
                        inner: felt
                    }

                    /// Account nonce
                    record nonce {
                        inner: felt
                    }

                    /// A block height in the chain
                    record block-number {
                        inner: felt
                    }

                    /// Account hash
                    record account-hash {
                        inner: word
                    }

                    /// Block hash
                    record block-hash {
                        inner: word
                    }

                    /// Storage value
                    record storage-value {
                        inner: word
                    }

                    /// Account storage root
                    record storage-root {
                        inner: word
                    }

                    /// Account code root
                    record account-code-root {
                        inner: word
                    }

                    /// Commitment to the account vault
                    record vault-commitment {
                        inner: word
                    }

                    /// An index of the created note
                    record note-idx {
                        inner: felt
                    }

                    record note-type {
                        inner: felt
                    }

                    record note-execution-hint {
                        inner: felt
                    }

                }
            }
        "#]].assert_eq(&source);
        assert_schema_root(&source, "p2id-note");
    }

    #[test]
    fn renders_nested_record_and_enum_schema() {
        reset_export_type_registry_for_tests();
        let destination: syn::ItemStruct = parse_quote! {
            /// Destination details.
            struct Destination {
                /// Destination account.
                account_id: AccountId,
            }
        };
        let route: syn::ItemEnum = parse_quote! {
            /// Route selection.
            enum Route {
                /// Send directly.
                Direct,
                /// Send through a destination.
                Via(Destination),
            }
        };
        let destination = exported_type_from_struct(&destination).expect("record must map");
        let destination_type: Type = parse_quote!(Destination);
        let route_payload = map_type_to_type_ref(
            &destination_type,
            &HashMap::from([(destination.rust_name.clone(), destination.clone())]),
        )
        .expect("enum payload must map");
        let route = ExportedTypeDef {
            docs: doc_comments(&route.attrs),
            rust_name: route.ident.to_string(),
            wit_name: route.ident.to_string().to_kebab_case(),
            kind: ExportedTypeKind::Variant {
                variants: vec![
                    ExportedVariant {
                        docs: doc_comments(&route.variants[0].attrs),
                        wit_name: "direct".into(),
                        payload: None,
                    },
                    ExportedVariant {
                        docs: doc_comments(&route.variants[1].attrs),
                        wit_name: "via".into(),
                        payload: Some(route_payload),
                    },
                ],
            },
        };
        let note: ItemStruct = parse_quote! {
            /// A routed note.
            struct RoutedNote {
                /// Selected route.
                route: Route,
            }
        };
        let source = render_note_storage_schema_with_registry(
            &note,
            "miden:routed-note",
            &Version::new(2, 3, 4),
            &[destination, route],
        )
        .expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[note]` macro.
            // Do not edit this file manually.

            package miden:routed-note-schema@2.3.4;

            use miden:base/core-types@1.0.0;

            interface note-storage {
                use core-types.{account-id};

                /// Destination details.
                record destination {
                    /// Destination account.
                    account-id: account-id,
                }

                /// Route selection.
                variant route {
                    /// Send directly.
                    direct,
                    /// Send through a destination.
                    via(destination),
                }

                /// A routed note.
                record routed-note {
                    /// Selected route.
                    route: route,
                }

                type storage = routed-note;
            }

            package miden:base@1.0.0 {
                interface core-types {
                    /// Represents an on-chain felt.
                    ///
                    /// Field modulus M = 2^64 - 2^32 + 1.
                    record felt {
                        /// The backing type is `f32` which will be treated as a felt by the compiler.
                	/// We're basically hijacking the Wasm `f32` type and treat as felt.
                        inner: f32,
                    }


                    /// A group of four field elements in the Miden base field.
                    record word {
                        a: felt,
                	b: felt,
                	c: felt,
                	d: felt,
                    }

                    /// A cryptographic digest representing a 256-bit hash value.
                    /// This is a wrapper around `word` which contains 4 field elements.
                    record digest {
                        inner: word
                    }

                    /// Unique identifier of an account.
                    ///
                    /// # Layout
                    ///
                    /// An `AccountId` consists of two field elements, where the first is called the prefix and the
                    /// second is called the suffix. It is laid out as follows:
                    ///
                    /// prefix: [hash (56 bits) | storage mode (2 bits) | type (2 bits) | version (4 bits)]
                    /// suffix: [zero bit | hash (55 bits) | 8 zero bits]
                    record account-id {
                    	prefix: felt,
                	suffix: felt
                    }

                    /// Creates a new account ID from a field element.
                    //account-id-from-felt: func(felt: felt) -> account-id;

                    /// Recipient of the note, i.e., hash(hash(hash(serial_num, [0; 4]), note_script_hash), input_hash)
                    record recipient {
                        inner: word
                    }

                    record tag {
                        inner: felt
                    }

                    /// A fungible or a non-fungible asset.
                    ///
                    /// In protocol v0.14 assets are encoded as two words: an asset key and an asset value.
                    ///
                    /// The methodology for constructing fungible and non-fungible assets is described below.
                    ///
                    /// # Fungible assets
                    /// - `key`: `[0, 0, faucet_id_suffix, faucet_id_prefix]`
                    /// - `value`: `[amount, 0, 0, 0]`
                    ///
                    /// # Non-fungible assets
                    /// - `key`: `[hash0, hash1, faucet_id_suffix, faucet_id_prefix]`
                    /// - `value`: `DATA_HASH`
                    record asset {
                        key: word,
                        value: word,
                    }

                    /// A validated fungible asset amount, at most 2^63 - 2^31.
                    record asset-amount {
                        inner: felt
                    }

                    /// Account nonce
                    record nonce {
                        inner: felt
                    }

                    /// A block height in the chain
                    record block-number {
                        inner: felt
                    }

                    /// Account hash
                    record account-hash {
                        inner: word
                    }

                    /// Block hash
                    record block-hash {
                        inner: word
                    }

                    /// Storage value
                    record storage-value {
                        inner: word
                    }

                    /// Account storage root
                    record storage-root {
                        inner: word
                    }

                    /// Account code root
                    record account-code-root {
                        inner: word
                    }

                    /// Commitment to the account vault
                    record vault-commitment {
                        inner: word
                    }

                    /// An index of the created note
                    record note-idx {
                        inner: felt
                    }

                    record note-type {
                        inner: felt
                    }

                    record note-execution-hint {
                        inner: felt
                    }

                }
            }
        "#]].assert_eq(&source);
        assert_schema_root(&source, "routed-note");
    }

    #[test]
    fn renders_multiline_doc_attributes_as_separate_wit_comments() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            #[doc = " First note line.\n Second note line."]
            struct DocumentedNote {
                #[doc = " First field line.\n Second field line."]
                value: u64,
            }
        };
        let source =
            render_note_storage_schema(&note, "miden:documented-note", &Version::new(1, 0, 0))
                .expect("multiline docs must render");

        assert!(source.contains("/// First note line.\n    /// Second note line."));
        assert!(source.contains("/// First field line.\n        /// Second field line."));
        assert_schema_root(&source, "documented-note");
    }

    #[test]
    fn rejects_signed_integer_storage_fields() {
        for (rust_type, wit_type) in [("i8", "s8"), ("i16", "s16"), ("i32", "s32"), ("i64", "s64")]
        {
            assert_unsupported_note_field_type(rust_type, wit_type);
        }
    }

    #[test]
    fn rejects_u16_storage_fields() {
        assert_unsupported_note_field_type("u16", "u16");
    }

    #[test]
    fn rejects_result_storage_fields() {
        assert_unsupported_note_field_type("Result<u64, u32>", "result<u64, u32>");
    }

    #[test]
    fn rejects_other_types_outside_the_storage_allow_list() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct FloatNote {
                value: f32,
            }
        };
        let err = render_note_storage_schema_with_registry(
            &note,
            "miden:float-note",
            &Version::new(1, 0, 0),
            &[],
        )
        .expect_err("types outside the note storage surface must fail");

        let message = err.to_string();
        assert!(message.contains("field `value` in type `FloatNote`"));
        assert!(message.contains("`f32` is not supported"));
        assert!(message.contains("supported types are"));
    }

    #[test]
    fn rejects_unsupported_types_nested_in_options() {
        assert_unsupported_note_field_type("Option<i16>", "s16");
    }

    #[test]
    fn rejects_unsupported_fields_in_nested_records() {
        reset_export_type_registry_for_tests();
        let nested: syn::ItemStruct = parse_quote! {
            struct Nested {
                count: u16,
            }
        };
        let nested = exported_type_from_struct(&nested).expect("record must map");
        let note: ItemStruct = parse_quote! {
            struct NestedNote {
                nested: Nested,
            }
        };
        let err = render_note_storage_schema_with_registry(
            &note,
            "miden:nested-note",
            &Version::new(1, 0, 0),
            &[nested],
        )
        .expect_err("unsupported nested fields must fail");

        let message = err.to_string();
        assert!(message.contains("field `count` in type `Nested`"));
        assert!(message.contains("unsupported WIT type `u16`"));
    }

    #[test]
    fn expansion_surfaces_wit_parser_errors_with_type_context() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct Type {
                value: u64,
            }
        };
        let err = expand_note_storage_schema(&note)
            .expect_err("a WIT keyword cannot be used as a record name");

        let message = err.to_string();
        assert!(message.contains("failed to resolve note storage schema"));
        assert!(message.contains("type `Type`"));
    }

    #[test]
    fn expansion_surfaces_wit_parser_errors_with_field_context() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct InvalidFieldNote {
                type_: u64,
            }
        };
        let err = expand_note_storage_schema(&note)
            .expect_err("a WIT keyword cannot be used as a field name");

        let message = err.to_string();
        assert!(message.contains("failed to resolve note storage schema"));
        assert!(message.contains("field `type_` of type `u64`"));
    }

    #[test]
    fn rejects_tuple_note_structs() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote!(
            struct TupleNote(Felt);
        );
        let err = render_note_storage_schema(&note, "miden:tuple-note", &Version::new(1, 0, 0))
            .expect_err("tuple notes must fail");

        assert_eq!(err.to_string(), NOTE_NAMED_FIELDS_ERROR);
    }

    #[test]
    fn rejects_vec_fields() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct VecNote {
                values: Vec<Felt>,
            }
        };
        let err = render_note_storage_schema(&note, "miden:vec-note", &Version::new(1, 0, 0))
            .expect_err("Vec fields must fail");

        assert_eq!(err.to_string(), "`Vec` is not supported in note storage schemas yet");
    }

    #[test]
    fn rejects_custom_types_registered_after_the_note() {
        reset_export_type_registry_for_tests();
        let note: ItemStruct = parse_quote! {
            struct CustomNote {
                value: MissingType,
            }
        };
        let err = render_note_storage_schema(&note, "miden:custom-note", &Version::new(1, 0, 0))
            .expect_err("unregistered custom types must fail");

        let message = err.to_string();
        assert!(message.contains("#[export_type]"));
        assert!(message.contains("before the #[note] struct"));
    }

    /// Checks one Rust field type against the note-specific allow-list diagnostic.
    fn assert_unsupported_note_field_type(rust_type: &str, expected_wit_type: &str) {
        reset_export_type_registry_for_tests();
        let note: ItemStruct =
            syn::parse_str(&format!("struct UnsupportedNote {{ value: {rust_type}, }}"))
                .expect("test note must parse");
        let err = render_note_storage_schema_with_registry(
            &note,
            "miden:unsupported-note",
            &Version::new(1, 0, 0),
            &[],
        )
        .expect_err("unsupported note field must fail");

        let message = err.to_string();
        assert!(message.contains("field `value` in type `UnsupportedNote`"));
        assert!(message.contains(&format!("unsupported WIT type `{expected_wit_type}`")));
        assert!(message.contains("supported types are"));
    }
}
