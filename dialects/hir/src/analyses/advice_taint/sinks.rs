use alloc::vec::Vec;

use midenc_dialect_arith as arith;
use midenc_hir::{CallOpInterface, Operation, Symbol, Type, Value, dialects::builtin};

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

pub(super) fn is_constrained_external_parameter_type(ty: &Type) -> bool {
    matches!(ty, Type::U32 | Type::U16 | Type::U8 | Type::I1)
}

pub(super) fn is_unconstrained_external_result_type(ty: &Type) -> bool {
    matches!(ty, Type::Felt | Type::Array(_))
}

pub(super) fn is_u32_presuming_sink(op: &Operation) -> bool {
    is_u32_presuming_arith_op(op) || is_u32_to_u64_zext(op)
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
