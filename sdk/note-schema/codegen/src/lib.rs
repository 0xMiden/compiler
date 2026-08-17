//! Shared Rust code generation for resolved note storage schemas.

#![deny(missing_docs)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use heck::{ToSnakeCase, ToUpperCamelCase};
use miden_note_schema::{
    NoteStorageSchema, PrimitiveType, SchemaCase, SchemaField, SchemaType, SchemaTypeKind,
    StandardLeaf,
};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};

/// An error reported while generating Rust bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenError {
    message: String,
}

impl CodegenError {
    /// Creates a code-generation error.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Rust host types and metadata produced from one note storage schema.
pub struct GeneratedTypes {
    tokens: TokenStream,
    root_ident: Ident,
    type_idents: Vec<Ident>,
    has_nested_named_types: bool,
}

impl GeneratedTypes {
    /// Returns the generated Rust items.
    pub const fn tokens(&self) -> &TokenStream {
        &self.tokens
    }

    /// Returns the Rust identifier for the root storage type.
    pub const fn root_ident(&self) -> &Ident {
        &self.root_ident
    }

    /// Returns every generated Rust type identifier in emission order.
    pub fn type_idents(&self) -> &[Ident] {
        &self.type_idents
    }

    /// Returns true when the schema has a generated named type below its storage root.
    pub const fn has_nested_named_types(&self) -> bool {
        self.has_nested_named_types
    }
}

/// Runtime crate paths used by generated host-profile code.
#[derive(Clone)]
pub struct RuntimePaths {
    miden_field: TokenStream,
    miden_field_repr: TokenStream,
    miden_note_schema: TokenStream,
    miden_protocol: TokenStream,
}

impl RuntimePaths {
    /// Creates a runtime path set from four crate or module paths.
    pub fn new(
        miden_field: TokenStream,
        miden_field_repr: TokenStream,
        miden_note_schema: TokenStream,
        miden_protocol: TokenStream,
    ) -> Self {
        Self {
            miden_field,
            miden_field_repr,
            miden_note_schema,
            miden_protocol,
        }
    }

