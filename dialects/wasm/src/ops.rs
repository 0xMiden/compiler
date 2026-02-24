use alloc::{boxed::Box, format};

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

// TODO make it more general to handle other sign extension ops too, see
// https://github.com/WebAssembly/spec/blob/main/proposals/sign-extension-ops/Overview.md
// TODO implement eval?
#[operation(
    dialect = WasmDialect,
    traits(UnaryOp),
    implements(UnaryOp, InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct I32Extend8S {
    #[operand]
    operand: AnySignedInteger,
    #[result]
    result: AnySignedInteger,
}

has_no_effects!(I32Extend8S);

impl InferTypeOpInterface for I32Extend8S {
    // TODO type check via trait based mechanisms during op construction instead of here
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let operand_ty = self.operand().as_value_ref().borrow().ty().clone();
        let operand_size = operand_ty.size_in_bits();
        if operand_size > 32 {
            return Err(Report::msg(format!(
                "invalid operation wasm.i32_extend_8s: expected operand type width <= 32 bits, \
                 but got '{operand_ty}' ({operand_size} bits)"
            )));
        }

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
