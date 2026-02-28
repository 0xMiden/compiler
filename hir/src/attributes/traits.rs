use super::{AttrRef, Attribute, AttributeRegistration};
use crate::{Immediate, parse};

/// When implemented on an attribute type, this function will be invoked to parse the contents
/// of the attribute data, if present.
pub trait AttrParser {
    fn parse(parser: &mut dyn parse::Parser<'_>) -> parse::ParseResult<AttrRef>;
}

/// Implemented on attribute types which are markers, which have the following properties:
///
/// * They have no concrete value, i.e. `()`
/// * The presence of the attribute is what makes it significant
///
/// This is automatically derived for all attributes that have a value type of `()`.
///
/// Marker types are always uniqued.
pub trait Marker {}

impl<T> Marker for T where T: AttributeRegistration<Value = ()> {}

/// Implemented by any attribute that represents an immediate integer value
pub trait IntegerLikeAttr: Attribute {
    /// Get the value of this attribute as an [Immediate]
    fn as_immediate(&self) -> Immediate;
    /// Set the value of this attribute to `value`, truncating on overflow.
    fn set_from_immediate_lossy(&mut self, value: Immediate);
}

/// Implemented by any attribute that represents an immediate boolean value
pub trait BoolLikeAttr: Attribute {
    /// Get the boolean value of this attribute
    fn as_bool(&self) -> bool;
    /// Set the underlying boolean value of this attribute to `value`
    fn set_bool(&mut self, value: bool);
}