    /// Creates paths through a facade crate's hidden runtime re-exports.
    pub fn through_facade(facade: TokenStream) -> Self {
        Self::new(
            quote!(#facade::__private::miden_field),
            quote!(#facade::__private::miden_field_repr),
            quote!(#facade::__private::miden_note_schema),
            quote!(#facade::__private::miden_protocol),
        )
    }
}

impl Default for RuntimePaths {
    fn default() -> Self {
        Self::new(
            quote!(::miden_field),
            quote!(::miden_field_repr),
            quote!(::miden_note_schema),
            quote!(::miden_protocol),
        )
    }
}

/// Generates Rust host-profile types and structural felt conversion helpers.
pub fn generate_host_types(
    schema: &NoteStorageSchema,
    runtime: &RuntimePaths,
) -> Result<GeneratedTypes, CodegenError> {
    schema
        .validate_native_leaf_shapes()
        .map_err(|error| CodegenError::new(error.to_string()))?;
    let root = schema.root();
    let root_fqn = root
        .fqn()
        .ok_or_else(|| CodegenError::new("the note storage root does not have a WIT FQN"))?;
    let root_name = root
        .name()
        .ok_or_else(|| CodegenError::new("the note storage root does not have a WIT name"))?;

    let mut definitions = Vec::new();
    let mut seen = BTreeSet::new();
    collect_named_types(root, &mut seen, &mut definitions)?;

    let mut rust_names = BTreeMap::new();
    let mut used_names = BTreeMap::<String, String>::new();
    for definition in &definitions {
        let fqn = definition.fqn().expect("collected named types always have a FQN");
        let name = definition.name().expect("collected named types always have a name");
        let ident = type_ident(name);
        if let Some(existing) = used_names.insert(ident.to_string(), fqn.to_owned())
            && existing != fqn
        {
            return Err(CodegenError::new(format!(
                "WIT types `{existing}` and `{fqn}` both map to Rust type `{ident}`"
            )));
        }
        rust_names.insert(fqn.to_owned(), ident);
    }

    let root_ident = rust_names.get(root_fqn).cloned().unwrap_or_else(|| type_ident(root_name));
    let helper_traits = generate_helper_traits(runtime);
    let items = definitions
        .iter()
        .map(|definition| generate_type(definition, &rust_names, runtime))
        .collect::<Result<Vec<_>, _>>()?;
    let type_idents = definitions
        .iter()
        .map(|definition| {
            rust_names
                .get(definition.fqn().expect("generated types have an FQN"))
                .expect("generated types have a Rust name")
                .clone()
        })
        .collect();
    let has_nested_named_types = definitions
        .iter()
        .any(|definition| definition.fqn().is_some_and(|fqn| fqn != root_fqn));

    Ok(GeneratedTypes {
        tokens: quote! {
            #helper_traits
            #(#items)*
        },
        root_ident,
        type_idents,
        has_nested_named_types,
    })
}

/// Collects reachable named records and variants, excluding mapped protocol leaves.
fn collect_named_types<'a>(
    ty: &'a SchemaType,
    seen: &mut BTreeSet<String>,
    definitions: &mut Vec<&'a SchemaType>,
) -> Result<(), CodegenError> {
    if ty.standard_leaf().is_some() {
        return Ok(());
    }

    let is_definition = matches!(ty.kind(), SchemaTypeKind::Record(_) | SchemaTypeKind::Variant(_));
    if is_definition {
        let fqn = ty.fqn().ok_or_else(|| {
            CodegenError::new("anonymous WIT records and variants cannot be emitted as Rust types")
        })?;
        ty.name().ok_or_else(|| {
            CodegenError::new(format!("WIT type `{fqn}` does not have a local name"))
        })?;
        if !seen.insert(fqn.to_owned()) {
            return Ok(());
        }
        definitions.push(ty);
    }

    match ty.kind() {
        SchemaTypeKind::Record(fields) => {
            for field in fields {
                collect_named_types(field.ty(), seen, definitions)?;
            }
        }
        SchemaTypeKind::Option(payload) => collect_named_types(payload, seen, definitions)?,
        SchemaTypeKind::Variant(cases) => {
            for payload in cases.iter().filter_map(SchemaCase::payload) {
                collect_named_types(payload, seen, definitions)?;
            }
        }
        SchemaTypeKind::Felt | SchemaTypeKind::Primitive(_) => {}
    }
    Ok(())
}

/// Generates the private traits that keep protocol-leaf order separate from foreign trait impls.
fn generate_helper_traits(runtime: &RuntimePaths) -> TokenStream {
    let RuntimePaths {
        miden_field,
        miden_field_repr,
        miden_note_schema,
        miden_protocol,
    } = runtime;
    let primitive_impls = [
        quote!(u64),
        quote!(u32),
        quote!(u8),
        quote!(bool),
        quote!(#miden_field::Felt),
        quote!(#miden_field::Word),
    ]
    .into_iter()
    .map(|ty| {
        quote! {
            impl __MidenNoteEncode for #ty {
                fn __write_note_felts(
                    &self,
                    writer: &mut #miden_field_repr::FeltWriter<'_>,
                ) -> #miden_note_schema::Result<()> {
                    #miden_field_repr::ToFeltRepr::write_felt_repr(self, writer);
                    Ok(())
                }
            }

            impl __MidenNoteDecode for #ty {
                fn __read_note_felts(
                    reader: &mut #miden_field_repr::FeltReader<'_>,
                ) -> #miden_note_schema::Result<Self> {
                    #miden_field_repr::FromFeltRepr::from_felt_repr(reader).map_err(|error| {
                        #miden_note_schema::Error::new(format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(#ty),
                        ))
                    })
                }
            }
        }
    });

    quote! {
        #[doc(hidden)]
        trait __MidenNoteEncode {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()>;
        }

        #[doc(hidden)]
        trait __MidenNoteDecode: Sized {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self>;
        }

        #(#primitive_impls)*

        impl __MidenNoteEncode for #miden_protocol::account::AccountId {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()> {
                // The WIT record declares prefix before suffix.
                writer.write(self.prefix().as_felt());
                writer.write(self.suffix());
                Ok(())
            }
        }

        impl __MidenNoteDecode for #miden_protocol::account::AccountId {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self> {
                let prefix = reader.read().map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "failed to decode account-id prefix: {error}"
                    ))
                })?;
                let suffix = reader.read().map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "failed to decode account-id suffix: {error}"
                    ))
                })?;
                #miden_protocol::account::AccountId::try_from_elements(suffix, prefix).map_err(
                    |error| {
                        #miden_note_schema::Error::new(format!(
                            "invalid account-id in note storage: {error}"
                        ))
                    },
                )
            }
        }

        impl __MidenNoteEncode for #miden_protocol::asset::AssetAmount {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()> {
                writer.write(#miden_field::Felt::from(*self));
                Ok(())
            }
        }

        impl __MidenNoteDecode for #miden_protocol::asset::AssetAmount {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self> {
                let value = reader.read().map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "failed to decode asset-amount: {error}"
                    ))
                })?;
                #miden_protocol::asset::AssetAmount::try_from(value).map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "invalid asset-amount in note storage: {error}"
                    ))
                })
            }
        }

        impl<T> __MidenNoteEncode for Option<T>
        where
            T: __MidenNoteEncode,
        {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()> {
                match self {
                    None => writer.write(#miden_field::Felt::ZERO),
                    Some(value) => {
                        writer.write(#miden_field::Felt::ONE);
                        value.__write_note_felts(writer)?;
                    }
                }
                Ok(())
            }
        }

        impl<T> __MidenNoteDecode for Option<T>
        where
            T: __MidenNoteDecode,
        {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self> {
                let tag = reader.read().map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "failed to decode option tag: {error}"
                    ))
                })?;
                match tag.as_canonical_u64() {
                    0 => Ok(None),
                    1 => Ok(Some(T::__read_note_felts(reader)?)),
                    tag => Err(#miden_note_schema::Error::new(format!(
                        "invalid option tag {tag}; expected 0 or 1"
                    ))),
                }
            }
        }
    }
}

