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

mod builders;
mod canonicalization;
mod ops;

use midenc_hir::{
    AttributeValue, Builder, Dialect, DialectInfo, DialectRegistration, OperationRef, SourceSpan,
    Type,
};

pub use self::{builders::ControlFlowOpBuilder, ops::*};

#[derive(Debug)]
pub struct ControlFlowDialect {
    info: DialectInfo,
}

impl ControlFlowDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for ControlFlowDialect {
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

impl DialectRegistration for ControlFlowDialect {
    const NAMESPACE: &'static str = "cf";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::Br>();
        info.register_operation::<ops::CondBr>();
        info.register_operation::<ops::Switch>();
        info.register_operation::<ops::Select>();
    }
}
