//! Rendering, validation and embedding of the WIT schema documents the SDK macros store in a
//! guest binary, shared by every schema kind (note storage, account storage).

use std::collections::{BTreeSet, HashMap, HashSet};

use midenc_frontend_wasm_metadata::{
    namespace::CORE_TYPES_INTERFACE, pad_to_link_section_alignment,
};
use proc_macro2::{Ident, Literal, Span, TokenStream as TokenStream2};
use quote::quote;
use semver::Version;
use syn::{Type, spanned::Spanned};
use wit_bindgen_core::wit_parser::Resolve;

use crate::{
    generate::CORE_TYPES_INTERFACE_ID,
    manifest_paths::SDK_WIT_SOURCE,
    types::{
        ExportedTypeDef, ExportedTypeKind, TypeRef, custom_type_shape_assertions,
        map_type_to_type_ref, nominal_type_identity_guards,
    },
    wit_builder::{WitBody, WitBuilder},
    wit_names::explicit_wit_identifier,
};

/// SDK core-types package name used for the embedded dependency package.
pub(crate) const CORE_TYPES_PACKAGE_NAME: &str = "miden:base";
/// Component package of a crate without a `miden-project.toml`.
pub(crate) const PLACEHOLDER_COMPONENT_PACKAGE: &str = "miden:empty";

/// The properties that tell one schema kind from another.
pub(crate) struct SchemaKind {
    /// Attribute macro that renders the schema, as written by the user (`#[note]`).
    pub(crate) macro_name: &'static str,
    /// Subject that starts a diagnostic about a type of the schema (`` `#[note]` storage ``).
    pub(crate) subject: &'static str,
    /// Name of the schema in diagnostics (`note storage schema`).
    pub(crate) schema_name: &'static str,
    /// WIT interface that holds the schema types.
    pub(crate) interface: &'static str,
    /// Source name reported for generated schema validation errors.
    pub(crate) source_name: &'static str,
    /// Wasm custom section the schema is embedded in.
    pub(crate) link_section: &'static str,
    /// Supported types, as listed in diagnostics.
    pub(crate) supported_types: &'static str,
    /// WIT primitive types the schema accepts.
    pub(crate) primitives: &'static [&'static str],
}

impl SchemaKind {
    /// Returns the schema name with its indefinite article (`a note storage schema`).
    fn schema_name_with_article(&self) -> String {
        let article = if self.schema_name.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an"
        } else {
            "a"
        };
        format!("{article} {}", self.schema_name)
    }
}

/// A rendered schema and the Rust definitions that produced it, root definition last.
pub(crate) struct RenderedSchema {
    /// Span diagnostics about the rendered schema point at.
    pub(crate) span: Span,
    /// The WIT source.
    pub(crate) source: String,
    /// The referenced custom types followed by the root definition.
    pub(crate) definitions: Vec<ExportedTypeDef>,
}