/// Generates one Rust record or variant and its structural conversion helpers.
fn generate_type(
    definition: &SchemaType,
    rust_names: &BTreeMap<String, Ident>,
    runtime: &RuntimePaths,
) -> Result<TokenStream, CodegenError> {
    let fqn = definition.fqn().expect("generated type definitions always have a FQN");
    let ident = rust_names.get(fqn).expect("every generated type has a Rust identifier");
    let docs = type_docs(definition, fqn);
    let derives = if supports_native_felt_repr(definition) {
        let miden_field_repr = &runtime.miden_field_repr;
        let crate_path = Literal::string(&miden_field_repr.to_string().replace(' ', ""));
        quote! {
            #[derive(
                Clone,
                Debug,
                PartialEq,
                Eq,
                #miden_field_repr::ToFeltRepr,
                #miden_field_repr::FromFeltRepr,
            )]
            #[felt_repr(crate_path = #crate_path)]
        }
    } else {
        quote! { #[derive(Clone, Debug, PartialEq, Eq)] }
    };

    let (item, encode_impl, decode_impl) = match definition.kind() {
        SchemaTypeKind::Record(fields) => {
            generate_record(ident, fields, rust_names, &docs, &derives, runtime)?
        }
        SchemaTypeKind::Variant(cases) => {
            generate_variant(ident, cases, rust_names, &docs, &derives, runtime)?
        }
        _ => {
            return Err(CodegenError::new(format!(
                "named WIT type `{fqn}` is not a record or variant"
            )));
        }
    };

    Ok(quote! {
        #item

        impl #ident {
            /// The canonical fully-qualified WIT name for this type.
            pub const WIT_FQN: &'static str = #fqn;
        }

        #encode_impl
        #decode_impl
    })
}

/// Generates a Rust struct in WIT declaration order.
fn generate_record(
    ident: &Ident,
    fields: &[SchemaField],
    rust_names: &BTreeMap<String, Ident>,
    docs: &TokenStream,
    derives: &TokenStream,
    runtime: &RuntimePaths,
) -> Result<(TokenStream, TokenStream, TokenStream), CodegenError> {
    let miden_field_repr = &runtime.miden_field_repr;
    let miden_note_schema = &runtime.miden_note_schema;
    let rust_fields = fields
        .iter()
        .map(|field| {
            let ident = value_ident(field.name());
            let ty = rust_type(field.ty(), rust_names, runtime)?;
            let docs = field_docs(field);
            Ok((ident, ty, docs))
        })
        .collect::<Result<Vec<_>, CodegenError>>()?;
    let field_idents = rust_fields.iter().map(|(ident, ..)| ident).collect::<Vec<_>>();
    let field_types = rust_fields.iter().map(|(_, ty, _)| ty).collect::<Vec<_>>();
    let field_docs = rust_fields.iter().map(|(_, _, docs)| docs).collect::<Vec<_>>();

    let item = quote! {
        #docs
        #derives
        pub struct #ident {
            #(
                #field_docs
                pub #field_idents: #field_types,
            )*
        }
    };
    let encode_impl = quote! {
        impl __MidenNoteEncode for #ident {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()> {
                #(self.#field_idents.__write_note_felts(writer)?;)*
                Ok(())
            }
        }
    };
    let decode_impl = quote! {
        impl __MidenNoteDecode for #ident {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self> {
                Ok(Self {
                    #(#field_idents: <#field_types as __MidenNoteDecode>::__read_note_felts(reader)?,)*
                })
            }
        }
    };
    Ok((item, encode_impl, decode_impl))
}

