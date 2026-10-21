//! Lower typed HIR to Miden Assembly.
//!
//! The physical-plan pipeline makes stack allocation explicit: selection describes each operation's
//! inputs and effects, allocation chooses stack and spill locations, and expansion emits instructions
//! against those bindings. An independent verifier checks the resulting instructions before they can
//! become an assembly procedure. The existing component emitter remains the production entry point
//! while these stages are integrated.
//!
//! Start with `plan` for the shared vocabulary, then `select`, `allocate`, `lower::expand`, and `verify`
//! for the successive stages. Instruction algorithms live in `emit`; they describe operation semantics
//! rather than choosing where live SSA values reside.

#![feature(debug_closure_helpers)]
#![cfg_attr(test, feature(fn_traits, unboxed_closures))]
#![feature(iter_array_chunks)]
#![deny(warnings)]

extern crate alloc;

// The physical-plan pipeline is exercised directly before replacing the component emitter.
// Remove this allowance when the production entry point uses this module.
#[allow(dead_code)]
mod allocate;
mod artifact;
mod data_segments;
mod emit;
mod emitter;
mod events;
pub mod intrinsics;
mod legalization;
mod linker;
mod lower;
mod opt;
// The component emitter does not yet consume physical plans; direct pipeline tests do.
// Remove this allowance when that production entry point is switched over.
#[allow(dead_code)]
mod plan;
// The physical-plan pipeline is exercised directly before replacing the component emitter.
// Remove this allowance when the production entry point uses this module.
#[allow(dead_code)]
mod select;
mod stack;
// The physical-plan pipeline is exercised directly before replacing the component emitter.
// Remove this allowance when the production entry point uses this module.
#[allow(dead_code)]
mod verify;

pub mod masm {
    pub use miden_assembly_syntax::{
        Path as LibraryPathRef, PathBuf as LibraryPath, PathComponent as LibraryPathComponent,
        ast::*,
        debuginfo::{SourceSpan, Span, Spanned},
        parser::{IntValue, PushValue},
    };
}

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    dialects::{builtin, debuginfo},
    inventory,
};

pub(crate) use self::lower::{HirLowering, HirSelection};
pub use self::{
    artifact::{MasmComponent, Rodata},
    events::{Event, FRAME_END_EVENT, FRAME_START_EVENT, PRINT_LN_EVENT},
    legalization::{
        CheckMasmLegality, LegalizeForMasm, masm_legalization_target,
        populate_masm_legalization_target,
    },
    lower::{NativePtr, ToMasmComponent},
    stack::{Constraint, Operand, OperandStack},
};

inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<builtin::BuiltinDialect>(
    lower_builtin_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<arith::ArithDialect>(
    lower_arith_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<cf::ControlFlowDialect>(
    lower_cf_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<scf::ScfDialect>(
    lower_scf_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<ub::UndefinedBehaviorDialect>(
    lower_ub_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<hir::HirDialect>(
    lower_hir_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<wasm::WasmDialect>(
    lower_wasm_ops
));
inventory::submit!(::midenc_hir::DialectRegistrationHookInfo::new::<debuginfo::DebugInfoDialect>(
    lower_debuginfo_ops
));

fn lower_builtin_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<builtin::Ret, dyn HirSelection>();
    info.register_operation_trait::<builtin::RetImm, dyn HirSelection>();

    info.register_operation_trait::<builtin::Ret, dyn HirLowering>();
    info.register_operation_trait::<builtin::RetImm, dyn HirLowering>();
    info.register_operation_trait::<builtin::GlobalSymbol, dyn HirSelection>();
    info.register_operation_trait::<builtin::GlobalSymbol, dyn HirLowering>();
    info.register_operation_trait::<builtin::UnrealizedConversionCast, dyn HirLowering>();
}

fn lower_arith_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<arith::Constant, dyn HirSelection>();
    info.register_operation_trait::<arith::Split, dyn HirSelection>();
    info.register_operation_trait::<arith::Join, dyn HirSelection>();

    info.register_operation_trait::<arith::Constant, dyn HirLowering>();
    info.register_operation_trait::<arith::Add, dyn HirSelection>();
    info.register_operation_trait::<arith::Add, dyn HirLowering>();
    info.register_operation_trait::<arith::AddOverflowing, dyn HirSelection>();
    info.register_operation_trait::<arith::AddOverflowing, dyn HirLowering>();
    info.register_operation_trait::<arith::Sub, dyn HirSelection>();
    info.register_operation_trait::<arith::Sub, dyn HirLowering>();
    info.register_operation_trait::<arith::SubOverflowing, dyn HirSelection>();
    info.register_operation_trait::<arith::SubOverflowing, dyn HirLowering>();
    info.register_operation_trait::<arith::Mul, dyn HirSelection>();
    info.register_operation_trait::<arith::Mul, dyn HirLowering>();
    info.register_operation_trait::<arith::MulOverflowing, dyn HirSelection>();
    info.register_operation_trait::<arith::MulOverflowing, dyn HirLowering>();
    info.register_operation_trait::<arith::Exp, dyn HirSelection>();
    info.register_operation_trait::<arith::Exp, dyn HirLowering>();
    info.register_operation_trait::<arith::Div, dyn HirSelection>();
    info.register_operation_trait::<arith::Div, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Add, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Add, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Sub, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Sub, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Mul, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Mul, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Div, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Div, dyn HirLowering>();
    info.register_operation_trait::<arith::Sdiv, dyn HirLowering>();
    info.register_operation_trait::<arith::Mod, dyn HirSelection>();
    info.register_operation_trait::<arith::Mod, dyn HirLowering>();
    info.register_operation_trait::<arith::Smod, dyn HirLowering>();
    info.register_operation_trait::<arith::Divmod, dyn HirSelection>();
    info.register_operation_trait::<arith::Divmod, dyn HirLowering>();
    info.register_operation_trait::<arith::Sdivmod, dyn HirLowering>();
    info.register_operation_trait::<arith::And, dyn HirSelection>();
    info.register_operation_trait::<arith::And, dyn HirLowering>();
    info.register_operation_trait::<arith::Or, dyn HirSelection>();
    info.register_operation_trait::<arith::Or, dyn HirLowering>();
    info.register_operation_trait::<arith::Xor, dyn HirSelection>();
    info.register_operation_trait::<arith::Xor, dyn HirLowering>();
    info.register_operation_trait::<arith::Band, dyn HirSelection>();
    info.register_operation_trait::<arith::Band, dyn HirLowering>();
    info.register_operation_trait::<arith::Bor, dyn HirSelection>();
    info.register_operation_trait::<arith::Bor, dyn HirLowering>();
    info.register_operation_trait::<arith::Bxor, dyn HirSelection>();
    info.register_operation_trait::<arith::Bxor, dyn HirLowering>();
    info.register_operation_trait::<arith::Shl, dyn HirSelection>();
    info.register_operation_trait::<arith::Shl, dyn HirLowering>();
    info.register_operation_trait::<arith::Shr, dyn HirSelection>();
    info.register_operation_trait::<arith::Shr, dyn HirLowering>();
    info.register_operation_trait::<arith::Ashr, dyn HirLowering>();
    info.register_operation_trait::<arith::Rotl, dyn HirSelection>();
    info.register_operation_trait::<arith::Rotl, dyn HirLowering>();
    info.register_operation_trait::<arith::Rotr, dyn HirSelection>();
    info.register_operation_trait::<arith::Rotr, dyn HirLowering>();
    info.register_operation_trait::<arith::Eq, dyn HirSelection>();
    info.register_operation_trait::<arith::Eq, dyn HirLowering>();
    info.register_operation_trait::<arith::Neq, dyn HirSelection>();
    info.register_operation_trait::<arith::Neq, dyn HirLowering>();
    info.register_operation_trait::<arith::Gt, dyn HirSelection>();
    info.register_operation_trait::<arith::Gt, dyn HirLowering>();
    info.register_operation_trait::<arith::Gte, dyn HirSelection>();
    info.register_operation_trait::<arith::Gte, dyn HirLowering>();
    info.register_operation_trait::<arith::Lt, dyn HirSelection>();
    info.register_operation_trait::<arith::Lt, dyn HirLowering>();
    info.register_operation_trait::<arith::Lte, dyn HirSelection>();
    info.register_operation_trait::<arith::Lte, dyn HirLowering>();
    info.register_operation_trait::<arith::Min, dyn HirSelection>();
    info.register_operation_trait::<arith::Min, dyn HirLowering>();
    info.register_operation_trait::<arith::Max, dyn HirSelection>();
    info.register_operation_trait::<arith::Max, dyn HirLowering>();
    info.register_operation_trait::<arith::Trunc, dyn HirSelection>();
    info.register_operation_trait::<arith::Trunc, dyn HirLowering>();
    info.register_operation_trait::<arith::Zext, dyn HirSelection>();
    info.register_operation_trait::<arith::Zext, dyn HirLowering>();
    info.register_operation_trait::<arith::Sext, dyn HirSelection>();
    info.register_operation_trait::<arith::Sext, dyn HirLowering>();
    info.register_operation_trait::<arith::Incr, dyn HirSelection>();
    info.register_operation_trait::<arith::Incr, dyn HirLowering>();
    info.register_operation_trait::<arith::Neg, dyn HirSelection>();
    info.register_operation_trait::<arith::Neg, dyn HirLowering>();
    info.register_operation_trait::<arith::Inv, dyn HirSelection>();
    info.register_operation_trait::<arith::Inv, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Neg, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Neg, dyn HirLowering>();
    info.register_operation_trait::<arith::Ext2Inv, dyn HirSelection>();
    info.register_operation_trait::<arith::Ext2Inv, dyn HirLowering>();
    info.register_operation_trait::<arith::Ilog2, dyn HirSelection>();
    info.register_operation_trait::<arith::Ilog2, dyn HirLowering>();
    info.register_operation_trait::<arith::Pow2, dyn HirSelection>();
    info.register_operation_trait::<arith::Pow2, dyn HirLowering>();
    info.register_operation_trait::<arith::Not, dyn HirSelection>();
    info.register_operation_trait::<arith::Not, dyn HirLowering>();
    info.register_operation_trait::<arith::Bnot, dyn HirSelection>();
    info.register_operation_trait::<arith::Bnot, dyn HirLowering>();
    info.register_operation_trait::<arith::IsOdd, dyn HirSelection>();
    info.register_operation_trait::<arith::IsOdd, dyn HirLowering>();
    info.register_operation_trait::<arith::Popcnt, dyn HirSelection>();
    info.register_operation_trait::<arith::Popcnt, dyn HirLowering>();
    info.register_operation_trait::<arith::Clz, dyn HirSelection>();
    info.register_operation_trait::<arith::Clz, dyn HirLowering>();
    info.register_operation_trait::<arith::Ctz, dyn HirSelection>();
    info.register_operation_trait::<arith::Ctz, dyn HirLowering>();
    info.register_operation_trait::<arith::Clo, dyn HirSelection>();
    info.register_operation_trait::<arith::Clo, dyn HirLowering>();
    info.register_operation_trait::<arith::Cto, dyn HirSelection>();
    info.register_operation_trait::<arith::Cto, dyn HirLowering>();
    info.register_operation_trait::<arith::Join, dyn HirLowering>();
    info.register_operation_trait::<arith::Split, dyn HirLowering>();
}

fn lower_cf_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<cf::CondBr, dyn HirSelection>();

    info.register_operation_trait::<cf::Select, dyn HirSelection>();
    info.register_operation_trait::<cf::Select, dyn HirLowering>();
    info.register_operation_trait::<cf::CondBr, dyn HirLowering>();
}

fn lower_scf_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<scf::If, dyn HirSelection>();
    info.register_operation_trait::<scf::IndexSwitch, dyn HirSelection>();
    info.register_operation_trait::<scf::While, dyn HirSelection>();
    info.register_operation_trait::<scf::Condition, dyn HirSelection>();
    info.register_operation_trait::<scf::Yield, dyn HirSelection>();

    info.register_operation_trait::<scf::If, dyn HirLowering>();
    info.register_operation_trait::<scf::While, dyn HirLowering>();
    info.register_operation_trait::<scf::IndexSwitch, dyn HirLowering>();
    info.register_operation_trait::<scf::Condition, dyn HirLowering>();
    info.register_operation_trait::<scf::Yield, dyn HirLowering>();
}

fn lower_ub_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<ub::Unreachable, dyn HirSelection>();

    info.register_operation_trait::<ub::Unreachable, dyn HirLowering>();
    info.register_operation_trait::<ub::Poison, dyn HirSelection>();
    info.register_operation_trait::<ub::Poison, dyn HirLowering>();
}

