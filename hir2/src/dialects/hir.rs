mod builders;
mod ops;

use alloc::rc::Rc;
use core::cell::{Cell, RefCell};

pub use self::{
    builders::{DefaultInstBuilder, FunctionBuilder, InstBuilder, InstBuilderBase},
    ops::*,
};
use crate::{
    interner, AttributeValue, Builder, BuilderExt, Dialect, DialectName, DialectRegistration,
    Immediate, OperationName, OperationRef, SourceSpan, Type,
};

#[derive(Default)]
pub struct HirDialect {
    registered_ops: RefCell<Vec<OperationName>>,
    registered_op_cache: Cell<Option<Rc<[OperationName]>>>,
}

impl core::fmt::Debug for HirDialect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HirDialect")
            .field_with("registered_ops", |f| {
                f.debug_set().entries(self.registered_ops.borrow().iter()).finish()
            })
            .finish_non_exhaustive()
    }
}

impl HirDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops.borrow().len()
    }
}

impl Dialect for HirDialect {
    #[inline]
    fn name(&self) -> DialectName {
        DialectName::from_symbol(interner::symbols::Hir)
    }

    fn registered_ops(&self) -> Rc<[OperationName]> {
        let registered = unsafe { (*self.registered_op_cache.as_ptr()).clone() };
        if registered.as_ref().is_some_and(|ops| self.num_registered() == ops.len()) {
            registered.unwrap()
        } else {
            let registered = self.registered_ops.borrow();
            let ops = Rc::from(registered.clone().into_boxed_slice());
            self.registered_op_cache.set(Some(Rc::clone(&ops)));
            ops
        }
    }

    fn get_or_register_op(
        &self,
        opcode: midenc_hir_symbol::Symbol,
        register: fn(DialectName, midenc_hir_symbol::Symbol) -> crate::OperationName,
    ) -> crate::OperationName {
        let mut registered = self.registered_ops.borrow_mut();
        match registered.binary_search_by_key(&opcode, |op| op.name()) {
            Ok(index) => registered[index].clone(),
            Err(index) => {
                let name = register(self.name(), opcode);
                registered.insert(index, name.clone());
                name
            }
        }
    }

    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        attr: Box<dyn AttributeValue>,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        use crate::Op;

        // Save the current insertion point
        let mut builder = crate::InsertionGuard::new(builder);

        // Only integer constants are supported for now
        if !ty.is_integer() {
            return None;
        }

        // Currently, we expect folds to produce `Immediate`-valued attributes
        if let Some(&imm) = attr.downcast_ref::<Immediate>() {
            // If the immediate value is of the same type as the expected result type, we're ready
            // to materialize the constant
            let imm_ty = imm.ty();
            if &imm_ty == ty {
                let op_builder = builder.create::<Constant, _>(span);
                return op_builder(imm)
                    .ok()
                    .map(|op| op.borrow().as_operation().as_operation_ref());
            }

            // The immediate value has a different type than expected, but we can coerce types, so
            // long as the value fits in the target type
            if imm_ty.size_in_bits() > ty.size_in_bits() {
                return None;
            }

            let imm = match ty {
                Type::I8 => match imm {
                    Immediate::I1(value) => Immediate::I8(value as i8),
                    Immediate::U8(value) => Immediate::I8(i8::try_from(value).ok()?),
                    _ => return None,
                },
                Type::U8 => match imm {
                    Immediate::I1(value) => Immediate::U8(value as u8),
                    Immediate::I8(value) => Immediate::U8(u8::try_from(value).ok()?),
                    _ => return None,
                },
                Type::I16 => match imm {
                    Immediate::I1(value) => Immediate::I16(value as i16),
                    Immediate::I8(value) => Immediate::I16(value as i16),
                    Immediate::U8(value) => Immediate::I16(value.into()),
                    Immediate::U16(value) => Immediate::I16(i16::try_from(value).ok()?),
                    _ => return None,
                },
                Type::U16 => match imm {
                    Immediate::I1(value) => Immediate::U16(value as u16),
                    Immediate::I8(value) => Immediate::U16(u16::try_from(value).ok()?),
                    Immediate::U8(value) => Immediate::U16(value as u16),
                    Immediate::I16(value) => Immediate::U16(u16::try_from(value).ok()?),
                    _ => return None,
                },
                Type::I32 => Immediate::I32(imm.as_i32()?),
                Type::U32 => Immediate::U32(imm.as_u32()?),
                Type::I64 => Immediate::I64(imm.as_i64()?),
                Type::U64 => Immediate::U64(imm.as_u64()?),
                Type::I128 => Immediate::I128(imm.as_i128()?),
                Type::U128 => Immediate::U128(imm.as_u128()?),
                Type::Felt => Immediate::Felt(imm.as_felt()?),
                ty => unimplemented!("unrecognized integral type '{ty}'"),
            };

            let op_builder = builder.create::<Constant, _>(span);
            return op_builder(imm).ok().map(|op| op.borrow().as_operation().as_operation_ref());
        }

        None
    }
}

impl DialectRegistration for HirDialect {
    const NAMESPACE: &'static str = "hir";

    #[inline]
    fn init() -> Self {
        Self::default()
    }
}
