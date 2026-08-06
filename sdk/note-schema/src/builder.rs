//! String-path note storage builder.

use std::collections::BTreeMap;

use miden_field_repr::FeltWriter;

use crate::{
    CodecRegistry, Error, Felt, NoteStorage, NoteStorageSchema, PrimitiveType, Result, SchemaType,
    SchemaTypeKind,
    codec::{parse_felt, parse_unsigned, write_repr},
    schema::normalize_name,
    value::validate_encoding,
};

/// Builds note storage from normalized dotted string paths.
pub struct NoteStorageBuilder<'a> {
    schema: &'a NoteStorageSchema,
    registry: &'a CodecRegistry,
    values: BTreeMap<String, String>,
}

impl<'a> NoteStorageBuilder<'a> {
    /// Creates an empty builder for a schema and codec registry.
    pub(crate) fn new(schema: &'a NoteStorageSchema, registry: &'a CodecRegistry) -> Self {
        Self {
            schema,
            registry,
            values: BTreeMap::new(),
        }
    }

    /// Sets a leaf value by kebab-case or snake_case dotted path.
    ///
    /// Options accept `none` or `some(<value>)`. Variants accept a case name or
    /// `case-name(<value>)` when the selected case has a payload.
    pub fn set(mut self, path: &str, value: impl AsRef<str>) -> Result<Self> {
        let segments = normalize_path(path)?;
        resolve_path(self.schema.root(), &segments)?;
        let normalized = segments.join(".");

        if let Some(conflict) = self.values.keys().find(|existing| {
            *existing == &normalized
                || existing.starts_with(&format!("{normalized}."))
                || normalized.starts_with(&format!("{existing}."))
        }) {
            return Err(Error::new(format!(
                "path `{normalized}` conflicts with the existing value at `{conflict}`"
            )));
        }
        self.values.insert(normalized, value.as_ref().to_owned());
        Ok(self)
    }

    /// Checks completeness and returns note storage in declaration order.
    pub fn build(self) -> Result<NoteStorage> {
        let mut felts = Vec::new();
        encode_type(
            self.schema.root(),
            "",
            &self.values,
            self.registry,
            &mut FeltWriter::new(&mut felts),
        )?;
        validate_encoding(self.schema.root(), &felts)
            .map_err(|err| err.context("built note storage does not match its schema"))?;
        NoteStorage::new(felts)
            .map_err(|err| Error::new(format!("failed to create note storage: {err}")))
    }
}

/// Normalizes and validates a dotted field path.
fn normalize_path(path: &str) -> Result<Vec<String>> {
    let segments = path.split('.').map(normalize_name).collect::<Vec<_>>();
    if segments.is_empty() || segments.iter().any(String::is_empty) {
        return Err(Error::new(format!("invalid empty note storage path `{path}`")));
    }
    Ok(segments)
}

/// Resolves a dotted path through structural record fields.
fn resolve_path<'a>(mut ty: &'a SchemaType, segments: &[String]) -> Result<&'a SchemaType> {
    let mut resolved = Vec::with_capacity(segments.len());
    for segment in segments {
        let SchemaTypeKind::Record(fields) = ty.kind() else {
            return Err(Error::new(format!(
                "path `{}` continues through non-record type `{}`",
                resolved.join("."),
                ty.fqn().or(ty.name()).unwrap_or("<anonymous>")
            )));
        };
        let field = fields.iter().find(|field| field.name() == segment).ok_or_else(|| {
            let prefix = if resolved.is_empty() {
                "<root>".to_owned()
            } else {
                resolved.join(".")
            };
            Error::new(format!("record `{prefix}` has no field named `{segment}`"))
        })?;
        resolved.push(segment.clone());
        ty = field.ty();
    }
    Ok(ty)
}

/// Encodes a schema type from complete path assignments.
fn encode_type(
    ty: &SchemaType,
    path: &str,
    values: &BTreeMap<String, String>,
    registry: &CodecRegistry,
    writer: &mut FeltWriter<'_>,
) -> Result<()> {
    if let Some(value) = values.get(path) {
        let felts = encode_text_value(ty, value, registry)
            .map_err(|err| err.context(format!("value at `{path}`")))?;
        for felt in felts {
            writer.write(felt);
        }
        return Ok(());
    }

    if let SchemaTypeKind::Record(fields) = ty.kind() {
        for field in fields {
            let field_path = if path.is_empty() {
                field.name().to_owned()
            } else {
                format!("{path}.{}", field.name())
            };
            encode_type(field.ty(), &field_path, values, registry, writer)?;
        }
        return Ok(());
    }

    Err(Error::new(format!(
        "missing note storage value for `{}`",
        if path.is_empty() { "<root>" } else { path }
    )))
}

