use core::{fmt, str::FromStr};

use midenc_hir::{
    AttrPrinter, SmallVec, Type, attributes::AttrParser, derive::DialectAttribute,
    print::AsmPrinter,
};

use crate::WasmDialect;

/// Represents the logical types that Wasm `I32` operands can have.
///
/// For example, Wasm's `i32.extend8_s` interprets the operand's low 8 bits as `i8` value and
/// sign-extends it to `i32`. That is captured by `LogicalTyI32::I8`.
#[derive(DialectAttribute, Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[attribute(dialect = WasmDialect, implements(AttrPrinter))]
#[repr(u8)]
pub enum LogicalTyI32 {
    // TODO try not having default
    #[default]
    I8,
    I16,
}

impl LogicalTyI32 {
    pub fn ty(&self) -> Type {
        match self {
            Self::I8 => Type::I8,
            Self::I16 => Type::I16,
        }
    }

    /// Interprets `x` as value of the logical type and sign-extends it to `i32`.
    pub fn sext(&self, x: i32) -> i32 {
        match self {
            Self::I8 => (x as i8) as i32,
            Self::I16 => (x as i16) as i32,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
        }
    }
}

impl AttrPrinter for LogicalTyI32Attr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_keyword(self.value.as_str());
    }
}

impl AttrParser for LogicalTyI32 {
    fn parse(
        parser: &mut dyn midenc_hir::parse::Parser<'_>,
    ) -> midenc_hir::parse::ParseResult<midenc_hir::AttributeRef> {
        use midenc_hir::parse::Token;

        let keywords = SmallVec::<[Token; 2]>::from_iter(
            ([LogicalTyI32::I8, LogicalTyI32::I16])
                .iter()
                .map(LogicalTyI32::as_str)
                .map(Token::BareIdent),
        );

        let logical_ty = parser.parse_keyword_from(&keywords)?;
        let visibility = logical_ty.as_str().parse::<LogicalTyI32>().unwrap();

        let attr = parser.context_rc().create_attribute::<LogicalTyI32Attr, _>(visibility);
        Ok(attr)
    }
}

impl fmt::Display for LogicalTyI32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::I8 => f.write_str("i8"),
            Self::I16 => f.write_str("i16"),
        }
    }
}

impl AsRef<str> for LogicalTyI32 {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for LogicalTyI32 {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "i8" => Ok(Self::I8),
            "i16" => Ok(Self::I16),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_logical_ty_i32_sext_i8() {
        let cases: Vec<(u32, u32)> = Vec::from([
            (
                0b0000_0000_0000_0000_0000_0000_0000_0000,
                0b0000_0000_0000_0000_0000_0000_0000_0000,
            ),
            (
                0b0000_0000_0000_0000_0000_0000_0000_0001,
                0b0000_0000_0000_0000_0000_0000_0000_0001,
            ),
            (
                0b0000_0000_0000_0000_0000_0000_0111_1111,
                0b0000_0000_0000_0000_0000_0000_0111_1111,
            ),
            (
                0b0000_0000_0000_0000_0000_0000_1000_0000,
                0b1111_1111_1111_1111_1111_1111_1000_0000,
            ),
            (
                0b0000_0000_0000_0000_0000_0000_1111_1111,
                0b1111_1111_1111_1111_1111_1111_1111_1111,
            ),
            (
                0b0000_0000_0000_0000_0001_0010_0011_0100,
                0b0000_0000_0000_0000_0000_0000_0011_0100,
            ),
            (
                0b1010_1011_1100_1101_1110_1111_0111_1111,
                0b0000_0000_0000_0000_0000_0000_0111_1111,
            ),
        ]);

        for (input, expected) in cases {
            let actual = LogicalTyI32::I8.sext(input as i32) as u32;
            assert_eq!(actual, expected,);
        }
    }

    #[test]
    fn test_logical_ty_i32_sext_i16() {
        let cases: Vec<(u32, u32)> = Vec::from([
            (
                0b0000_0000_0000_0000_0000_0000_0000_0000,
                0b0000_0000_0000_0000_0000_0000_0000_0000,
            ),
            (
                0b0000_0000_0000_0000_0000_0000_0000_0001,
                0b0000_0000_0000_0000_0000_0000_0000_0001,
            ),
            (
                0b0000_0000_0000_0000_0111_1111_1111_1111,
                0b0000_0000_0000_0000_0111_1111_1111_1111,
            ),
            (
                0b0000_0000_0000_0000_1000_0000_0000_0000,
                0b1111_1111_1111_1111_1000_0000_0000_0000,
            ),
            (
                0b0000_0000_0000_0000_1111_1111_1111_1111,
                0b1111_1111_1111_1111_1111_1111_1111_1111,
            ),
            (
                0b0001_0010_0011_0100_0101_0110_0111_1000,
                0b0000_0000_0000_0000_0101_0110_0111_1000,
            ),
            (
                0b1010_1011_1100_1101_1110_1111_0111_1111,
                0b1111_1111_1111_1111_1110_1111_0111_1111,
            ),
        ]);

        for (input, expected) in cases {
            let actual = LogicalTyI32::I16.sext(input as i32) as u32;
            assert_eq!(actual, expected,);
        }
    }
}
