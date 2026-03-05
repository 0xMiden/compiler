#![no_std]
#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![deny(warnings)]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

mod attributes;
mod builders;
mod ops;

use midenc_hir::{
    AttributeRef, Builder, BuilderExt, Dialect, DialectInfo, OperationRef, SourceSpan, Type,
    derive::DialectRegistration,
};

pub use self::{attributes::PoisonAttr, builders::UndefinedBehaviorOpBuilder, ops::*};

#[derive(Debug, DialectRegistration)]
#[dialect(name = "ub")]
pub struct UndefinedBehaviorDialect {
    info: DialectInfo,
}

impl From<DialectInfo> for UndefinedBehaviorDialect {
    fn from(info: DialectInfo) -> Self {
        Self { info }
    }
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
        attr: AttributeRef,
        _ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        if let Ok(poison_attr) = attr.try_downcast::<PoisonAttr>() {
            let poison_value = poison_attr.borrow().as_value().clone();
            let op_builder = builder.create::<Poison, _>(span);
            return op_builder(poison_value).ok().map(|op| op.as_operation_ref());
        }
        None
    }
}

#[cfg(false)]
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

    fn register_attributes(info: &mut DialectInfo) {
        info.register_attribute::<attributes::PoisonAttr>();
    }
}
