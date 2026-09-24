//! Named values decoded from note storage.

use core::fmt;

use miden_field_repr::{FeltReader, FromFeltRepr};

use crate::{
    CodecRegistry, Error, Felt, NoteStorage, PrimitiveType, Result, SchemaType, SchemaTypeKind,
    schema::normalize_name,
};

/// A named value decoded from note storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedValue {
    name: Option<String>,
    docs: Option<String>,
    fqn: Option<String>,
    kind: DecodedValueKind,
}

impl DecodedValue {
    /// Returns the field or root type name.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the field-level or root type documentation.
    pub fn docs(&self) -> Option<&str> {
        self.docs.as_deref()
    }

    /// Returns the canonical WIT FQN when this value has one.
    pub fn fqn(&self) -> Option<&str> {
        self.fqn.as_deref()
    }

    /// Returns the decoded value kind.
    pub const fn kind(&self) -> &DecodedValueKind {
        &self.kind
    }

    /// Finds a direct record field with kebab-case or snake_case spelling.
    pub fn field(&self, name: &str) -> Option<&Self> {
        let DecodedValueKind::Record(fields) = &self.kind else {
            return None;
        };
        let name = normalize_name(name);
        fields.iter().find(|field| field.name.as_deref() == Some(name.as_str()))
    }
}

/// The structural value stored in a decoded node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedValueKind {
    /// A codec-rendered or primitive leaf with its encoded felts.
    Leaf {
        /// The structural felt representation.
        felts: Vec<Felt>,
        /// The registry-backed or primitive display text.
        display: String,
    },
    /// Record fields in declaration order.
    Record(Vec<DecodedValue>),
    /// An optional value.
    Option(Option<Box<DecodedValue>>),
    /// A selected variant case and its optional payload.
    Variant {
        /// The selected case name.
        case: String,
        /// The decoded case payload.
        value: Option<Box<DecodedValue>>,
    },
}

impl fmt::Display for DecodedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DecodedValueKind::Leaf { display, .. } => f.write_str(display),
            DecodedValueKind::Record(fields) => {
                f.write_str("{")?;
                for (index, field) in fields.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(field.name.as_deref().unwrap_or("<unnamed>"))?;
                    f.write_str(": ")?;
                    field.fmt(f)?;
                }
                f.write_str("}")
            }
            DecodedValueKind::Option(None) => f.write_str("none"),
            DecodedValueKind::Option(Some(value)) => write!(f, "some({value})"),
            DecodedValueKind::Variant { case, value: None } => f.write_str(case),
            DecodedValueKind::Variant {
                case,
                value: Some(value),
            } => write!(f, "{case}({value})"),
        }
    }
}

/// Decodes a root storage value and rejects trailing felts.
pub(crate) fn decode(
    root: &SchemaType,
    storage: &NoteStorage,
    registry: &CodecRegistry,
) -> Result<DecodedValue> {
    let (value, consumed) = decode_type(
        root,
        storage.items(),
        Some(registry),
        root.name().map(str::to_owned),
        root.docs().map(str::to_owned),
    )?;
    if consumed != storage.items().len() {
        return Err(Error::new(format!(
            "note storage has {} trailing felt(s) after the schema root",
            storage.items().len() - consumed
        )));
    }
    Ok(value)
}

/// Validates one complete structural encoding without applying codecs.
pub(crate) fn validate_encoding(ty: &SchemaType, felts: &[Felt]) -> Result<()> {
    let (_, consumed) = decode_type(ty, felts, None, None, None)?;
    if consumed != felts.len() {
        return Err(Error::new(format!(
            "value has {} trailing felt(s) after its structural encoding",
            felts.len() - consumed
        )));
    }
    Ok(())
}

