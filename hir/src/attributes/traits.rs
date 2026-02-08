use super::{AttrRef, Attribute};
use crate::{Immediate, parse};

/// Implemented by any attribute that represents an immediate integer value
pub trait IntegerLikeAttr: Attribute {
    /// Get the value of this attribute as an [Immediate]
    fn as_immediate(&self) -> Immediate;
    /// Set the value of this attribute to `value`, truncating on overflow.
    fn set_from_immediate_lossy(&mut self, value: Immediate);
}

/// When implemented on an attribute type, this function will be invoked to parse the contents
/// of the attribute data, if present.
pub trait AttrParser {
    fn parse(parser: &mut dyn parse::Parser<'_>) -> parse::ParseResult<AttrRef>;
}