/// Renders a schema WIT document: the schema package with its one interface, followed by the
/// embedded SDK core-types package.
///
/// `custom_types` are written first, then `root`, then the optional `type <alias> = <root>;`.
pub(crate) fn render_schema_wit(
    kind: &SchemaKind,
    schema_package: &str,
    core_imports: &BTreeSet<String>,
    custom_types: &[ExportedTypeDef],
    root: &ExportedTypeDef,
    root_alias: Option<&str>,
) -> Result<String, syn::Error> {
    let mut wit = WitBuilder::unversioned(kind.macro_name, schema_package);
    if !core_imports.is_empty() {
        wit.use_path(&CORE_TYPES_INTERFACE_ID);
        wit.blank_line();
    }
    wit.interface(kind.interface, |interface| {
        if !core_imports.is_empty() {
            interface.line(&format!(
                "use core-types.{{{}}};",
                core_imports.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            interface.blank_line();
        }

        for custom_type in custom_types {
            render_type_definition(interface, custom_type);
            interface.blank_line();
        }
        render_type_definition(interface, root);
        if let Some(alias) = root_alias {
            interface.blank_line();
            interface.line(&format!("type {alias} = {};", root.wit_name));
        }
    });
    wit.blank_line();
    render_core_types_package(&mut wit)?;
    Ok(wit.finish())
}

/// Validates a rendered schema and emits the items that embed it in the guest binary: identity
/// guards and shape assertions for the types written at the root, and the static `static_ident`
/// holding the NUL-padded schema in the kind's link section.
pub(crate) fn emit_schema(
    kind: &SchemaKind,
    rendered: RenderedSchema,
    registry: &[ExportedTypeDef],
    static_ident: &str,
) -> Result<TokenStream2, syn::Error> {
    validate_rendered_schema(kind, &rendered)?;
    // Identity items reference types as they are written at this macro site, so only the root
    // definition (whose field types are in scope here) is checked. Registry definitions carry
    // their own identity items at their `#[export_type]` sites.
    let root_definition = rendered
        .definitions
        .last()
        .expect("a rendered schema always contains its root definition");
    let identity_guards = nominal_type_identity_guards(root_definition, rendered.span)?;
    let registry_by_rust_name = registry
        .iter()
        .cloned()
        .map(|definition| (definition.rust_name.clone(), definition))
        .collect();
    let shape_assertions =
        custom_type_shape_assertions(root_definition, &registry_by_rust_name, rendered.span)?;
    let schema_static = emit_schema_static(kind.link_section, static_ident, rendered.source);

    Ok(quote! {
        #identity_guards
        #shape_assertions

        #schema_static
    })
}

/// Emits the static `static_ident` holding the NUL-padded `source` in the link section
/// `section_name`.
pub(crate) fn emit_schema_static(
    section_name: &str,
    static_ident: &str,
    source: String,
) -> TokenStream2 {
    let bytes = pad_to_link_section_alignment(source.into_bytes());
    let bytes_len = bytes.len();
    let encoded_bytes = Literal::byte_string(&bytes);
    let macos_link_section = macos_link_section(section_name);
    let static_ident = Ident::new(static_ident, Span::call_site());

    quote! {
        // Mach-O limits section names to 16 bytes. Wasm uses the canonical section name below.
        #[cfg_attr(target_os = "macos", unsafe(link_section = #macos_link_section))]
        #[cfg_attr(
            not(target_os = "macos"),
            unsafe(link_section = #section_name)
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        // No `#[used]`: the section survives without it, and on this target the
        // attribute additionally lands the bytes in the module's data segments,
        // growing every package by the size of its own schema.
        pub static #static_ident: [u8; #bytes_len] = *#encoded_bytes;
    }
}

/// Derives the Mach-O link-section name from a canonical Wasm custom-section name: the segment
/// and the first 16 bytes of the section.
pub(crate) fn macos_link_section(section_name: &str) -> String {
    let (segment, section) = section_name
        .split_once(',')
        .expect("a schema section name must include a segment");
    let section = section
        .get(..section.len().min(16))
        .expect("a schema section name must be ASCII");
    format!("{segment},{section}")
}

/// Maps a Rust type of the schema to WIT syntax with kind-specific diagnostics.
pub(crate) fn map_schema_field_type(
    kind: &SchemaKind,
    ty: &Type,
    registry: &HashMap<String, ExportedTypeDef>,
    context: &str,
) -> Result<TypeRef, syn::Error> {
    if contains_vec(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!("`Vec` is not supported in {}s yet", kind.schema_name),
        ));
    }

    map_type_to_type_ref(ty, registry).map_err(|err| {
        syn::Error::new(
            ty.span(),
            format!(
                "{} {context} is not supported: {err}; supported types are {}",
                kind.subject, kind.supported_types
            ),
        )
    })
}

/// Returns true when a path type names `Vec`, either directly or in an angle-bracketed
/// type argument at any nesting depth.
///
/// The search looks through groups and parentheses. All other type forms, such as
/// references, arrays, slices, tuples, and pointers, give false.
pub(crate) fn contains_vec(ty: &Type) -> bool {
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

/// Checks one record or variant against the type surface of the schema kind.
pub(crate) fn validate_schema_definition(
    kind: &SchemaKind,
    definition: &ExportedTypeDef,
    span: Span,
) -> Result<(), syn::Error> {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                validate_schema_type_ref(
                    kind,
                    &field.ty,
                    span,
                    &format!("field `{}` in type `{}`", field.wit_name, definition.rust_name),
                )?;
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for variant in variants {
                if let Some(payload) = &variant.payload {
                    validate_schema_type_ref(
                        kind,
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

/// Checks one type reference against the type surface of the schema kind.
pub(crate) fn validate_schema_type_ref(
    kind: &SchemaKind,
    type_ref: &TypeRef,
    span: Span,
    context: &str,
) -> Result<(), syn::Error> {
    if type_ref.is_custom
        || type_ref.is_sdk_core_record()
        || kind.primitives.contains(&type_ref.wit_name.as_str())
    {
        return Ok(());
    }

    if type_ref.path.last().is_some_and(|segment| segment == "Option") {
        let [inner] = type_ref.dependencies.as_slice() else {
            return Err(unsupported_schema_type(kind, type_ref, span, context));
        };
        return validate_schema_type_ref(kind, inner, span, context);
    }

    Err(unsupported_schema_type(kind, type_ref, span, context))
}

/// Builds an actionable error for a type outside the type surface of the schema kind.
fn unsupported_schema_type(
    kind: &SchemaKind,
    type_ref: &TypeRef,
    span: Span,
    context: &str,
) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "{} {context} uses unsupported WIT type `{}`; supported types are {}",
            kind.subject, type_ref.wit_name, kind.supported_types,
        ),
    )
}

/// Resolves the generated WIT before it is embedded in the guest binary.
pub(crate) fn validate_rendered_schema(
    kind: &SchemaKind,
    rendered: &RenderedSchema,
) -> Result<(), syn::Error> {
    let mut resolve = Resolve::default();
    resolve
        .push_str(kind.source_name, &rendered.source)
        .map(|_| ())
        .map_err(|error| {
            let message = resolve.render_error(&error);
            let (span, context) = rendered_schema_error_context(kind, rendered, &message);
            syn::Error::new(
                span,
                format!("failed to resolve {} for {context}: {message}", kind.schema_name),
            )
        })
}

/// Finds the Rust definition associated with a WIT parser diagnostic.
fn rendered_schema_error_context(
    kind: &SchemaKind,
    rendered: &RenderedSchema,
    error: &str,
) -> (Span, String) {
    let fallback = rendered
        .definitions
        .last()
        .map(|definition| (rendered.span, format!("type `{}`", definition.rust_name)))
        .unwrap_or_else(|| (Span::call_site(), format!("the {} type", kind.subject)));
    let Some(error_line) = wit_error_line(kind, error).and_then(|line| line.checked_sub(1)) else {
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
                    let field_prefix = format!("{}:", explicit_wit_identifier(&field.wit_name));
                    if error_line.starts_with(&field_prefix) {
                        return (
                            rendered.span,
                            format!(
                                "field `{}` of type `{}` in type `{}`",
                                field.wit_name, field.ty.wit_name, definition.rust_name
                            ),
                        );
                    }
                }
            }
            ExportedTypeKind::Variant { variants } => {
                for variant in variants {
                    if error_line.starts_with(&explicit_wit_identifier(&variant.wit_name)) {
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
fn wit_error_line(kind: &SchemaKind, error: &str) -> Option<usize> {
    let marker = format!("{}:", kind.source_name);
    error.lines().find_map(|line| {
        let (_, location) = line.split_once(&marker)?;
        location.split(':').next()?.parse().ok()
    })
}

/// Returns the registry definitions reachable from the root, in registry order.
pub(crate) fn referenced_custom_types(
    kind: &SchemaKind,
    root: &ExportedTypeDef,
    registry: &[ExportedTypeDef],
    span: Span,
) -> Result<Vec<ExportedTypeDef>, syn::Error> {
    let by_wit_name = registry
        .iter()
        .map(|definition| (definition.wit_name.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut referenced = HashSet::new();
    visit_definition_types(kind, root, &by_wit_name, &mut referenced, span)?;

    Ok(registry
        .iter()
        .filter(|definition| referenced.contains(definition.wit_name.as_str()))
        .cloned()
        .collect())
}

/// Visits every type reference in an exported definition.
fn visit_definition_types(
    kind: &SchemaKind,
    definition: &ExportedTypeDef,
    registry: &HashMap<&str, &ExportedTypeDef>,
    referenced: &mut HashSet<String>,
    span: Span,
) -> Result<(), syn::Error> {
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            for field in fields {
                visit_type_ref(kind, &field.ty, registry, referenced, span)?;
            }
        }
        ExportedTypeKind::Variant { variants } => {
            for payload in variants.iter().filter_map(|variant| variant.payload.as_ref()) {
                visit_type_ref(kind, payload, registry, referenced, span)?;
            }
        }
    }
    Ok(())
}

/// Visits a type reference and its custom-type dependencies.
fn visit_type_ref(
    kind: &SchemaKind,
    type_ref: &TypeRef,
    registry: &HashMap<&str, &ExportedTypeDef>,
    referenced: &mut HashSet<String>,
    span: Span,
) -> Result<(), syn::Error> {
    for dependency in &type_ref.dependencies {
        visit_type_ref(kind, dependency, registry, referenced, span)?;
    }
    if !type_ref.is_custom || referenced.contains(&type_ref.wit_name) {
        return Ok(());
    }

    let definition = registry.get(type_ref.wit_name.as_str()).ok_or_else(|| {
        let rust_name = type_ref.path.last().map(String::as_str).unwrap_or(&type_ref.wit_name);
        syn::Error::new(
            span,
            format!(
                "custom type `{rust_name}` in {} needs #[export_type] on its definition before \
                 the {} struct",
                kind.schema_name_with_article(),
                kind.macro_name
            ),
        )
    })?;
    referenced.insert(type_ref.wit_name.clone());
    visit_definition_types(kind, definition, registry, referenced, span)
}

/// Returns the sorted SDK core types used by the rendered schema definitions.
pub(crate) fn required_core_type_imports(
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
pub(crate) fn render_type_definition(interface: &mut WitBody, definition: &ExportedTypeDef) {
    render_docs(interface, &definition.docs);
    match &definition.kind {
        ExportedTypeKind::Record { fields } => {
            interface.block(&format!("record {} {{", definition.wit_name), |record| {
                for field in fields {
                    render_docs(record, &field.docs);
                    record.line(&format!(
                        "{}: {},",
                        explicit_wit_identifier(&field.wit_name),
                        field.ty.wit_name
                    ));
                }
            });
        }
        ExportedTypeKind::Variant { variants } => {
            interface.block(&format!("variant {} {{", definition.wit_name), |variant_body| {
                for variant in variants {
                    render_docs(variant_body, &variant.docs);
                    match &variant.payload {
                        Some(payload) => variant_body.line(&format!(
                            "{}({}),",
                            explicit_wit_identifier(&variant.wit_name),
                            payload.wit_name
                        )),
                        None => variant_body
                            .line(&format!("{},", explicit_wit_identifier(&variant.wit_name))),
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

/// Writes the embedded SDK `core-types` interface as a braced dependency package.
fn render_core_types_package(wit: &mut WitBuilder) -> Result<(), syn::Error> {
    let body = extract_interface_body(SDK_WIT_SOURCE, CORE_TYPES_INTERFACE).ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
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
    let body = &source[body_start..];
    let bytes = body.as_bytes();
    let mut offset = 0usize;
    let mut in_line_comment = false;
    let mut block_comment_depth = 0usize;
    while offset < bytes.len() {
        if in_line_comment {
            if bytes[offset] == b'\n' {
                in_line_comment = false;
            }
            offset += 1;
            continue;
        }
        if block_comment_depth > 0 {
            if bytes[offset..].starts_with(b"/*") {
                block_comment_depth += 1;
                offset += 2;
            } else if bytes[offset..].starts_with(b"*/") {
                block_comment_depth -= 1;
                offset += 2;
            } else {
                offset += 1;
            }
            continue;
        }
        if bytes[offset..].starts_with(b"//") {
            in_line_comment = true;
            offset += 2;
            continue;
        }
        if bytes[offset..].starts_with(b"/*") {
            block_comment_depth = 1;
            offset += 2;
            continue;
        }
        match bytes[offset] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[body_start..body_start + offset]);
                }
            }
            _ => {}
        }
        offset += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ignores_comment_braces_while_extracting_interface_body() {
        let source =
            "package test:a;\ninterface core-types {\n    /// A closing } before { opening \
             brace.\n    /* Another } before { pair. */\n    record nested {\n        value: \
             u32,\n    }\n}\n";
        let source_without_braces = source.replace("} before {", "before");
        let body = extract_interface_body(source, CORE_TYPES_INTERFACE).unwrap();
        let body_without_braces =
            extract_interface_body(&source_without_braces, CORE_TYPES_INTERFACE).unwrap();

        assert_eq!(body.replace("} before {", "before"), body_without_braces);
        assert!(body.contains("/// A closing } before { opening brace."));
        assert!(body.contains("/* Another } before { pair. */"));
    }
}