fn lower_hir_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<hir::PtrToInt, dyn HirSelection>();
    info.register_operation_trait::<hir::IntToPtr, dyn HirSelection>();
    info.register_operation_trait::<hir::Bitcast, dyn HirSelection>();
    info.register_operation_trait::<hir::Dyncall, dyn HirSelection>();
    info.register_operation_trait::<hir::ExecIndirect, dyn HirSelection>();

    info.register_operation_trait::<hir::Assert, dyn HirSelection>();
    info.register_operation_trait::<hir::Assert, dyn HirLowering>();
    info.register_operation_trait::<hir::Assertz, dyn HirSelection>();
    info.register_operation_trait::<hir::Assertz, dyn HirLowering>();
    info.register_operation_trait::<hir::AssertEq, dyn HirSelection>();
    info.register_operation_trait::<hir::AssertEq, dyn HirLowering>();
    info.register_operation_trait::<hir::AssertU32, dyn HirSelection>();
    info.register_operation_trait::<hir::AssertU32, dyn HirLowering>();
    info.register_operation_trait::<hir::Cast, dyn HirSelection>();
    info.register_operation_trait::<hir::Cast, dyn HirLowering>();
    //info.register_operation_trait::<hir::ConstantBytes, dyn HirLowering>();
    info.register_operation_trait::<hir::ConstantPointer, dyn HirSelection>();
    info.register_operation_trait::<hir::ConstantPointer, dyn HirLowering>();
    info.register_operation_trait::<hir::Exec, dyn HirLowering>();
    info.register_operation_trait::<hir::ExecIndirect, dyn HirLowering>();
    info.register_operation_trait::<hir::ProcedureRoot, dyn HirLowering>();
    info.register_operation_trait::<hir::ExecFpi, dyn HirLowering>();
    info.register_operation_trait::<hir::Call, dyn HirLowering>();
    info.register_operation_trait::<hir::Dyncall, dyn HirLowering>();
    info.register_operation_trait::<hir::Syscall, dyn HirLowering>();
    info.register_operation_trait::<hir::Hash, dyn HirSelection>();
    info.register_operation_trait::<hir::HMerge, dyn HirSelection>();
    info.register_operation_trait::<hir::MTreeGet, dyn HirSelection>();
    info.register_operation_trait::<hir::MTreeSet, dyn HirSelection>();
    info.register_operation_trait::<hir::MTreeMerge, dyn HirSelection>();
    info.register_operation_trait::<hir::MTreeVerify, dyn HirSelection>();

    info.register_operation_trait::<hir::HPerm, dyn HirSelection>();
    info.register_operation_trait::<hir::HornerBase, dyn HirSelection>();
    info.register_operation_trait::<hir::HornerExt, dyn HirSelection>();
    info.register_operation_trait::<hir::LogDeferred, dyn HirSelection>();
    info.register_operation_trait::<hir::FriExt2Fold4, dyn HirSelection>();
    info.register_operation_trait::<hir::MemStream, dyn HirSelection>();
    info.register_operation_trait::<hir::AdvicePipe, dyn HirSelection>();
    info.register_operation_trait::<hir::CryptoStream, dyn HirSelection>();
    info.register_operation_trait::<hir::EvalCircuit, dyn HirSelection>();
    info.register_operation_trait::<hir::SystemEvent, dyn HirSelection>();
    info.register_operation_trait::<hir::EmitEvent, dyn HirSelection>();
    info.register_operation_trait::<hir::EmitEventImm, dyn HirSelection>();
    info.register_operation_trait::<hir::AdvicePop, dyn HirSelection>();
    info.register_operation_trait::<hir::AdviceLoadWord, dyn HirSelection>();
    info.register_operation_trait::<hir::Caller, dyn HirSelection>();
    info.register_operation_trait::<hir::UnpackWord, dyn HirSelection>();
    info.register_operation_trait::<hir::Clk, dyn HirSelection>();
    info.register_operation_trait::<hir::Load, dyn HirSelection>();
    info.register_operation_trait::<hir::Store, dyn HirSelection>();
    info.register_operation_trait::<hir::LoadLocal, dyn HirSelection>();
    info.register_operation_trait::<hir::StoreLocal, dyn HirSelection>();
    info.register_operation_trait::<hir::LocalAddress, dyn HirSelection>();
    info.register_operation_trait::<hir::MemGrow, dyn HirSelection>();
    info.register_operation_trait::<hir::MemSize, dyn HirSelection>();
    info.register_operation_trait::<hir::MemSet, dyn HirSelection>();
    info.register_operation_trait::<hir::MemCpy, dyn HirSelection>();
    info.register_operation_trait::<hir::MemMove, dyn HirSelection>();
    info.register_operation_trait::<hir::Store, dyn HirLowering>();
    info.register_operation_trait::<hir::StoreLocal, dyn HirLowering>();
    info.register_operation_trait::<hir::Load, dyn HirLowering>();
    info.register_operation_trait::<hir::LoadLocal, dyn HirLowering>();
    info.register_operation_trait::<hir::LocalAddress, dyn HirLowering>();
    info.register_operation_trait::<hir::Caller, dyn HirLowering>();
    info.register_operation_trait::<hir::UnpackWord, dyn HirLowering>();
    info.register_operation_trait::<hir::Clk, dyn HirLowering>();
    info.register_operation_trait::<hir::AdvicePop, dyn HirLowering>();
    info.register_operation_trait::<hir::AdviceLoadWord, dyn HirLowering>();
    info.register_operation_trait::<hir::AdvicePipe, dyn HirLowering>();
    info.register_operation_trait::<hir::EmitEvent, dyn HirLowering>();
    info.register_operation_trait::<hir::EmitEventImm, dyn HirLowering>();
    info.register_operation_trait::<hir::SystemEvent, dyn HirLowering>();
    info.register_operation_trait::<hir::Hash, dyn HirLowering>();
    info.register_operation_trait::<hir::HMerge, dyn HirLowering>();
    info.register_operation_trait::<hir::HPerm, dyn HirLowering>();
    info.register_operation_trait::<hir::MTreeGet, dyn HirLowering>();
    info.register_operation_trait::<hir::MTreeSet, dyn HirLowering>();
    info.register_operation_trait::<hir::MTreeMerge, dyn HirLowering>();
    info.register_operation_trait::<hir::MTreeVerify, dyn HirLowering>();
    info.register_operation_trait::<hir::CryptoStream, dyn HirLowering>();
    info.register_operation_trait::<hir::FriExt2Fold4, dyn HirLowering>();
    info.register_operation_trait::<hir::HornerBase, dyn HirLowering>();
    info.register_operation_trait::<hir::HornerExt, dyn HirLowering>();
    info.register_operation_trait::<hir::EvalCircuit, dyn HirLowering>();
    info.register_operation_trait::<hir::LogDeferred, dyn HirLowering>();
    info.register_operation_trait::<hir::MemStream, dyn HirLowering>();
    info.register_operation_trait::<hir::MemGrow, dyn HirLowering>();
    info.register_operation_trait::<hir::MemSize, dyn HirLowering>();
    info.register_operation_trait::<hir::MemSet, dyn HirLowering>();
    info.register_operation_trait::<hir::MemCpy, dyn HirLowering>();
    info.register_operation_trait::<hir::MemMove, dyn HirLowering>();
    info.register_operation_trait::<hir::PrintLn, dyn HirLowering>();
    info.register_operation_trait::<hir::PrintLn, dyn HirSelection>();
}

