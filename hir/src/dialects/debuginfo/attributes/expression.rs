use alloc::{format, vec::Vec};

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
    /// A function-local slot containing a byte address.
    LocalSlot(u32),
    /// A module-global slot containing a byte address.
    GlobalSlot(Symbol),
}

/// Represents target-neutral HIR operations for describing variable locations and values.
///
/// Bare slots contain values; Deref reads memory at those values. Address and FrameBase produce
/// byte-address locations with an implicit final memory read, unless StackValue requests a scalar.
/// Deref preserves that address/value distinction. Source DWARF is normalized to this convention
/// by the frontend. Arithmetic retains DW_OP names; target-neutral slots use DI_OP names.
///
/// This is an in-memory/textual HIR representation, not a package wire format.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExpressionOp {
    /// Variable is in a logical function-local slot.
    LocalSlot(u32),
    /// Variable is in a logical operand-stack slot.
    OperandStackSlot(u32),
    /// DW_OP_constu - Unsigned constant value
    ConstU64(u64),
    /// DW_OP_consts - Signed constant value
    ConstS64(i64),
    /// DW_OP_plus_uconst - Add unsigned constant to top of stack
    PlusUConst(u64),
    /// DW_OP_minus - Subtract top two stack values
    Minus,
    /// DW_OP_plus - Add top two stack values
    Plus,
    /// DW_OP_deref - Dereference the address at top of stack
    Deref,
    /// DW_OP_stack_value - The value on the stack is the value of the variable
    StackValue,
    /// DW_OP_piece - Describes a piece of a variable
    Piece(u64),
    /// DW_OP_bit_piece - Describes a piece of a variable in bits
    BitPiece { size: u64, offset: u64 },
    /// DW_OP_addr - pushes memory address `address` on the expression operand stack
    Address { address: u64 },
    /// Logical frame-base slot + byte offset.
    FrameBase { base: FrameBase, byte_offset: i64 },
    /// Variable is in a logical module-global slot identified by its HIR symbol.
    GlobalSlot(Symbol),
    /// Placeholder for unsupported operations
    Unsupported(Symbol),
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
                    + text(format!("\"{}\"", name.as_str().escape_default()))
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
                        + text(format!("\"{}\"{byte_offset:+}", index.as_str().escape_default()))
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

/// Describes how to compute or locate a variable's value using target-neutral HIR operations.
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

impl AttrPrinter for ExpressionAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;
        *printer += const_text("[");
        for (index, operation) in self.operations.iter().enumerate() {
            if index > 0 {
                *printer += const_text(", ");
            }
            *printer += operation.render();
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
