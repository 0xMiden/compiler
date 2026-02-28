pub mod attributes;
mod builders;
mod ops;

pub use self::{
    builders::{BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder, WorldBuilder},
    ops::*,
};
use crate::{
    Dialect, DialectInfo,
    derive::{Dialect, DialectRegistration},
};

#[derive(Dialect, DialectRegistration, Debug)]
pub struct BuiltinDialect {
    #[dialect(info)]
    info: DialectInfo,
}

impl BuiltinDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

/*
impl DialectRegistration for BuiltinDialect {
    const NAMESPACE: &'static str = "builtin";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::World>();
        info.register_operation::<ops::Component>();
        info.register_operation::<ops::Module>();
        info.register_operation::<ops::Function>();
        info.register_operation::<ops::GlobalVariable>();
        info.register_operation::<ops::GlobalSymbol>();
        info.register_operation::<ops::Segment>();
        info.register_operation::<ops::UnrealizedConversionCast>();
        info.register_operation::<ops::Ret>();
        info.register_operation::<ops::RetImm>();
    }

    fn register_attributes(info: &mut DialectInfo) {
        info.register_attribute::<attributes::BoolAttr>();
        info.register_attribute::<attributes::BytesAttr>();
        info.register_attribute::<attributes::I8Attr>();
        info.register_attribute::<attributes::U8Attr>();
        info.register_attribute::<attributes::I16Attr>();
        info.register_attribute::<attributes::U16Attr>();
        info.register_attribute::<attributes::I32Attr>();
        info.register_attribute::<attributes::U32Attr>();
        info.register_attribute::<attributes::I64Attr>();
        info.register_attribute::<attributes::U64Attr>();
        info.register_attribute::<attributes::I128Attr>();
        info.register_attribute::<attributes::U128Attr>();
        info.register_attribute::<attributes::ImmediateAttr>();
        info.register_attribute::<attributes::IdentAttr>();
        info.register_attribute::<attributes::LocationAttr>();
        info.register_attribute::<attributes::OverflowAttr>();
        info.register_attribute::<attributes::StringAttr>();
        info.register_attribute::<attributes::SymbolRefAttr>();
        info.register_attribute::<attributes::TypeAttr>();
        info.register_attribute::<attributes::FunctionTypeAttr>();
        info.register_attribute::<attributes::UnitAttr>();
        info.register_attribute::<attributes::VersionAttr>();
        info.register_attribute::<attributes::VisibilityAttr>();
        info.register_attribute::<attributes::SignatureAttr>();
        info.register_attribute::<attributes::SretAttr>();
        info.register_attribute::<attributes::ZextAttr>();
        info.register_attribute::<attributes::SextAttr>();
    }
}
*/
