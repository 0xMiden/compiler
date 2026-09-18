use alloc::{format, string::ToString, vec::Vec};

use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, parse::ParserExt, print::AsmPrinter,
};

/// The logical HIR slot that supplies a frame base.
///
/// These slots are independent of both the source DWARF encoding and the final Miden frame layout.
/// The backend resolves them after the function's aligned frame size and global layout are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameBase {
    LocalSlot(u32),
    GlobalSlot(Symbol),
}

/// Represents target-neutral HIR operations for describing variable locations and values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExpressionOp {
    /// Variable is in a logical function-local slot.
    LocalSlot(u32) = 0,
    /// Variable is in a logical operand-stack slot.
    OperandStackSlot(u32) = 2,
    /// DW_OP_constu - Unsigned constant value
    ConstU64(u64) = 3,
    /// DW_OP_consts - Signed constant value
    ConstS64(i64) = 4,
    /// DW_OP_plus_uconst - Add unsigned constant to top of stack
    PlusUConst(u64) = 5,
    /// DW_OP_minus - Subtract top two stack values
    Minus = 6,
    /// DW_OP_plus - Add top two stack values
    Plus = 7,
    /// DW_OP_deref - Dereference the address at top of stack
    Deref = 8,
    /// DW_OP_stack_value - The value on the stack is the value of the variable
    StackValue = 9,
    /// DW_OP_piece - Describes a piece of a variable
    Piece(u64) = 10,
    /// DW_OP_bit_piece - Describes a piece of a variable in bits
    BitPiece { size: u64, offset: u64 } = 11,
    /// DW_OP_addr - pushes memory address `address` on the expression operand stack
    Address { address: u64 } = 13,
    /// Logical frame-base slot + byte offset.
    FrameBase { base: FrameBase, byte_offset: i64 } = 14,
    /// Variable is in a logical module-global slot identified by its HIR symbol.
    GlobalSlot(Symbol) = 15,
    /// Placeholder for unsupported operations
    Unsupported(Symbol) = u8::MAX,
}

impl ExpressionOp {
    const fn tag(&self) -> u8 {
        // SAFETY: This is safe because we have given this enum a
        // primitive representation with #[repr(u8)], with the first
        // field of the underlying union-of-structs the discriminant
        //
        // See the section on "accessing the numeric value of the discriminant"
        // here: https://doc.rust-lang.org/std/mem/fn.discriminant.html
        unsafe { *(self as *const Self).cast::<u8>() }
    }
}

impl miden_core::serde::Serializable for ExpressionOp {
    fn write_into<W: miden_core::serde::ByteWriter>(&self, target: &mut W) {
        target.write_u8(self.tag());
        match self {
            Self::LocalSlot(idx) | Self::OperandStackSlot(idx) => {
                target.write_u32(*idx);
            }
            Self::ConstU64(val) | Self::PlusUConst(val) | Self::Piece(val) => {
                target.write_u64(*val);
            }
            Self::ConstS64(val) => {
                target.write_u64(*val as u64);
            }
            Self::Minus | Self::Plus | Self::Deref | Self::StackValue => (),
            Self::BitPiece { size, offset } => {
                target.write_u64(*size);
                target.write_u64(*offset);
            }
            Self::FrameBase { base, byte_offset } => {
                match base {
                    FrameBase::LocalSlot(index) => {
                        target.write_u8(0);
                        target.write_u32(*index);
                    }
                    FrameBase::GlobalSlot(index) => {
                        target.write_u8(1);
                        target.write_usize(index.as_str().len());
                        target.write_bytes(index.as_str().as_bytes());
                    }
                }
                target.write_u64(*byte_offset as u64);
            }
            Self::Address { address } => {
                target.write_u64(*address);
            }
            Self::GlobalSlot(name) | Self::Unsupported(name) => {
                target.write_usize(name.as_str().len());
                target.write_bytes(name.as_str().as_bytes());
            }
        }
    }
}

