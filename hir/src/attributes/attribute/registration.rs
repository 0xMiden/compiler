use alloc::{boxed::Box, format, rc::Rc};

use super::AttributeValue;
use crate::{Attribute, Context, Type, UnsafeIntrusiveEntityRef, traits::TraitInfo};

pub trait AttributeRegistration: Attribute {
    type Value: AttributeValue;

    /// The name of the dialect this attribute is declared part of
    fn dialect_name() -> ::midenc_hir_symbol::Symbol;
    /// The name of the attribute
    fn name() -> ::midenc_hir_symbol::Symbol;
    /// The fully-qualified name of the attribute (i.e. `<dialect>.<name>`)
    fn full_name() -> ::midenc_hir_symbol::Symbol {
        ::midenc_hir_symbol::Symbol::intern(format!(
            "{}.{}",
            Self::dialect_name(),
            <Self as AttributeRegistration>::name()
        ))
    }
    /// The set of statically known traits for this attribute
    fn traits() -> Box<[TraitInfo]>;
    /// Create a new instance of this attribute in the given context
    fn create<V>(context: &Rc<Context>, value: V, ty: Type) -> UnsafeIntrusiveEntityRef<Self>
    where
        Self::Value: From<V>;
    /// Create a new default-valued instance of this attribute in the given context
    fn create_default(context: &Rc<Context>) -> UnsafeIntrusiveEntityRef<Self>;
    /// Get a reference to the concrete value of this attribute
    fn underlying_value(attr: &Self) -> &Self::Value;
    /// Get a mutable reference the concrete value of this attribute
    fn underlying_value_mut(attr: &mut Self) -> &mut Self::Value;
}