fn lower_wasm_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<wasm::I32Load8S, dyn HirSelection>();
    info.register_operation_trait::<wasm::I32Load16S, dyn HirSelection>();
    info.register_operation_trait::<wasm::I64Load8S, dyn HirSelection>();
    info.register_operation_trait::<wasm::I64Load16S, dyn HirSelection>();
    info.register_operation_trait::<wasm::I64Load32S, dyn HirSelection>();

    info.register_operation_trait::<wasm::SignExtend, dyn HirSelection>();
    info.register_operation_trait::<wasm::SignExtend, dyn HirLowering>();
    info.register_operation_trait::<wasm::I32Load8S, dyn HirLowering>();
    info.register_operation_trait::<wasm::I32Load16S, dyn HirLowering>();
    info.register_operation_trait::<wasm::I64Load8S, dyn HirLowering>();
    info.register_operation_trait::<wasm::I64Load16S, dyn HirLowering>();
    info.register_operation_trait::<wasm::I64Load32S, dyn HirLowering>();
    info.register_operation_trait::<wasm::I32RemS, dyn HirSelection>();
    info.register_operation_trait::<wasm::I32RemS, dyn HirLowering>();
    info.register_operation_trait::<wasm::I64RemS, dyn HirSelection>();
    info.register_operation_trait::<wasm::I64RemS, dyn HirLowering>();
}