impl miden_core::serde::Deserializable for ExpressionOp {
    fn read_from<R: miden_core::serde::ByteReader>(
        source: &mut R,
    ) -> Result<Self, miden_core::serde::DeserializationError> {
        use miden_core::serde::DeserializationError;

        Ok(match source.read_u8()? {
            0 => Self::LocalSlot(u32::read_from(source)?),
            2 => Self::OperandStackSlot(u32::read_from(source)?),
            3 => Self::ConstU64(u64::read_from(source)?),
            4 => Self::ConstS64(u64::read_from(source)? as i64),
            5 => Self::PlusUConst(u64::read_from(source)?),
            6 => Self::Minus,
            7 => Self::Plus,
            8 => Self::Deref,
            9 => Self::StackValue,
            10 => Self::Piece(u64::read_from(source)?),
            11 => {
                let size = u64::read_from(source)?;
                let offset = u64::read_from(source)?;
                Self::BitPiece { size, offset }
            }
            13 => {
                let address = u64::read_from(source)?;
                Self::Address { address }
            }
            14 => {
                let base = match source.read_u8()? {
                    0 => FrameBase::LocalSlot(u32::read_from(source)?),
                    1 => {
                        let len = usize::read_from(source)?;
                        let bytes = source.read_slice(len)?;
                        let name = core::str::from_utf8(bytes)
                            .map_err(|err| DeserializationError::InvalidValue(err.to_string()))?;
                        FrameBase::GlobalSlot(Symbol::intern(name))
                    }
                    tag => {
                        return Err(DeserializationError::InvalidValue(format!(
                            "invalid frame-base tag '{tag}'"
                        )));
                    }
                };
                let byte_offset = u64::read_from(source)? as i64;
                Self::FrameBase { base, byte_offset }
            }
            15 => {
                let len = usize::read_from(source)?;
                let bytes = source.read_slice(len)?;
                let name = core::str::from_utf8(bytes)
                    .map_err(|err| DeserializationError::InvalidValue(err.to_string()))?;
                Self::GlobalSlot(Symbol::intern(name))
            }
            u8::MAX => {
                let len = usize::read_from(source)?;
                let bytes = source.read_slice(len)?;
                let s = core::str::from_utf8(bytes)
                    .map_err(|err| DeserializationError::InvalidValue(err.to_string()))?;
                Self::Unsupported(Symbol::intern(s))
            }
            invalid => {
                return Err(DeserializationError::InvalidValue(format!(
                    "unknown DIExpressionOp tag '{invalid}'"
                )));
            }
        })
    }

    fn min_serialized_size() -> usize {
        1
    }
}

impl crate::formatter::PrettyPrint for ExpressionOp {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        match self {
            Self::LocalSlot(idx) => {
                const_text("DI_OP_local_slot") + const_text("(") + display(idx) + const_text(")")
            }
            Self::GlobalSlot(name) => {
                const_text("DI_OP_global_slot")
                    + const_text("(")
                    + text(format!("{:?}", name.as_str()))
                    + const_text(")")
            }
            Self::OperandStackSlot(idx) => {
                const_text("DI_OP_operand_stack_slot")
                    + const_text("(")
                    + display(idx)
                    + const_text(")")
            }
            Self::ConstU64(val) => {
                const_text("DW_OP_constu") + const_text("(") + display(val) + const_text(")")
            }
            Self::ConstS64(val) => {
                const_text("DW_OP_consts") + const_text("(") + display(val) + const_text(")")
            }
            Self::PlusUConst(val) => {
                const_text("DW_OP_plus_uconst") + const_text("(") + display(val) + const_text(")")
            }
            Self::Minus => const_text("DW_OP_minus"),
            Self::Plus => const_text("DW_OP_plus"),
            Self::Deref => const_text("DW_OP_deref"),
            Self::StackValue => const_text("DW_OP_stack_value"),
            Self::Piece(size) => {
                const_text("DW_OP_piece") + const_text("(") + display(*size) + const_text(")")
            }
            Self::BitPiece { size, offset } => {
                const_text("DW_OP_bit_piece")
                    + const_text("(")
                    + display(*size)
                    + const_text(",")
                    + display(*offset)
                    + const_text(")")
            }
            Self::FrameBase { base, byte_offset } => match base {
                FrameBase::LocalSlot(index) => {
                    const_text("DI_OP_frame_base(local_slot, ")
                        + text(format!("{index}{byte_offset:+}"))
                        + const_text(")")
                }
                FrameBase::GlobalSlot(index) => {
                    const_text("DI_OP_frame_base(global_slot, ")
                        + text(format!("{:?}{byte_offset:+}", index.as_str()))
                        + const_text(")")
                }
            },
            Self::Address { address } => {
                const_text("DW_OP_addr") + const_text("(") + display(*address) + const_text(")")
            }
            Self::Unsupported(name) => const_text(name.as_str()),
        }
    }
}

