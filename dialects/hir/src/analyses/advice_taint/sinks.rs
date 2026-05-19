use alloc::vec::Vec;

use midenc_dialect_arith as arith;
use midenc_hir::{
    CallOpInterface, Operation, Symbol, Type, Value,
    dialects::builtin,
    effects::{AdviceEffect, AdviceEffectOpInterface},
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

pub(super) fn is_constrained_external_parameter_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Enum(_)
            | Type::Ptr(_)
            | Type::I32
            | Type::U32
            | Type::I16
            | Type::U16
            | Type::I8
            | Type::U8
            | Type::I1
    )
}

pub(super) fn is_unconstrained_external_result_type(ty: &Type) -> bool {
    matches!(ty, Type::Felt | Type::Array(_))
}

pub(super) fn is_u32_presuming_sink(op: &Operation) -> bool {
    is_u32_presuming_arith_op(op) || is_u32_to_u64_zext(op)
}

pub(super) fn u32_presuming_operand_indices(op: &Operation) -> Vec<usize> {
    if !is_u32_presuming_sink(op) {
        return Vec::new();
    }

    op.operands()
        .iter()
        .enumerate()
        .filter_map(|(index, operand)| {
            let value = operand.borrow().as_value_ref();
            (value.borrow().ty() == &Type::U32).then_some(index)
        })
        .collect()
}

pub(super) fn operation_result_has_advice_read_effect(
    op: &Operation,
    result: midenc_hir::ValueRef,
) -> bool {
    let Some(interface) = op.as_trait::<dyn AdviceEffectOpInterface>() else {
        return false;
    };

    interface.effects().any(|effect| {
        effect.effect() == &AdviceEffect::Read
            && effect.value().is_some_and(|value| value == result)
    })
}

fn is_u32_presuming_arith_op(op: &Operation) -> bool {
    if !has_u32_operand(op) {
        return false;
    }

    op.is::<arith::Add>()
        || op.is::<arith::AddOverflowing>()
        || op.is::<arith::Sub>()
        || op.is::<arith::SubOverflowing>()
        || op.is::<arith::Mul>()
        || op.is::<arith::MulOverflowing>()
        || op.is::<arith::Div>()
        || op.is::<arith::Mod>()
        || op.is::<arith::Divmod>()
        || op.is::<arith::Band>()
        || op.is::<arith::Bor>()
        || op.is::<arith::Bxor>()
        || op.is::<arith::Shl>()
        || op.is::<arith::Shr>()
        || op.is::<arith::Rotl>()
        || op.is::<arith::Rotr>()
        || op.is::<arith::Eq>()
        || op.is::<arith::Neq>()
        || op.is::<arith::Gt>()
        || op.is::<arith::Gte>()
        || op.is::<arith::Lt>()
        || op.is::<arith::Lte>()
        || op.is::<arith::Min>()
        || op.is::<arith::Max>()
        || op.is::<arith::Bnot>()
        || op.is::<arith::Popcnt>()
        || op.is::<arith::Ctz>()
        || op.is::<arith::Clz>()
        || op.is::<arith::Clo>()
        || op.is::<arith::Cto>()
}

fn is_u32_to_u64_zext(op: &Operation) -> bool {
    // MASM widening/add3/madd u32 instructions lower by first refining operands to u32, then
    // zero-extending them to u64 for the widened arithmetic. The zext is the u32-consuming
    // boundary that remains visible after lifting.
    op.is::<arith::Zext>()
        && has_u32_operand(op)
        && op.results().all().iter().any(|result| result.borrow().ty() == &Type::U64)
}

fn has_u32_operand(op: &Operation) -> bool {
    op.operands().iter().any(|operand| {
        let value = operand.borrow().as_value_ref();
        value.borrow().ty() == &Type::U32
    })
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{SourceSpan, testing::Test};

    use super::*;
    use crate::HirOpBuilder;

    #[test]
    fn shift_only_treats_u32_typed_operands_as_u32_presuming() {
        let mut test = Test::new("shift", &[], &[]);
        let mut builder = test.function_builder();
        let lhs = builder.i32(7, SourceSpan::UNKNOWN);
        let rhs = builder.u32(1, SourceSpan::UNKNOWN);
        let result = builder.shl(lhs, rhs, SourceSpan::UNKNOWN).unwrap();
        let op = result.borrow().get_defining_op().unwrap();
        let op = op.borrow();

        assert_eq!(u32_presuming_operand_indices(&op), [1]);
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