/// Encodes one direct string value.
fn encode_text_value(ty: &SchemaType, value: &str, registry: &CodecRegistry) -> Result<Vec<Felt>> {
    if let Some(fqn) = ty.fqn()
        && let Some(codec) = registry.codec(fqn)
    {
        let felts = codec.parse(value)?;
        codec
            .validate(&felts)
            .map_err(|err| err.context(format!("codec `{fqn}` validation failed")))?;
        validate_encoding(ty, &felts)
            .map_err(|err| err.context(format!("codec `{fqn}` changed the structural layout")))?;
        return Ok(felts);
    }

    match ty.kind() {
        SchemaTypeKind::Felt => Ok(write_repr(&parse_felt(value)?)),
        SchemaTypeKind::Primitive(primitive) => encode_primitive(*primitive, value),
        SchemaTypeKind::Option(payload) => encode_option(payload, value, registry),
        SchemaTypeKind::Variant(cases) => {
            let (case_name, payload_text) = parse_constructor(value)?;
            let case_name = normalize_name(case_name);
            let (ordinal, case) =
                cases.iter().enumerate().find(|(_, case)| case.name() == case_name).ok_or_else(
                    || {
                        Error::new(format!(
                            "variant `{}` has no case named `{case_name}`",
                            ty.fqn().or(ty.name()).unwrap_or("<anonymous>")
                        ))
                    },
                )?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| Error::new("variant has more than u32::MAX cases"))?;
            let mut felts = write_repr(&ordinal);
            match (case.payload(), payload_text) {
                (None, None) => {}
                (None, Some(_)) => {
                    return Err(Error::new(format!(
                        "variant case `{case_name}` does not accept a payload"
                    )));
                }
                (Some(_), None) => {
                    return Err(Error::new(format!("variant case `{case_name}` needs a payload")));
                }
                (Some(payload), Some(payload_text)) => {
                    felts.extend(encode_text_value(payload, payload_text, registry)?);
                }
            }
            Ok(felts)
        }
        SchemaTypeKind::Record(_) => Err(Error::new(format!(
            "type `{}` has no codec; set its leaf fields with dotted paths",
            ty.fqn().or(ty.name()).unwrap_or("<anonymous record>")
        ))),
    }
}

/// Encodes an option tag and optional direct payload.
fn encode_option(payload: &SchemaType, value: &str, registry: &CodecRegistry) -> Result<Vec<Felt>> {
    let value = value.trim();
    if value == "none" {
        return Ok(write_repr(&0u32));
    }
    let (constructor, payload_text) = parse_constructor(value)?;
    if normalize_name(constructor) != "some" {
        return Err(Error::new("an option value must be `none` or `some(<value>)`"));
    }
    let payload_text =
        payload_text.ok_or_else(|| Error::new("an option `some` value needs a payload"))?;
    let mut felts = write_repr(&1u32);
    felts.extend(encode_text_value(payload, payload_text, registry)?);
    Ok(felts)
}

/// Encodes one supported primitive through `miden-field-repr`.
fn encode_primitive(primitive: PrimitiveType, value: &str) -> Result<Vec<Felt>> {
    match primitive {
        PrimitiveType::U64 => Ok(write_repr(&parse_unsigned(value, "u64")?)),
        PrimitiveType::U32 => {
            let value = u32::try_from(parse_unsigned(value, "u32")?)
                .map_err(|_| Error::new(format!("u32 value `{value}` is out of range")))?;
            Ok(write_repr(&value))
        }
        PrimitiveType::U8 => {
            let value = u8::try_from(parse_unsigned(value, "u8")?)
                .map_err(|_| Error::new(format!("u8 value `{value}` is out of range")))?;
            Ok(write_repr(&value))
        }
        PrimitiveType::Bool => {
            let value = match value.trim() {
                "true" | "1" => true,
                "false" | "0" => false,
                value => {
                    return Err(Error::new(format!(
                        "invalid bool `{value}`; expected true, false, 1, or 0"
                    )));
                }
            };
            Ok(write_repr(&value))
        }
    }
}

/// Splits `name` or `name(payload)` text without interpreting the payload.
fn parse_constructor(value: &str) -> Result<(&str, Option<&str>)> {
    let value = value.trim();
    let Some(open) = value.find('(') else {
        return Ok((value, None));
    };
    if !value.ends_with(')') {
        return Err(Error::new(format!("value `{value}` has an unclosed payload")));
    }
    let name = value[..open].trim();
    let payload = value[open + 1..value.len() - 1].trim();
    if name.is_empty() || payload.is_empty() {
        return Err(Error::new(format!("value `{value}` has an empty constructor or payload")));
    }
    Ok((name, Some(payload)))
}