impl ExpressionOp {
    fn parse(parser: &mut dyn crate::parse::Parser<'_>) -> crate::parse::ParseResult<Self> {
        use crate::parse::Token;

        let mut op = parser
            .token_stream_mut()
            .expect_map("DIExpression operator", |tok| match tok {
                Token::BareIdent(id) => match id {
                    "DI_OP_local_slot" => Some(ExpressionOp::LocalSlot(0)),
                    "DI_OP_global_slot" => {
                        Some(ExpressionOp::GlobalSlot(Symbol::intern("placeholder")))
                    }
                    "DI_OP_operand_stack_slot" => Some(ExpressionOp::OperandStackSlot(0)),
                    "DW_OP_constu" => Some(ExpressionOp::ConstU64(0)),
                    "DW_OP_consts" => Some(ExpressionOp::ConstS64(0)),
                    "DW_OP_plus_uconst" => Some(ExpressionOp::PlusUConst(0)),
                    "DW_OP_minus" => Some(ExpressionOp::Minus),
                    "DW_OP_plus" => Some(ExpressionOp::Plus),
                    "DW_OP_deref" => Some(ExpressionOp::Deref),
                    "DW_OP_stack_value" => Some(ExpressionOp::StackValue),
                    "DW_OP_piece" => Some(ExpressionOp::Piece(0)),
                    "DW_OP_bit_piece" => Some(ExpressionOp::BitPiece { size: 0, offset: 0 }),
                    "DI_OP_frame_base" => Some(ExpressionOp::FrameBase {
                        base: FrameBase::GlobalSlot(Symbol::intern("placeholder")),
                        byte_offset: 0,
                    }),
                    "DW_OP_addr" => Some(ExpressionOp::Address { address: 0 }),
                    other => Some(ExpressionOp::Unsupported(Symbol::intern(other))),
                },
                _ => None,
            })?
            .into_inner();
        match &mut op {
            ExpressionOp::LocalSlot(idx) | ExpressionOp::OperandStackSlot(idx) => {
                parser.parse_lparen()?;
                *idx = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_rparen()?;
            }
            ExpressionOp::GlobalSlot(name) => {
                parser.parse_lparen()?;
                *name = parser.parse_string()?.into_inner().into();
                parser.parse_rparen()?;
            }
            ExpressionOp::ConstU64(val)
            | ExpressionOp::PlusUConst(val)
            | ExpressionOp::Piece(val)
            | ExpressionOp::Address { address: val } => {
                parser.parse_lparen()?;
                *val = parser.parse_decimal_integer::<u64>()?.into_inner();
                parser.parse_rparen()?;
            }
            ExpressionOp::ConstS64(val) => {
                parser.parse_lparen()?;
                *val = parser.parse_decimal_integer::<i64>()?.into_inner();
                parser.parse_rparen()?;
            }
            ExpressionOp::Minus
            | ExpressionOp::Plus
            | ExpressionOp::Deref
            | ExpressionOp::StackValue
            | ExpressionOp::Unsupported(_) => (),
            ExpressionOp::BitPiece { size, offset } => {
                parser.parse_lparen()?;
                *size = parser.parse_decimal_integer::<u64>()?.into_inner();
                parser.parse_comma()?;
                *offset = parser.parse_decimal_integer::<u64>()?.into_inner();
                parser.parse_rparen()?;
            }
            ExpressionOp::FrameBase { base, byte_offset } => {
                parser.parse_lparen()?;
                let is_local = parser
                    .token_stream_mut()
                    .expect_map("'local_slot' or 'global_slot' modifier", |tok| match tok {
                        Token::BareIdent("local_slot") => Some(true),
                        Token::BareIdent("global_slot") => Some(false),
                        _ => None,
                    })?
                    .into_inner();
                parser.parse_comma()?;
                let parsed_base = if is_local {
                    FrameBase::LocalSlot(parser.parse_decimal_integer::<u32>()?.into_inner())
                } else {
                    FrameBase::GlobalSlot(parser.parse_string()?.into_inner().into())
                };
                // The printed form is `INDEX{+|-}OFFSET`, e.g.
                // `DI_OP_frame_base(local_slot, 2+8)`.
                let negative = parser
                    .token_stream_mut()
                    .expect_map("'+' or '-' offset sign", |tok| match tok {
                        Token::Plus => Some(false),
                        Token::Minus => Some(true),
                        _ => None,
                    })?
                    .into_inner();
                let (offset_span, magnitude) = parser.parse_decimal_integer::<u64>()?.into_parts();
                let signed = if negative {
                    -(magnitude as i128)
                } else {
                    magnitude as i128
                };
                *byte_offset = i64::try_from(signed).map_err(|_| {
                    crate::parse::ParserError::InvalidIntegerLiteral {
                        span: offset_span,
                        reason: format!("byte offset '{signed}' is out of range for i64"),
                    }
                })?;
                *base = parsed_base;
                parser.parse_rparen()?;
            }
        }

        Ok(op)
    }
}

/// Represents a DWARF expression that describes how to compute or locate a variable's value
#[derive(DialectAttribute, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[attribute(dialect = DebugInfoDialect, implements(AttrPrinter))]
pub struct Expression {
    pub operations: Vec<ExpressionOp>,
}

