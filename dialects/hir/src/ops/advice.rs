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
///
/// Each input must occupy exactly one nonempty raw slot. Input types may differ; the four
/// results are Felt values in advice-word order, with element zero at the top.
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

impl Verify<dyn MemoryEffectOpInterface> for AdviceLoadWord {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        for (index, operand) in self.as_operation().operands().all().iter().enumerate() {
            let ty = operand.borrow().ty().clone();
            let raw = ty.clone().to_raw_parts().ok_or_else(|| {
                Report::msg(format!(
                    "invalid hir.advice_load_word operand {index} ({ty}): no raw representation"
                ))
            })?;
            if ty.size_in_bytes() == 0 || raw.len() != 1 {
                return Err(Report::msg(format!(
                    "invalid hir.advice_load_word operand {index} ({ty}): expected one nonempty \
                     raw slot"
                )));
            }
        }
        Ok(())
    }
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
                let value = context.make_result(span, Type::Felt, owner, i as u16);
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

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use midenc_hir::{dialects::builtin::BuiltinOpBuilder, testing::Test};

    use super::*;
    use crate::HirOpBuilder;

    fn verify_word(types: &[Type]) -> Result<(), Report> {
        let mut test = Test::new("advice_word", types, &vec![Type::Felt; 4]);
        let args = test
            .function()
            .borrow()
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|arg| *arg as ValueRef)
            .collect::<Vec<_>>();
        {
            let mut b = test.function_builder();
            let (a, b0, c, d) =
                b.advice_load_word(args[0], args[1], args[2], args[3], SourceSpan::UNKNOWN)?;
            assert!([a, b0, c, d].iter().all(|v| v.borrow().ty() == &Type::Felt));
            b.ret([a, b0, c, d], SourceSpan::UNKNOWN)?;
        }
        test.function().borrow().as_operation().recursively_verify()
    }

    #[test]
    fn advice_word_accepts_mixed_pointer_and_packed_source() {
        verify_word(&vec![Type::Felt; 4]).unwrap();
        verify_word(&[
            Type::U8,
            Type::from(PointerType::new(Type::U128)),
            Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)),
            Type::from(ArrayType::new(Type::U8, 4)),
        ])
        .unwrap();
        verify_word(&[Type::I1, Type::I16, Type::I32, Type::U32]).unwrap();
    }

    #[test]
    fn advice_word_rejects_wide_source() {
        for ty in [
            Type::U64,
            Type::I64,
            Type::U128,
            Type::from(ArrayType::new(Type::Felt, 4)),
            Type::Never,
            Type::from(ArrayType::new(Type::U8, 0)),
            Type::Unknown,
        ] {
            for index in 0..4 {
                let mut types = vec![Type::Felt; 4];
                types[index] = ty.clone();
                let error = format!("{}", verify_word(&types).unwrap_err());
                assert!(error.contains(&format!("operand {index}")), "{error}");
                if ty == Type::Unknown {
                    assert!(error.contains("no raw representation"), "{error}");
                } else {
                    assert!(error.contains("one nonempty raw slot"), "{error}");
                }
            }
        }
    }
}
