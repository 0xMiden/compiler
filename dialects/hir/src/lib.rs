#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![no_std]
#![deny(warnings)]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

pub mod analyses;
pub mod assertions;
mod attributes;
mod builders;
mod canonicalization;
mod ops;
pub mod transforms;

use midenc_dialect_arith as arith;
use midenc_hir::{
    AttributeRef, Builder, BuilderExt, Dialect, DialectInfo, OperationRef, SourceSpan, Type,
    attributes::IntegerLikeAttr, derive::DialectRegistration,
};

pub use self::{attributes::*, builders::HirOpBuilder, ops::*};

/// Attribute marking a lifted component export as the note script executed by the transaction
/// kernel.
///
/// Set by frontend component-export lifting; consumed by MASM codegen, which converts it into a
/// MASM procedure attribute and requires it of the callee of every [`ProcedureRoot`] op marked
/// [`ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR`].
pub const NOTE_SCRIPT_EXPORT_ATTR: &str = "note_script";

/// Attribute marking a lifted component export as an account authentication script.
///
/// Set by frontend component-export lifting; consumed by MASM codegen, which converts it into a
/// MASM procedure attribute.
pub const AUTH_SCRIPT_EXPORT_ATTR: &str = "auth_script";

/// Attribute marking a lifted component export as an account procedure: part of the account
/// interface callable from transaction scripts, notes, FPI, and sibling components.
///
/// Set by frontend component-export lifting; consumed by MASM codegen, which converts it into a
/// MASM procedure attribute.
pub const ACCOUNT_PROCEDURE_EXPORT_ATTR: &str = "account_procedure";

/// Attribute marking a lifted component export as a transaction script.
///
/// Set by frontend component-export lifting; consumed by MASM codegen, which converts it into a
/// MASM procedure attribute.
pub const TRANSACTION_SCRIPT_EXPORT_ATTR: &str = "transaction_script";

/// Attribute marking a translated core Wasm function as the component start function.
///
/// Set by the Wasm component frontend when folding the state-free startup adapter emitted by
/// rustc's component linker. Consumed by MASM codegen, which executes the marked function as the
/// final phase of per-context component initialization. The marker's value must be a
/// `builtin::UnitAttr`.
///
/// The marker must be attached to the resolved `builtin::Function` definition, never a
/// `builtin::FunctionAlias`, even when the startup adapter imports a secondary export name.
pub const WASM_COMPONENT_START_ATTR: &str = "wasm_component_start";

#[derive(Debug, DialectRegistration)]
pub struct HirDialect {
    info: DialectInfo,
}

impl From<DialectInfo> for HirDialect {
    fn from(info: DialectInfo) -> Self {
        Self { info }
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
        attr: AttributeRef,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        // Save the current insertion point
        let mut builder = midenc_hir::InsertionGuard::new(builder);

        // Check for `PointerAttr`
        if let Ok(attr) = attr.try_downcast_attr::<PointerAttr>() {
            let Type::Ptr(pointer_type) = ty else {
                panic!("unexpected pointer constant given when materializing non-pointer value");
            };
            let attr =
                attributes::Pointer::new(attr.borrow().addr(), pointer_type.as_ref().clone());
            let op_builder = builder.create::<ConstantPointer, _>(span);
            return op_builder(attr).ok().map(|op| op.as_operation_ref());
        }

        // If we want an integer constant, delegate to the arith dialect
        if ty.is_integer() {
            let dialect = builder.context().get_or_register_dialect::<arith::ArithDialect>();
            return dialect.materialize_constant(&mut builder, attr, ty, span);
        }

        // Only pointer constants are supported here for now
        let Type::Ptr(pointer_type) = ty else {
            return None;
        };

        // Currently, we expect folds to produce `IntegerLikeAttr`-valued attributes
        let attr = attr.borrow();
        if let Some(imm) = attr.as_attr().as_trait::<dyn IntegerLikeAttr>() {
            // We're materializing a constant pointer from an integer immediate
            if let Some(addr) = imm.as_immediate().as_u32() {
                let attr = attributes::Pointer::new(addr, pointer_type.as_ref().clone());
                let op_builder = builder.create::<ConstantPointer, _>(span);
                op_builder(attr).ok().map(|op| op.as_operation_ref())
            } else {
                // Invalid pointer immediate
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod pointer_constant_tests {
    use alloc::rc::Rc;

    use midenc_hir::{
        AddressSpace, Context, OpBuilder, PointerType, Value, attributes::InferAttributeValueType,
        dialects::builtin::attributes::U32Attr,
    };

    use super::*;

    #[test]
    fn pointer_constant_materialization_preserves_requested_complete_type() {
        let context = Rc::new(Context::default());
        let dialect = context.get_or_register_dialect::<HirDialect>();
        let mut builder = OpBuilder::new(context.clone());
        for space in [AddressSpace::Byte, AddressSpace::Element] {
            for pointee in [
                Type::U32,
                PointerType::new(Type::U8).into(),
                PointerType::new_with_address_space(Type::U8, AddressSpace::Element).into(),
            ] {
                let ty = Type::from(PointerType::new_with_address_space(pointee, space));
                let integer: AttributeRef =
                    context.create_attribute::<U32Attr, _>(u32::MAX).as_attribute_ref();
                let pointer: AttributeRef = context
                    .create_attribute::<PointerAttr, _>(Pointer::new(
                        u32::MAX,
                        PointerType::new(Type::U8),
                    ))
                    .as_attribute_ref();
                for attr in [integer, pointer] {
                    let op = dialect
                        .materialize_constant(&mut builder, attr, &ty, SourceSpan::UNKNOWN)
                        .unwrap();
                    let op = op.borrow();
                    let constant = op.downcast_ref::<ConstantPointer>().unwrap();
                    assert_eq!(constant.get_value().addr(), u32::MAX);
                    assert_eq!(constant.result().ty(), &ty);
                    assert_eq!(constant.get_value().infer_type_from_value(), ty);
                }
            }
        }
    }
}