impl Expression {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    pub fn with_ops(operations: Vec<ExpressionOp>) -> Self {
        Self { operations }
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl miden_core::serde::Serializable for Expression {
    fn write_into<W: miden_core::serde::ByteWriter>(&self, target: &mut W) {
        target.write_usize(self.operations.len());
        for op in self.operations.iter() {
            target.write(op);
        }
    }
}

impl miden_core::serde::Deserializable for Expression {
    fn read_from<R: miden_core::serde::ByteReader>(
        source: &mut R,
    ) -> Result<Self, miden_core::serde::DeserializationError> {
        let len = usize::read_from(source)?;
        let operations = source.read_many_iter(len)?.collect::<Result<Vec<_>, _>>()?;
        Ok(Self::with_ops(operations))
    }
}

impl AttrPrinter for ExpressionAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        if self.operations.is_empty() {
            *printer += const_text("[]");
            return;
        }

        *printer += const_text("[");
        for (i, op) in self.operations.iter().enumerate() {
            if i > 0 {
                *printer += const_text(", ");
            }
            match op {
                ExpressionOp::LocalSlot(idx) => {
                    *printer += const_text("DI_OP_local_slot");
                    *printer += const_text("(") + display(*idx) + const_text(")");
                }
                ExpressionOp::GlobalSlot(idx) => {
                    *printer += const_text("DI_OP_global_slot");
                    *printer += const_text("(");
                    printer.print_string(idx.as_str());
                    *printer += const_text(")");
                }
                ExpressionOp::OperandStackSlot(idx) => {
                    *printer += const_text("DI_OP_operand_stack_slot");
                    *printer += const_text("(") + display(*idx) + const_text(")");
                }
                ExpressionOp::ConstU64(val) => {
                    *printer += const_text("DW_OP_constu");
                    *printer += const_text("(") + display(*val) + const_text(")");
                }
                ExpressionOp::ConstS64(val) => {
                    *printer += const_text("DW_OP_consts");
                    *printer += const_text("(") + display(*val) + const_text(")");
                }
                ExpressionOp::PlusUConst(val) => {
                    *printer += const_text("DW_OP_plus_uconst");
                    *printer += const_text("(") + display(*val) + const_text(")");
                }
                ExpressionOp::Minus => *printer += const_text("DW_OP_minus"),
                ExpressionOp::Plus => *printer += const_text("DW_OP_plus"),
                ExpressionOp::Deref => *printer += const_text("DW_OP_deref"),
                ExpressionOp::StackValue => *printer += const_text("DW_OP_stack_value"),
                ExpressionOp::Piece(size) => {
                    *printer += const_text("DW_OP_piece");
                    *printer += const_text("(") + display(*size) + const_text(")");
                }
                ExpressionOp::BitPiece { size, offset } => {
                    *printer += const_text("DW_OP_bit_piece");
                    *printer += const_text("(")
                        + display(*size)
                        + const_text(",")
                        + display(*offset)
                        + const_text(")");
                }
                ExpressionOp::FrameBase { base, byte_offset } => match base {
                    FrameBase::LocalSlot(index) => {
                        *printer += const_text("DI_OP_frame_base(local_slot, ");
                        *printer += text(format!("{}{:+}", index, byte_offset));
                        *printer += const_text(")");
                    }
                    FrameBase::GlobalSlot(index) => {
                        *printer += const_text("DI_OP_frame_base(global_slot, ");
                        printer.print_string(index.as_str());
                        *printer += text(format!("{:+}", byte_offset));
                        *printer += const_text(")");
                    }
                },
                ExpressionOp::Address { address } => {
                    *printer += const_text("DW_OP_addr");
                    *printer += const_text("(") + display(*address) + const_text(")");
                }
                ExpressionOp::Unsupported(name) => *printer += const_text(name.as_str()),
            }
        }
        *printer += const_text("]");
    }
}

impl AttrParser for ExpressionAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Delimiter;

        let mut ops = Vec::default();
        parser.parse_comma_separated_list(
            Delimiter::OptionalBracket,
            Some("DIExpression"),
            |parser| {
                ops.push(ExpressionOp::parse(parser)?);

                Ok(true)
            },
        )?;

        let attr = parser
            .context_rc()
            .create_attribute::<ExpressionAttr, _>(Expression::with_ops(ops));

        Ok(attr.as_attribute_ref())
    }
}

#[cfg(test)]
mod tests {
    use miden_core::serde::{Deserializable, Serializable, SliceReader};

    use super::*;

    #[test]
    fn global_slot_expressions_round_trip_by_symbol() {
        for op in [
            ExpressionOp::GlobalSlot(Symbol::intern("global4")),
            ExpressionOp::FrameBase {
                base: FrameBase::GlobalSlot(Symbol::intern("__stack_pointer")),
                byte_offset: -16,
            },
        ] {
            let bytes = op.to_bytes();
            let mut reader = SliceReader::new(&bytes);
            assert_eq!(ExpressionOp::read_from(&mut reader).unwrap(), op);
        }
    }
}