/// Decodes one value prefix and returns the consumed felt count.
fn decode_type(
    ty: &SchemaType,
    input: &[Felt],
    registry: Option<&CodecRegistry>,
    name: Option<String>,
    docs: Option<String>,
) -> Result<(DecodedValue, usize)> {
    if let (Some(registry), Some(fqn)) = (registry, ty.fqn())
        && let Some(codec) = registry.codec(fqn)
    {
        let (_, consumed) = decode_type(ty, input, None, None, None)?;
        let felts = input[..consumed].to_vec();
        codec
            .validate(&felts)
            .map_err(|err| err.context(format!("codec `{fqn}` rejected decoded value")))?;
        let display = codec
            .display(&felts)
            .map_err(|err| err.context(format!("codec `{fqn}` failed to display decoded value")))?;
        return Ok((
            DecodedValue {
                name,
                docs,
                fqn: Some(fqn.to_owned()),
                kind: DecodedValueKind::Leaf { display, felts },
            },
            consumed,
        ));
    }

    let fqn = ty.fqn().map(str::to_owned);
    let (kind, consumed) = match ty.kind() {
        SchemaTypeKind::Felt => {
            let mut reader = FeltReader::new(input);
            let felt = reader
                .read()
                .map_err(|err| Error::new(format!("invalid felt representation: {err}")))?;
            (
                DecodedValueKind::Leaf {
                    felts: vec![felt],
                    display: felt.as_canonical_u64().to_string(),
                },
                reader.pos(),
            )
        }
        SchemaTypeKind::Primitive(primitive) => decode_primitive(*primitive, input)?,
        SchemaTypeKind::Record(fields) => {
            let mut decoded = Vec::with_capacity(fields.len());
            let mut offset = 0usize;
            for field in fields {
                let (value, consumed) = decode_type(
                    field.ty(),
                    &input[offset..],
                    registry,
                    Some(field.name().to_owned()),
                    field.docs().map(str::to_owned),
                )
                .map_err(|err| err.context(format!("field `{}`", field.name())))?;
                offset = offset
                    .checked_add(consumed)
                    .ok_or_else(|| Error::new("decoded record width is too large"))?;
                decoded.push(value);
            }
            (DecodedValueKind::Record(decoded), offset)
        }
        SchemaTypeKind::Option(payload) => {
            let mut reader = FeltReader::new(input);
            let tag = reader
                .read()
                .map_err(|err| Error::new(format!("invalid option representation: {err}")))?
                .as_canonical_u64();
            match tag {
                0 => (DecodedValueKind::Option(None), reader.pos()),
                1 => {
                    let (value, consumed) =
                        decode_type(payload, &input[reader.pos()..], registry, None, None)?;
                    (DecodedValueKind::Option(Some(Box::new(value))), reader.pos() + consumed)
                }
                tag => {
                    return Err(Error::new(format!("invalid option tag {tag}; expected 0 or 1")));
                }
            }
        }
        SchemaTypeKind::Variant(cases) => {
            let mut reader = FeltReader::new(input);
            let tag = reader
                .read_u32()
                .map_err(|err| Error::new(format!("invalid variant representation: {err}")))?
                as usize;
            let case = cases.get(tag).ok_or_else(|| {
                Error::new(format!(
                    "invalid variant tag {tag}; expected a declaration ordinal below {}",
                    cases.len()
                ))
            })?;
            let (value, consumed) = match case.payload() {
                Some(payload) => {
                    let (value, consumed) =
                        decode_type(payload, &input[reader.pos()..], registry, None, None)?;
                    (Some(Box::new(value)), reader.pos() + consumed)
                }
                None => (None, reader.pos()),
            };
            (
                DecodedValueKind::Variant {
                    case: case.name().to_owned(),
                    value,
                },
                consumed,
            )
        }
    };

    Ok((
        DecodedValue {
            name,
            docs,
            fqn,
            kind,
        },
        consumed,
    ))
}

/// Decodes a supported primitive through `miden-field-repr`.
fn decode_primitive(primitive: PrimitiveType, input: &[Felt]) -> Result<(DecodedValueKind, usize)> {
    match primitive {
        PrimitiveType::U64 => read_primitive::<u64>(input),
        PrimitiveType::U32 => read_primitive::<u32>(input),
        PrimitiveType::U8 => read_primitive::<u8>(input),
        PrimitiveType::Bool => read_primitive::<bool>(input),
    }
}

/// Decodes and displays one primitive value.
fn read_primitive<T>(input: &[Felt]) -> Result<(DecodedValueKind, usize)>
where
    T: FromFeltRepr + fmt::Display,
{
    let mut reader = FeltReader::new(input);
    let value = T::from_felt_repr(&mut reader)
        .map_err(|err| Error::new(format!("invalid primitive representation: {err}")))?;
    Ok((
        DecodedValueKind::Leaf {
            felts: input[..reader.pos()].to_vec(),
            display: value.to_string(),
        },
        reader.pos(),
    ))
}
