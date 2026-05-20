use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Pop one field element from the VM advice stack.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, AdviceEffectOpInterface, OpPrinter)
)]
pub struct AdvicePop {
    #[result]
    result: IntFelt,
}

impl EffectOpInterface<AdviceEffect> for AdvicePop {
    fn effects(&self) -> EffectIterator<AdviceEffect> {
        EffectIterator::from_smallvec(smallvec![
            EffectInstance::new_with_resource(AdviceEffect::Free, AdviceStackResource),
            EffectInstance::new_with_resource(AdviceEffect::Write, AdviceStackResource),
            EffectInstance::new_for_value_with_resource(
                AdviceEffect::Read,
                self.result().as_op_result_ref(),
                AdviceStackResource
            )
        ])
    }
}

impl InferTypeOpInterface for AdvicePop {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::Felt);
        Ok(())
    }
}

/// Pop one word from the VM advice stack, overwriting the top four operand stack slots.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, AdviceEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct AdviceLoadWord {
    #[operand]
    old0: AnyType,
    #[operand]
    old1: AnyType,
    #[operand]
    old2: AnyType,
    #[operand]
    old3: AnyType,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
}

impl EffectOpInterface<AdviceEffect> for AdviceLoadWord {
    fn effects(&self) -> EffectIterator<AdviceEffect> {
        EffectIterator::from_smallvec(smallvec![
            EffectInstance::new_with_resource(AdviceEffect::Free, AdviceStackResource),
            EffectInstance::new_with_resource(AdviceEffect::Write, AdviceStackResource),
            EffectInstance::new_for_value_with_resource(
                AdviceEffect::Read,
                self.result0().as_op_result_ref(),
                AdviceStackResource
            ),
            EffectInstance::new_for_value_with_resource(
                AdviceEffect::Read,
                self.result1().as_op_result_ref(),
                AdviceStackResource
            ),
            EffectInstance::new_for_value_with_resource(
                AdviceEffect::Read,
                self.result2().as_op_result_ref(),
                AdviceStackResource
            ),
            EffectInstance::new_for_value_with_resource(
                AdviceEffect::Read,
                self.result3().as_op_result_ref(),
                AdviceStackResource
            )
        ])
    }
}

impl InferTypeOpInterface for AdviceLoadWord {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result0_mut().set_type(Type::Felt);
        self.result1_mut().set_type(Type::Felt);
        self.result2_mut().set_type(Type::Felt);
        self.result3_mut().set_type(Type::Felt);
        Ok(())
    }
}

/// Pop two words from the VM advice stack, write them to memory, and update the stack window.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, AdviceEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct AdvicePipe {
    #[operands]
    stack: IntFelt,
    #[results]
    outputs: IntFelt,
}

impl EffectOpInterface<AdviceEffect> for AdvicePipe {
    fn effects(&self) -> EffectIterator<AdviceEffect> {
        EffectIterator::new(
            [
                EffectInstance::new_with_resource(AdviceEffect::Free, AdviceStackResource),
                EffectInstance::new_with_resource(AdviceEffect::Write, AdviceStackResource),
            ]
            .into_iter()
            .chain(self.outputs().iter().map(|out| {
                EffectInstance::new_for_value_with_resource(
                    AdviceEffect::Read,
                    *out,
                    AdviceStackResource,
                )
            })),
        )
    }
}

impl InferTypeOpInterface for AdvicePipe {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        const WINDOW_SIZE: usize = 13;

        if self.stack().len() != WINDOW_SIZE {
            return Err(Report::msg(format!(
                "invalid hir.advice_pipe: expected {WINDOW_SIZE} operand(s), but got {}",
                self.stack().len()
            )));
        }

        if !self.op.results.is_empty() && self.op.results.len() != WINDOW_SIZE {
            return Err(Report::msg(format!(
                "invalid hir.advice_pipe: expected {WINDOW_SIZE} result(s), but got {}",
                self.op.results.len()
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..WINDOW_SIZE {
                let value = context.make_result(span, Type::Felt, owner, i as u8);
                self.op.results.push(value);
            }
        } else {
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(Type::Felt);
            }
        }

        Ok(())
    }
}