/// Generates a Rust enum with declaration-ordinal tags.
fn generate_variant(
    ident: &Ident,
    cases: &[SchemaCase],
    rust_names: &BTreeMap<String, Ident>,
    docs: &TokenStream,
    derives: &TokenStream,
    runtime: &RuntimePaths,
) -> Result<(TokenStream, TokenStream, TokenStream), CodegenError> {
    let miden_field = &runtime.miden_field;
    let miden_field_repr = &runtime.miden_field_repr;
    let miden_note_schema = &runtime.miden_note_schema;
    let rust_cases = cases
        .iter()
        .map(|case| {
            let ident = type_ident(case.name());
            let payload =
                case.payload().map(|ty| rust_type(ty, rust_names, runtime)).transpose()?;
            let docs = case_docs(case);
            Ok((ident, payload, docs))
        })
        .collect::<Result<Vec<_>, CodegenError>>()?;

    let declarations = rust_cases.iter().map(|(case, payload, docs)| match payload {
        Some(payload) => quote! {
            #docs
            #case(#payload),
        },
        None => quote! {
            #docs
            #case,
        },
    });
    let encode_arms = rust_cases.iter().enumerate().map(|(ordinal, (case, payload, _))| {
        let ordinal = ordinal as u32;
        match payload {
            Some(_) => quote! {
                Self::#case(value) => {
                    writer.write(#miden_field::Felt::from_u32(#ordinal));
                    value.__write_note_felts(writer)?;
                }
            },
            None => quote! {
                Self::#case => {
                    writer.write(#miden_field::Felt::from_u32(#ordinal));
                }
            },
        }
    });
    let decode_arms = rust_cases.iter().enumerate().map(|(ordinal, (case, payload, _))| {
        let ordinal = ordinal as u32;
        match payload {
            Some(payload) => quote! {
                #ordinal => Ok(Self::#case(
                    <#payload as __MidenNoteDecode>::__read_note_felts(reader)?,
                )),
            },
            None => quote! { #ordinal => Ok(Self::#case), },
        }
    });
    let case_count = cases.len();

    let item = quote! {
        #docs
        #derives
        pub enum #ident {
            #(#declarations)*
        }
    };
    let encode_impl = quote! {
        impl __MidenNoteEncode for #ident {
            fn __write_note_felts(
                &self,
                writer: &mut #miden_field_repr::FeltWriter<'_>,
            ) -> #miden_note_schema::Result<()> {
                match self {
                    #(#encode_arms)*
                }
                Ok(())
            }
        }
    };
    let decode_impl = quote! {
        impl __MidenNoteDecode for #ident {
            fn __read_note_felts(
                reader: &mut #miden_field_repr::FeltReader<'_>,
            ) -> #miden_note_schema::Result<Self> {
                let tag = reader.read_u32().map_err(|error| {
                    #miden_note_schema::Error::new(format!(
                        "failed to decode {} tag: {error}",
                        stringify!(#ident),
                    ))
                })?;
                match tag {
                    #(#decode_arms)*
                    tag => Err(#miden_note_schema::Error::new(format!(
                        "invalid {} tag {tag}; expected a declaration ordinal below {}",
                        stringify!(#ident),
                        #case_count,
                    ))),
                }
            }
        }
    };
    Ok((item, encode_impl, decode_impl))
}

