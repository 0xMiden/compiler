mod builders;
mod ops;

pub use self::{
    builders::{BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder, WorldBuilder},
    ops::*,
};
use crate::{
    AttributeValue, Builder, Dialect, DialectInfo, DialectRegistration, OperationRef, SourceSpan,
    Type,
};

#[derive(Debug)]
pub struct BuiltinDialect {
    info: DialectInfo,
}

impl BuiltinDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

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
        info.register_operation::<ops::Ret>();
        info.register_operation::<ops::RetImm>();
    }
}

impl Dialect for BuiltinDialect {
    #[inline]
    fn info(&self) -> &DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        _builder: &mut dyn Builder,
        _attr: Box<dyn AttributeValue>,
        _ty: &Type,
        _span: SourceSpan,
    ) -> Option<OperationRef> {
        None
    }
}
