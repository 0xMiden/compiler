use core::{fmt, str::FromStr};

use smallvec::SmallVec;

use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

/// This enumeration represents the various ways in which arithmetic operations
/// can be configured to behave when either the operands or results over/underflow
/// the range of the integral type.
///
/// Always check the documentation of the specific instruction involved to see if there
/// are any specific differences in how this enum is interpreted compared to the default
/// meaning of each variant.
#[derive(DialectAttribute, Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
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
    /// The result of the operation will be computed as in `Wrapping`, however in addition to the
    /// result, this variant also pushes a value on the stack which represents whether or not the
    /// operation over/underflowed; either 1 if over/underflow occurred, or 0 otherwise.
    Overflowing,
}

impl AttrPrinter for OverflowAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_keyword(self.value.as_str());
    }
}

impl AttrParser for OverflowAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Token;

        let keywords = SmallVec::<[Token; 4]>::from_iter(
            ([
                Overflow::Unchecked,
                Overflow::Checked,
                Overflow::Wrapping,
                Overflow::Overflowing,
            ])
            .iter()
            .map(Overflow::as_str)
            .map(Token::BareIdent),
        );

        let overflow = parser.parse_keyword_from(&keywords)?;
        let overflow = overflow.as_str().parse::<Overflow>().unwrap();

        let attr = parser.context_rc().create_attribute::<OverflowAttr, _>(overflow);
        Ok(attr)
    }
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

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unchecked => "unchecked",
            Self::Checked => "checked",
            Self::Wrapping => "wrapping",
            Self::Overflowing => "overflow",
        }
    }
}

impl FromStr for Overflow {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unchecked" => Ok(Self::Unchecked),
            "checked" => Ok(Self::Checked),
            "wrapping" => Ok(Self::Wrapping),
            "overflowing" => Ok(Self::Overflowing),
            _ => Err("unknown overflow type"),
        }
    }
}

impl AsRef<str> for Overflow {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Overflow {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl crate::formatter::PrettyPrint for Overflow {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}
