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
mod mem;
mod ops;

use midenc_dialect_arith as arith;
use midenc_hir::{
    AttributeRef, Builder, Dialect, DialectInfo, OperationRef, SourceSpan, Type,
    derive::DialectRegistration,
};

pub use self::{builders::WasmOpBuilder, mem::prepare_addr, ops::*};

#[derive(Debug, DialectRegistration)]
pub struct WasmDialect {
    info: DialectInfo,
}

impl From<DialectInfo> for WasmDialect {
    fn from(info: DialectInfo) -> Self {
        Self { info }
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
        attr: AttributeRef,
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
