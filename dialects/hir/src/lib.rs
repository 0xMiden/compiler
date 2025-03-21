#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![no_std]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

pub mod assertions;
mod attributes;
mod builders;
mod ops;
pub mod transforms;

use alloc::boxed::Box;

use midenc_dialect_arith as arith;
use midenc_hir::{
    AttributeValue, Builder, BuilderExt, Dialect, DialectInfo, DialectRegistration, Immediate,
    OperationRef, SourceSpan, Type,
};

pub use self::{attributes::*, builders::HirOpBuilder, ops::*};

#[derive(Debug)]
pub struct HirDialect {
    info: DialectInfo,
}

impl HirDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for HirDialect {
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

        // Check for `PointerAttr`
        if let Some(attr) = attr.downcast_ref::<PointerAttr>() {
            let pointee_type = ty
                .pointee()
                .expect("unexpected pointer constant given when materializing non-pointer value")
                .clone();
            let mut attr = attr.clone();
            attr.set_pointee_type(pointee_type);
            let op_builder = builder.create::<ConstantPointer, _>(span);
            return op_builder(attr).ok().map(|op| op.as_operation_ref());
        }

        // If we want an integer constant, delegate to the arith dialect
        if ty.is_integer() {
            let dialect = builder.context().get_or_register_dialect::<arith::ArithDialect>();
            return dialect.materialize_constant(&mut builder, attr, ty, span);
        }

        // Only pointer constants are supported here for now
        if !ty.is_pointer() {
            return None;
        }

        // Currently, we expect folds to produce `Immediate`-valued attributes for integer-likes
        if let Some(&imm) = attr.downcast_ref::<Immediate>() {
            // We're materializing a constant pointer from a integer immediate
            if let Some(pointee_type) = ty.pointee() {
                if let Some(addr) = imm.as_u32() {
                    let op_builder = builder.create::<ConstantPointer, _>(span);
                    let attr = PointerAttr::new(Immediate::U32(addr), pointee_type.clone());
                    return op_builder(attr).ok().map(|op| op.as_operation_ref());
                } else {
                    // Invalid pointer immediate
                    return None;
                }
            }
        }

        None
    }
}

impl DialectRegistration for HirDialect {
    const NAMESPACE: &'static str = "hir";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::Assert>();
        info.register_operation::<ops::Assertz>();
        info.register_operation::<ops::AssertEq>();
        info.register_operation::<ops::PtrToInt>();
        info.register_operation::<ops::IntToPtr>();
        info.register_operation::<ops::Cast>();
        info.register_operation::<ops::Bitcast>();
        info.register_operation::<ops::ConstantBytes>();
        info.register_operation::<ops::Exec>();
        info.register_operation::<ops::Call>();
        info.register_operation::<ops::Store>();
        info.register_operation::<ops::StoreLocal>();
        info.register_operation::<ops::Load>();
        info.register_operation::<ops::LoadLocal>();
        info.register_operation::<ops::MemGrow>();
        info.register_operation::<ops::MemSize>();
        info.register_operation::<ops::MemSet>();
        info.register_operation::<ops::MemCpy>();
        info.register_operation::<ops::Spill>();
        info.register_operation::<ops::Reload>();
    }
}
