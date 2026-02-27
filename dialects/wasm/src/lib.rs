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

use midenc_dialect_arith as arith;
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
        builder: &mut dyn Builder,
        attr: Box<dyn AttributeValue>,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        // Save the current insertion point
        let mut builder = midenc_hir::InsertionGuard::new(builder);

        // For now only integer constants are supported. Delegate them to the arith dialect
        if ty.is_integer() {
            let dialect = builder.context().get_or_register_dialect::<arith::ArithDialect>();
            return dialect.materialize_constant(&mut builder, attr, ty, span);
        }

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
