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
pub mod transforms;

use midenc_hir2::{
    AttributeValue, Builder, Dialect, DialectInfo, DialectRegistration, OperationRef, SourceSpan,
    Type,
};

pub use self::{builders::StructuredControlFlowOpBuilder, ops::*};

#[derive(Debug)]
pub struct ScfDialect {
    info: DialectInfo,
}

impl ScfDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for ScfDialect {
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

impl DialectRegistration for ScfDialect {
    const NAMESPACE: &'static str = "scf";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::If>();
        info.register_operation::<ops::While>();
        info.register_operation::<ops::IndexSwitch>();
        info.register_operation::<ops::Condition>();
        info.register_operation::<ops::Yield>();
    }
}
