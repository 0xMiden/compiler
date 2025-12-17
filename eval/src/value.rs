use midenc_hir::{Immediate, SourceSpan, Type, ValueRef};
use midenc_session::diagnostics::{Diagnostic, miette};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum InvalidCastError {
    #[error("cannot cast an immediate to a value of type {0}")]
    #[diagnostic()]
    UnsupportedType(Type),
    #[error("failed to cast {value} to type {ty}: value is out of range for type")]
    #[diagnostic()]
    InvalidBitcast { value: Immediate, ty: Type },
}

/// The runtime value representation for the IR
///
/// Only immediates and poison values are explicitly represented, as heap-allocated values can
/// only ever be accessed in terms of immediate values.
#[derive(Debug, Copy, Clone)]
pub enum Value {
    /// The value is invalid, and if we ever attempt to use it as an actual operand for anything
    /// other than control flow, we will raise a report with the span of the source code where
    /// the poison was generated, and the span where it was used.
    Poison {
        origin: SourceSpan,
        used: SourceSpan,
        /// The value assigned to the poison, also used to derive its type
        value: Immediate,
    },
    /// An immediate value
    Immediate(Immediate),
}

impl Value {
    pub fn poison(span: SourceSpan, value: impl Into<Immediate>) -> Self {
        Self::Poison {
            origin: span,
            used: SourceSpan::UNKNOWN,
            value: value.into(),
        }
    }

    pub fn ty(&self) -> Type {
        match self {
            Self::Poison { value, .. } | Self::Immediate(value) => value.ty(),
        }
    }

    pub fn map_ty(self, ty: &Type) -> Result<Self, InvalidCastError> {
        match self {
            Self::Poison {
                origin,
                used,
                value,
            } => {
                let value = Self::cast_immediate(value, ty)?;
                Ok(Self::Poison {
                    origin,
                    used,
                    value,
                })
            }
            Self::Immediate(value) => Self::cast_immediate(value, ty).map(Self::Immediate),
        }
    }

    pub fn cast_immediate(value: Immediate, ty: &Type) -> Result<Immediate, InvalidCastError> {
        let result = match ty {
            Type::I1 => value.as_bool().map(Immediate::I1),
            Type::I8 => value.bitcast_i8().map(Immediate::I8),
            Type::U8 => value.bitcast_u8().map(Immediate::U8),
            Type::I16 => value.bitcast_i16().map(Immediate::I16),
            Type::U16 => value.bitcast_u16().map(Immediate::U16),
            Type::I32 => value.bitcast_i32().map(Immediate::I32),
            Type::U32 => value.bitcast_u32().map(Immediate::U32),
            Type::I64 => value.bitcast_i64().map(Immediate::I64),
            Type::U64 => value.bitcast_u64().map(Immediate::U64),
            Type::I128 => value.bitcast_i128().map(Immediate::I128),
            Type::U128 => value.bitcast_u128().map(Immediate::U128),
            Type::Felt => value.bitcast_felt().map(Immediate::Felt),
            Type::F64 => value.bitcast_f64().map(Immediate::F64),
            Type::Ptr(_) => value.bitcast_u32().map(Immediate::U32),
            ty => return Err(InvalidCastError::UnsupportedType(ty.clone())),
        };

        result.ok_or_else(|| InvalidCastError::InvalidBitcast {
            value,
            ty: ty.clone(),
        })
    }
}

impl Eq for Value {}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Poison { value: x, .. }, Self::Poison { value: y, .. }) => x == y,
            (Self::Poison { .. }, _) | (_, Self::Poison { .. }) => false,
            (Self::Immediate(x), Self::Immediate(y)) => x == y,
        }
    }
}

impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Poison { .. } => f.write_str("poison"),
            Self::Immediate(imm) => write!(f, "{imm}"),
        }
    }
}

impl<T: Into<Immediate>> From<T> for Value {
    fn from(value: T) -> Self {
        Self::Immediate(value.into())
    }
}

/// A utility type for displaying value assignments in debug tracing
pub struct MaterializedValue {
    pub id: ValueRef,
    pub value: Value,
}
impl core::fmt::Display for MaterializedValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} = {}", &self.id, &self.value)
    }
}
