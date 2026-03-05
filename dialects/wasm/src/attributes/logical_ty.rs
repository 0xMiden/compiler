use alloc::boxed::Box;
use core::fmt;

use midenc_hir::{AttributeValue, Type, formatter};

/// Represents the logical types that Wasm `I32` operands can have.
///
/// For example, Wasm's `i32.extend8_s` interprets the operand's low 8 bits as `i8` value and
/// sign-extends it to `i32`. That is captured by `LogicalTyAttrI32::I8`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LogicalTyAttrI32 {
    I8,
    I16,
}

impl LogicalTyAttrI32 {
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
}

impl fmt::Display for LogicalTyAttrI32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::I8 => f.write_str("I8"),
            Self::I16 => f.write_str("I16"),
        }
    }
}

impl formatter::PrettyPrint for LogicalTyAttrI32 {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        display(self)
    }
}

impl AttributeValue for LogicalTyAttrI32 {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }

    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(*self)
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
            let actual = LogicalTyAttrI32::I8.sext(input as i32) as u32;
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
            let actual = LogicalTyAttrI32::I16.sext(input as i32) as u32;
            assert_eq!(actual, expected,);
        }
    }
}
