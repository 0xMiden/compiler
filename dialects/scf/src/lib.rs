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

mod builders;
mod canonicalization;
mod ops;
pub mod transforms;

use midenc_hir::{
    AttributeRef, Builder, Dialect, DialectInfo, OperationRef, SourceSpan, Type,
    derive::DialectRegistration,
};

pub use self::{builders::StructuredControlFlowOpBuilder, ops::*};

#[derive(Debug, DialectRegistration)]
pub struct ScfDialect {
    info: DialectInfo,
}

impl From<DialectInfo> for ScfDialect {
    fn from(info: DialectInfo) -> Self {
        Self { info }
    }
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
        _attr: AttributeRef,
        _ty: &Type,
        _span: SourceSpan,
    ) -> Option<OperationRef> {
        None
    }
}

#[cfg(false)]
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