fn lower_debuginfo_ops(info: &mut midenc_hir::DialectInfo) {
    info.register_operation_trait::<debuginfo::DebugValue, dyn HirSelection>();
    info.register_operation_trait::<debuginfo::DebugDeclare, dyn HirSelection>();
    info.register_operation_trait::<debuginfo::DebugKill, dyn HirSelection>();

    info.register_operation_trait::<debuginfo::DebugDeclare, dyn HirLowering>();
    info.register_operation_trait::<debuginfo::DebugValue, dyn HirLowering>();
    info.register_operation_trait::<debuginfo::DebugKill, dyn HirLowering>();
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, vec::Vec};

    use midenc_hir::{Context, traits::TransparentCast};

    use super::*;

    /// A transparent cast is lowered by renaming its operand, and only so: no op of a dialect
    /// MASM codegen lowers is both a `TransparentCast` and a `HirLowering`, which `emit_inst`
    /// would otherwise have two ways to lower, and the three casts the frontends build as no-ops
    /// are transparent.
    #[test]
    fn transparent_casts_have_no_other_lowering() {
        let context = Rc::new(Context::default());
        let dialects = [
            context.get_or_register_dialect::<builtin::BuiltinDialect>(),
            context.get_or_register_dialect::<arith::ArithDialect>(),
            context.get_or_register_dialect::<cf::ControlFlowDialect>(),
            context.get_or_register_dialect::<scf::ScfDialect>(),
            context.get_or_register_dialect::<ub::UndefinedBehaviorDialect>(),
            context.get_or_register_dialect::<hir::HirDialect>(),
            context.get_or_register_dialect::<wasm::WasmDialect>(),
            context.get_or_register_dialect::<debuginfo::DebugInfoDialect>(),
        ];

        let mut transparent = Vec::new();
        for dialect in &dialects {
            for op in dialect.registered_ops() {
                if op.implements::<dyn TransparentCast>() {
                    assert!(
                        !op.implements::<dyn HirLowering>(),
                        "'{op}' is a transparent cast with a HirLowering"
                    );
                    transparent.push(op.to_string());
                }
            }
        }
        transparent.sort();
        assert_eq!(transparent, ["hir.bitcast", "hir.int_to_ptr", "hir.ptr_to_int"]);
    }
}
