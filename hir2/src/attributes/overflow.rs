use core::fmt;

use crate::define_attr_type;

/// This enumeration represents the various ways in which arithmetic operations
/// can be configured to behave when either the operands or results over/underflow
/// the range of the integral type.
///
/// Always check the documentation of the specific instruction involved to see if there
/// are any specific differences in how this enum is interpreted compared to the default
/// meaning of each variant.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Typically, this means the operation is performed using the equivalent field element
    /// operation, rather than a dedicated operation for the given type. Because of this, the
    /// result of the operation may exceed that of the integral type expected, but this will
    /// not be caught right away.
    ///
    /// It is the callers responsibility to ensure that resulting value is in range.
    #[default]
    Unchecked,
    /// The operation will trap if the operands, or the result, is not valid for the range of the
    /// integral type involved, e.g. u32.
    Checked,
    /// The operation will wrap around, depending on the range of the integral type. For example,
    /// given a u32 value, this is done by applying `mod 2^32` to the result.
    Wrapping,
    /// The result of the operation will be computed as in [Wrapping], however in addition to the
    /// result, this variant also pushes a value on the stack which represents whether or not the
    /// operation over/underflowed; either 1 if over/underflow occurred, or 0 otherwise.
    Overflowing,
}
impl Overflow {
    /// Returns true if overflow is unchecked
    pub fn is_unchecked(&self) -> bool {
        matches!(self, Self::Unchecked)
    }

    /// Returns true if overflow will cause a trap
    pub fn is_checked(&self) -> bool {
        matches!(self, Self::Checked)
    }

    /// Returns true if overflow will add an extra boolean on top of the stack
    pub fn is_overflowing(&self) -> bool {
        matches!(self, Self::Overflowing)
    }
}
impl fmt::Display for Overflow {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unchecked => f.write_str("unchecked"),
            Self::Checked => f.write_str("checked"),
            Self::Wrapping => f.write_str("wrapping"),
            Self::Overflowing => f.write_str("overflow"),
        }
    }
}
impl crate::formatter::PrettyPrint for Overflow {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}

define_attr_type!(Overflow);