/// Maps one schema type to its host-profile Rust type.
fn rust_type(
    ty: &SchemaType,
    rust_names: &BTreeMap<String, Ident>,
    runtime: &RuntimePaths,
) -> Result<TokenStream, CodegenError> {
    if let Some(mapped) = mapped_leaf(ty, runtime) {
        return Ok(mapped);
    }
    if let Some(fqn) = ty.fqn()
        && let Some(ident) = rust_names.get(fqn)
    {
        return Ok(quote!(#ident));
    }

    match ty.kind() {
        SchemaTypeKind::Primitive(PrimitiveType::U64) => Ok(quote!(u64)),
        SchemaTypeKind::Primitive(PrimitiveType::U32) => Ok(quote!(u32)),
        SchemaTypeKind::Primitive(PrimitiveType::U8) => Ok(quote!(u8)),
        SchemaTypeKind::Primitive(PrimitiveType::Bool) => Ok(quote!(bool)),
        SchemaTypeKind::Option(payload) => {
            let payload = rust_type(payload, rust_names, runtime)?;
            Ok(quote!(Option<#payload>))
        }
        SchemaTypeKind::Record(_) | SchemaTypeKind::Variant(_) => Err(CodegenError::new(format!(
            "named WIT type `{}` was not collected for Rust generation",
            ty.fqn().or(ty.name()).unwrap_or("<anonymous>")
        ))),
        SchemaTypeKind::Felt => {
            let miden_field = &runtime.miden_field;
            Ok(quote!(#miden_field::Felt))
        }
    }
}

/// Returns the native Rust type for one standard WIT leaf.
fn mapped_leaf(ty: &SchemaType, runtime: &RuntimePaths) -> Option<TokenStream> {
    let RuntimePaths {
        miden_field,
        miden_protocol,
        ..
    } = runtime;
    match ty.standard_leaf()? {
        StandardLeaf::Felt => Some(quote!(#miden_field::Felt)),
        StandardLeaf::Word => Some(quote!(#miden_field::Word)),
        StandardLeaf::AccountId => Some(quote!(#miden_protocol::account::AccountId)),
        StandardLeaf::AssetAmount => Some(quote!(#miden_protocol::asset::AssetAmount)),
    }
}

/// Returns true when all fields implement the native felt-repr traits without protocol adapters.
fn supports_native_felt_repr(ty: &SchemaType) -> bool {
    if matches!(ty.standard_leaf(), Some(StandardLeaf::AccountId | StandardLeaf::AssetAmount)) {
        return false;
    }
    match ty.kind() {
        SchemaTypeKind::Felt | SchemaTypeKind::Primitive(_) => true,
        SchemaTypeKind::Record(fields) => {
            fields.iter().all(|field| supports_native_felt_repr(field.ty()))
        }
        SchemaTypeKind::Option(payload) => supports_native_felt_repr(payload),
        SchemaTypeKind::Variant(cases) => {
            cases.iter().all(|case| case.payload().is_none_or(supports_native_felt_repr))
        }
    }
}

/// Produces documentation for a generated WIT type.
fn type_docs(ty: &SchemaType, fqn: &str) -> TokenStream {
    let docs = ty
        .docs()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Rust binding for WIT type `{fqn}`."));
    quote!(#[doc = #docs])
}

/// Produces documentation for a generated record field.
fn field_docs(field: &SchemaField) -> TokenStream {
    let docs = field
        .docs()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Value of the WIT `{}` field.", field.name()));
    quote!(#[doc = #docs])
}

/// Produces documentation for a generated variant case.
fn case_docs(case: &SchemaCase) -> TokenStream {
    let docs = case
        .docs()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("WIT `{}` case.", case.name()));
    quote!(#[doc = #docs])
}

/// Converts a WIT type or case name to a Rust type identifier.
fn type_ident(name: &str) -> Ident {
    rust_ident(&name.to_upper_camel_case())
}

/// Converts a WIT field name to a Rust value identifier.
fn value_ident(name: &str) -> Ident {
    rust_ident(&name.replace('-', "_").to_snake_case())
}

/// Creates an identifier and avoids Rust reserved words.
fn rust_ident(name: &str) -> Ident {
    const RESERVED: &[&str] = &[
        "Self", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue",
        "crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if",
        "impl", "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv",
        "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "try",
        "type", "typeof", "union", "unsafe", "unsized", "use", "virtual", "where", "while",
        "yield",
    ];
    if RESERVED.contains(&name) {
        format_ident!("{name}_")
    } else {
        Ident::new(name, Span::call_site())
    }
}

#[cfg(test)]
mod tests;
