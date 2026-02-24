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

use alloc::boxed::Box;

mod builders;
mod ops;

use midenc_hir::{
    AttributeValue, Builder, Dialect, DialectInfo, DialectRegistration, OperationRef, SourceSpan,
    Type,
};

pub use self::{builders::WasmOpBuilder, ops::*};

#[derive(Debug)]
pub struct WasmDialect {
    info: DialectInfo,
}

impl WasmDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for WasmDialect {
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
        // TODO implement
        None
    }
}

impl DialectRegistration for WasmDialect {
    const NAMESPACE: &'static str = "wasm";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::I32Extend8S>();
    }
}
