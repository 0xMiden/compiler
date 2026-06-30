use alloc::{format, string::ToString, vec::Vec};

use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, parse::ParserExt, print::AsmPrinter,
};

/// Represents DWARF expression operations for describing variable locations
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExpressionOp {
    /// DW_OP_WASM_location 0x00 - Variable is in a WebAssembly local
    WasmLocal(u32) = 0,
    /// DW_OP_WASM_location 0x01 - Variable is in a WebAssembly global
    WasmGlobal(u32) = 1,
    /// DW_OP_WASM_location 0x02 - Variable is on the WebAssembly operand stack
    WasmStack(u32) = 2,
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
    /// DW_OP_fbreg - Frame base register + offset.
    /// The variable is in WASM linear memory at `value_of(global[global_index]) + byte_offset`.
    FrameBase { global_index: u32, byte_offset: i64 } = 12,
    /// DW_OP_addr - pushes memory address `address` on the expression operand stack
    Address { address: u64 } = 13,
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
            Self::WasmLocal(idx) | Self::WasmGlobal(idx) | Self::WasmStack(idx) => {
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
            Self::FrameBase {
                global_index,
                byte_offset,
            } => {
                target.write_u32(*global_index);
                target.write_u64(*byte_offset as u64);
            }
            Self::Address { address } => {
                target.write_u64(*address);
            }
            Self::Unsupported(name) => {
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
            0 => Self::WasmLocal(u32::read_from(source)?),
            1 => Self::WasmGlobal(u32::read_from(source)?),
            2 => Self::WasmStack(u32::read_from(source)?),
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
            12 => {
                let global_index = u32::read_from(source)?;
                let byte_offset = u64::read_from(source)? as i64;
                Self::FrameBase {
                    global_index,
                    byte_offset,
                }
            }
            13 => {
                let address = u64::read_from(source)?;
                Self::Address { address }
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
}

impl crate::formatter::PrettyPrint for ExpressionOp {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        match self {
            Self::WasmLocal(idx) => {
                const_text("DW_OP_WASM_local") + const_text("(") + display(idx) + const_text(")")
            }
            Self::WasmGlobal(idx) => {
                const_text("DW_OP_WASM_global") + const_text("(") + display(idx) + const_text(")")
            }
            Self::WasmStack(idx) => {
                const_text("DW_OP_WASM_stack") + const_text("(") + display(idx) + const_text(")")
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
            Self::FrameBase {
                global_index,
                byte_offset,
            } => {
                if let Some(local_index) = decode_frame_base_local_index(*global_index) {
                    const_text("DW_OP_fbreg(local, ")
                        + text(format!("{local_index}{byte_offset:+}"))
                        + const_text(")")
                } else {
                    const_text("DW_OP_fbreg(global, ")
                        + text(format!("{global_index}{byte_offset:+}"))
                        + const_text(")")
                }
            }
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
                    "DW_OP_WASM_local" => Some(ExpressionOp::WasmLocal(0)),
                    "DW_OP_WASM_global" => Some(ExpressionOp::WasmGlobal(0)),
                    "DW_OP_WASM_stack" => Some(ExpressionOp::WasmStack(0)),
                    "DW_OP_constu" => Some(ExpressionOp::ConstU64(0)),
                    "DW_OP_consts" => Some(ExpressionOp::ConstS64(0)),
                    "DW_OP_plus_uconst" => Some(ExpressionOp::PlusUConst(0)),
                    "DW_OP_minus" => Some(ExpressionOp::Minus),
                    "DW_OP_plus" => Some(ExpressionOp::Plus),
                    "DW_OP_deref" => Some(ExpressionOp::Deref),
                    "DW_OP_stack_value" => Some(ExpressionOp::StackValue),
                    "DW_OP_piece" => Some(ExpressionOp::Piece(0)),
                    "DW_OP_bit_piece" => Some(ExpressionOp::BitPiece { size: 0, offset: 0 }),
                    "DW_OP_fbreg" => Some(ExpressionOp::FrameBase {
                        global_index: 0,
                        byte_offset: 0,
                    }),
                    "DW_OP_addr" => Some(ExpressionOp::Address { address: 0 }),
                    other => Some(ExpressionOp::Unsupported(Symbol::intern(other))),
                },
                _ => None,
            })?
            .into_inner();
        match &mut op {
            ExpressionOp::WasmLocal(idx)
            | ExpressionOp::WasmGlobal(idx)
            | ExpressionOp::WasmStack(idx) => {
                parser.parse_lparen()?;
                *idx = parser.parse_decimal_integer::<u32>()?.into_inner();
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
            ExpressionOp::FrameBase {
                global_index,
                byte_offset,
            } => {
                parser.parse_lparen()?;
                parser
                    .token_stream_mut()
                    .expect_if("'local' or 'global' modifier", |tok| {
                        matches!(tok, Token::BareIdent("local" | "global"))
                    })?
                    .into_inner();
                parser.parse_comma()?;
                let index = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_comma()?;
                *byte_offset = parser.parse_decimal_integer::<i64>()?.into_inner();
                *global_index = encode_frame_base_local_index(index).unwrap_or(index);
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
        let mut expr = Self::with_ops(Vec::with_capacity(len));
        for _ in 0..len {
            expr.operations.push(ExpressionOp::read_from(source)?);
        }
        Ok(expr)
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
                ExpressionOp::WasmLocal(idx) => {
                    *printer += const_text("DW_OP_WASM_local");
                    *printer += const_text("(") + display(*idx) + const_text(")");
                }
                ExpressionOp::WasmGlobal(idx) => {
                    *printer += const_text("DW_OP_WASM_global");
                    *printer += const_text("(") + display(*idx) + const_text(")");
                }
                ExpressionOp::WasmStack(idx) => {
                    *printer += const_text("DW_OP_WASM_stack");
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
                ExpressionOp::FrameBase {
                    global_index,
                    byte_offset,
                } => {
                    if let Some(local_index) = decode_frame_base_local_index(*global_index) {
                        *printer += const_text("DW_OP_fbreg(local, ");
                        *printer += text(format!("{}{:+}", local_index, byte_offset));
                        *printer += const_text(")");
                    } else {
                        *printer += const_text("DW_OP_fbreg(global, ");
                        *printer += text(format!("{}{:+}", global_index, byte_offset));
                        *printer += const_text(")");
                    }
                }
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

/// High-bit marker used to carry a Wasm-local frame base through the existing
/// `FrameBase { global_index, byte_offset }` debug-location shape without
/// changing the VM-facing `DebugVarLocation` ABI.
///
/// Before MASM lowering completes, the low bits hold a raw Wasm local index.
/// After local patching, the low 16 bits hold the signed FMP-relative offset of
/// the Miden local containing the frame-base byte address.
pub const FRAME_BASE_LOCAL_MARKER: u32 = 1 << 31;

pub fn encode_frame_base_local_index(local_index: u32) -> Option<u32> {
    if local_index < FRAME_BASE_LOCAL_MARKER {
        Some(FRAME_BASE_LOCAL_MARKER | local_index)
    } else {
        None
    }
}

pub fn decode_frame_base_local_index(encoded: u32) -> Option<u32> {
    (encoded & FRAME_BASE_LOCAL_MARKER != 0).then_some(encoded & !FRAME_BASE_LOCAL_MARKER)
}

pub fn encode_frame_base_local_offset(local_offset: i16) -> u32 {
    FRAME_BASE_LOCAL_MARKER | u16::from_le_bytes(local_offset.to_le_bytes()) as u32
}

pub fn decode_frame_base_local_offset(encoded: u32) -> Option<i16> {
    if encoded & FRAME_BASE_LOCAL_MARKER == 0 {
        return None;
    }
    let low_bits = (encoded & 0xffff) as u16;
    Some(i16::from_le_bytes(low_bits.to_le_bytes()))
}
