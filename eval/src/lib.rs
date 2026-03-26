#![no_std]
#![feature(debug_closure_helpers)]
#![deny(warnings)]

#[cfg(any(feature = "std", test))]
extern crate std;

extern crate alloc;

mod eval;
mod evaluator;
#[cfg(test)]
mod tests;
mod value;

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{dialects::builtin, inventory};

pub use self::{
    eval::{ControlFlowEffect, Eval, Initialize},
    evaluator::HirEvaluator,
    value::Value,
};

inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<builtin::BuiltinDialect>(
    eval_builtin_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<arith::ArithDialect>(
    eval_arith_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<cf::ControlFlowDialect>(
    eval_cf_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<scf::ScfDialect>(
    eval_scf_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<ub::UndefinedBehaviorDialect>(
    eval_ub_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<hir::HirDialect>(
    eval_hir_dialect
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<wasm::WasmDialect>(
    eval_wasm_dialect
));

fn eval_builtin_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<builtin::Ret, dyn Eval>();
    info.register_operation_trait::<builtin::RetImm, dyn Eval>();
}

fn eval_arith_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<arith::Constant, dyn Eval>();
    info.register_operation_trait::<arith::Add, dyn Eval>();
    info.register_operation_trait::<arith::AddOverflowing, dyn Eval>();
    info.register_operation_trait::<arith::Sub, dyn Eval>();
    info.register_operation_trait::<arith::SubOverflowing, dyn Eval>();
    info.register_operation_trait::<arith::Mul, dyn Eval>();
    info.register_operation_trait::<arith::MulOverflowing, dyn Eval>();
    info.register_operation_trait::<arith::Exp, dyn Eval>();
    info.register_operation_trait::<arith::Div, dyn Eval>();
    info.register_operation_trait::<arith::Sdiv, dyn Eval>();
    info.register_operation_trait::<arith::Mod, dyn Eval>();
    info.register_operation_trait::<arith::Smod, dyn Eval>();
    info.register_operation_trait::<arith::Divmod, dyn Eval>();
    info.register_operation_trait::<arith::Sdivmod, dyn Eval>();
    info.register_operation_trait::<arith::And, dyn Eval>();
    info.register_operation_trait::<arith::Or, dyn Eval>();
    info.register_operation_trait::<arith::Xor, dyn Eval>();
    info.register_operation_trait::<arith::Band, dyn Eval>();
    info.register_operation_trait::<arith::Bor, dyn Eval>();
    info.register_operation_trait::<arith::Bxor, dyn Eval>();
    info.register_operation_trait::<arith::Shl, dyn Eval>();
    info.register_operation_trait::<arith::Shr, dyn Eval>();
    info.register_operation_trait::<arith::Ashr, dyn Eval>();
    info.register_operation_trait::<arith::Rotl, dyn Eval>();
    info.register_operation_trait::<arith::Rotr, dyn Eval>();
    info.register_operation_trait::<arith::Eq, dyn Eval>();
    info.register_operation_trait::<arith::Neq, dyn Eval>();
    info.register_operation_trait::<arith::Gt, dyn Eval>();
    info.register_operation_trait::<arith::Gte, dyn Eval>();
    info.register_operation_trait::<arith::Lt, dyn Eval>();
    info.register_operation_trait::<arith::Lte, dyn Eval>();
    info.register_operation_trait::<arith::Min, dyn Eval>();
    info.register_operation_trait::<arith::Max, dyn Eval>();
    info.register_operation_trait::<arith::Trunc, dyn Eval>();
    info.register_operation_trait::<arith::Zext, dyn Eval>();
    info.register_operation_trait::<arith::Sext, dyn Eval>();
    info.register_operation_trait::<arith::Incr, dyn Eval>();
    info.register_operation_trait::<arith::Neg, dyn Eval>();
    info.register_operation_trait::<arith::Inv, dyn Eval>();
    info.register_operation_trait::<arith::Ilog2, dyn Eval>();
    info.register_operation_trait::<arith::Pow2, dyn Eval>();
    info.register_operation_trait::<arith::Not, dyn Eval>();
    info.register_operation_trait::<arith::Bnot, dyn Eval>();
    info.register_operation_trait::<arith::IsOdd, dyn Eval>();
    info.register_operation_trait::<arith::Popcnt, dyn Eval>();
    info.register_operation_trait::<arith::Clz, dyn Eval>();
    info.register_operation_trait::<arith::Ctz, dyn Eval>();
    info.register_operation_trait::<arith::Clo, dyn Eval>();
    info.register_operation_trait::<arith::Cto, dyn Eval>();
}

fn eval_cf_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<cf::Select, dyn Eval>();
    info.register_operation_trait::<cf::Br, dyn Eval>();
    info.register_operation_trait::<cf::CondBr, dyn Eval>();
    info.register_operation_trait::<cf::Switch, dyn Eval>();
}

fn eval_scf_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<scf::If, dyn Eval>();
    info.register_operation_trait::<scf::While, dyn Eval>();
    info.register_operation_trait::<scf::IndexSwitch, dyn Eval>();
    info.register_operation_trait::<scf::Condition, dyn Eval>();
    info.register_operation_trait::<scf::Yield, dyn Eval>();
}

fn eval_ub_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<ub::Unreachable, dyn Eval>();
    info.register_operation_trait::<ub::Poison, dyn Eval>();
}

fn eval_hir_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<hir::Assert, dyn Eval>();
    info.register_operation_trait::<hir::Assertz, dyn Eval>();
    info.register_operation_trait::<hir::AssertEq, dyn Eval>();
    info.register_operation_trait::<hir::PtrToInt, dyn Eval>();
    info.register_operation_trait::<hir::IntToPtr, dyn Eval>();
    info.register_operation_trait::<hir::Cast, dyn Eval>();
    info.register_operation_trait::<hir::Bitcast, dyn Eval>();
    //info.register_operation_trait::<hir::ConstantBytes, dyn Eval>();
    info.register_operation_trait::<hir::Exec, dyn Eval>();
    info.register_operation_trait::<hir::Store, dyn Eval>();
    info.register_operation_trait::<hir::StoreLocal, dyn Eval>();
    info.register_operation_trait::<hir::Load, dyn Eval>();
    info.register_operation_trait::<hir::LoadLocal, dyn Eval>();
    info.register_operation_trait::<hir::MemGrow, dyn Eval>();
    info.register_operation_trait::<hir::MemSize, dyn Eval>();
    info.register_operation_trait::<hir::MemSet, dyn Eval>();
    info.register_operation_trait::<hir::MemCpy, dyn Eval>();
}

fn eval_wasm_dialect(info: &mut ::midenc_hir::DialectInfo) {
    info.register_operation_trait::<wasm::SignExtend, dyn Eval>();
    info.register_operation_trait::<wasm::I32Load8S, dyn Eval>();
    info.register_operation_trait::<wasm::I32Load16S, dyn Eval>();
    info.register_operation_trait::<wasm::I64Load8S, dyn Eval>();
    info.register_operation_trait::<wasm::I64Load16S, dyn Eval>();
    info.register_operation_trait::<wasm::I64Load32S, dyn Eval>();
}
