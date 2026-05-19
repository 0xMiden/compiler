use alloc::vec::Vec;

use midenc_hir::{
    CallOpInterface, Operation, Symbol, Type,
    dialects::builtin,
    effects::{AdviceEffect, AdviceEffectOpInterface},
    traits::{
        ValueRangeConstraint, is_unconstrained_value_type, operation_operand_range_requirement,
    },
};

pub(super) fn is_external_call(call: &dyn CallOpInterface) -> bool {
    let Some(callee) = call.resolve() else {
        return true;
    };
    let callee = callee.borrow();
    callee
        .as_symbol_operation()
        .downcast_ref::<builtin::Function>()
        .is_some_and(Symbol::is_declaration)
}

pub(super) fn external_call_param_types(call: &dyn CallOpInterface) -> Option<Vec<Type>> {
    let callee = call.resolve()?;
    let callee = callee.borrow();
    let function = callee.as_symbol_operation().downcast_ref::<builtin::Function>()?;
    Some(function.get_signature().params().iter().map(|param| param.ty.clone()).collect())
}

pub(super) fn external_call_result_has_unconstrained_advice_effect(
    call: &dyn CallOpInterface,
    result_index: usize,
) -> bool {
    let Some(callee) = call.resolve() else {
        return false;
    };
    let callee = callee.borrow();
    let Some(function) = callee.as_symbol_operation().downcast_ref::<builtin::Function>() else {
        return false;
    };
    if !function.is_declaration() {
        return false;
    }

    function.advice_effects().as_value().iter().any(|effect| {
        effect.effect == AdviceEffect::Read
            && effect
                .result
                .is_none_or(|effect_result| usize::from(effect_result) == result_index)
    })
}

pub(super) fn external_parameter_range_constraint(ty: &Type) -> Option<ValueRangeConstraint> {
    ValueRangeConstraint::from_type(ty)
}

pub(super) fn is_unconstrained_external_result_type(ty: &Type) -> bool {
    is_unconstrained_value_type(ty)
}

pub(super) fn is_range_constrained_sink(op: &Operation) -> bool {
    op.operands()
        .iter()
        .enumerate()
        .any(|(index, _)| operation_operand_range_requirement(op, index).is_some())
}

pub(super) fn range_constrained_operand_indices(op: &Operation) -> Vec<usize> {
    op.operands()
        .iter()
        .enumerate()
        .filter_map(|(index, _)| operation_operand_range_requirement(op, index).map(|_| index))
        .collect()
}

pub(super) fn operation_result_has_advice_read_effect(
    op: &Operation,
    result: midenc_hir::ValueRef,
) -> bool {
    let Some(interface) = op.as_trait::<dyn AdviceEffectOpInterface>() else {
        return false;
    };

    if !is_unconstrained_value_type(result.borrow().ty()) {
        return false;
    }

    interface.effects().any(|effect| {
        effect.effect() == &AdviceEffect::Read
            && effect.value().is_none_or(|value| value == result)
    })
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{SourceSpan, testing::Test};

    use super::*;
    use crate::HirOpBuilder;

    #[test]
    fn shift_treats_only_constrained_typed_operands_as_range_constrained() {
        let mut test = Test::new("shift", &[], &[]);
        let mut builder = test.function_builder();
        let lhs = builder.i32(7, SourceSpan::UNKNOWN);
        let rhs = builder.u32(1, SourceSpan::UNKNOWN);
        let result = builder.shl(lhs, rhs, SourceSpan::UNKNOWN).unwrap();
        let op = result.borrow().get_defining_op().unwrap();
        let op = op.borrow();

        assert_eq!(range_constrained_operand_indices(&op), [0, 1]);
    }

    #[test]
    fn operation_result_advice_read_effects_mark_result_producers() {
        let mut test = Test::new("advice_pop", &[], &[]);
        let mut builder = test.function_builder();
        let result = builder.advice_pop(SourceSpan::UNKNOWN).unwrap();
        let op = result.borrow().get_defining_op().unwrap();
        let op = op.borrow();

        assert!(operation_result_has_advice_read_effect(&op, result));
    }
}
