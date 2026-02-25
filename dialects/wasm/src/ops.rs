use alloc::boxed::Box;

use midenc_hir::{
    derive::operation, effects::MemoryEffectOpInterface, matchers::Matcher, traits::*, *,
};

use crate::WasmDialect;

macro_rules! has_no_effects {
    ($Op:ty) => {
        impl ::midenc_hir::effects::EffectOpInterface<::midenc_hir::effects::MemoryEffect> for $Op {
            fn has_no_effect(&self) -> bool {
                true
            }

            fn effects(
                &self,
            ) -> ::midenc_hir::effects::EffectIterator<::midenc_hir::effects::MemoryEffect> {
                ::midenc_hir::effects::EffectIterator::from_smallvec(::midenc_hir::smallvec![])
            }
        }
    };
}

#[operation(
    dialect = WasmDialect,
    traits(UnaryOp),
    implements(UnaryOp, InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct I32Extend8S {
    #[operand]
    operand: Int32,
    #[result]
    result: Int32,
}

has_no_effects!(I32Extend8S);

impl InferTypeOpInterface for I32Extend8S {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::I32);
        Ok(())
    }
}

impl Foldable for I32Extend8S {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.operand().as_operand_ref())
        {
            let extended = value.as_i32().map(|v| Immediate::I32((v as i8) as i32));

            if let Some(extended) = extended {
                *value = extended;
                results.push(OpFoldResult::Attribute(value));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            let extended = value.as_i32().map(|v| Immediate::I32((v as i8) as i32));

            if let Some(extended) = extended {
                results.push(OpFoldResult::Attribute(Box::new(extended)));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }
}
