#![no_std]
#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

use alloc::boxed::Box;

mod attributes;
mod builders;
mod ops;

use midenc_hir2::{
    AttributeValue, Builder, BuilderExt, Dialect, DialectInfo, DialectRegistration, OperationRef,
    SourceSpan, Type,
};

pub use self::{attributes::PoisonAttr, builders::UndefinedBehaviorOpBuilder, ops::*};

#[derive(Debug)]
pub struct UndefinedBehaviorDialect {
    info: DialectInfo,
}

impl UndefinedBehaviorDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for UndefinedBehaviorDialect {
    #[inline]
    fn info(&self) -> &DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        attr: Box<dyn AttributeValue>,
        _ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        if let Some(attr) = attr.downcast_ref::<PoisonAttr>() {
            let op_builder = builder.create::<Poison, _>(span);
            return op_builder(attr.clone()).ok().map(|op| op.as_operation_ref());
        }
        None
    }
}

impl DialectRegistration for UndefinedBehaviorDialect {
    const NAMESPACE: &'static str = "ub";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::Poison>();
        info.register_operation::<ops::Unreachable>();
    }
}
