#![feature(debug_closure_helpers)]
#![feature(assert_matches)]
#![feature(iter_array_chunks)]
#![feature(iterator_try_collect)]
#![deny(warnings)]

extern crate alloc;

mod artifact;
mod data_segments;
mod emit;
mod emitter;
mod events;
pub mod intrinsics;
mod linker;
mod lower;
mod opt;
mod stack;

pub mod masm {
    pub use miden_assembly_syntax::{
        KernelLibrary, Library, LibraryNamespace, LibraryPath,
        ast::*,
        debuginfo::{SourceSpan, Span, Spanned},
        parser::{IntValue, PushValue},
    };
}

pub(crate) use self::lower::HirLowering;
pub use self::{
    artifact::{MasmComponent, Rodata},
    events::{TRACE_FRAME_END, TRACE_FRAME_START, TraceEvent},
    lower::{NativePtr, ToMasmComponent},
    stack::{Constraint, Operand, OperandStack},
};

pub fn register_dialect_hooks(context: &midenc_hir::Context) {
    use midenc_dialect_arith as arith;
    use midenc_dialect_cf as cf;
    use midenc_dialect_hir as hir;
    use midenc_dialect_scf as scf;
    use midenc_dialect_ub as ub;
    use midenc_hir::dialects::builtin;

    context.register_dialect_hook::<builtin::BuiltinDialect, _>(|info, _context| {
        info.register_operation_trait::<builtin::Ret, dyn HirLowering>();
        info.register_operation_trait::<builtin::RetImm, dyn HirLowering>();
        info.register_operation_trait::<builtin::GlobalSymbol, dyn HirLowering>();
    });
    context.register_dialect_hook::<arith::ArithDialect, _>(|info, _context| {
        info.register_operation_trait::<arith::Constant, dyn HirLowering>();
        info.register_operation_trait::<arith::Add, dyn HirLowering>();
        info.register_operation_trait::<arith::AddOverflowing, dyn HirLowering>();
        info.register_operation_trait::<arith::Sub, dyn HirLowering>();
        info.register_operation_trait::<arith::SubOverflowing, dyn HirLowering>();
        info.register_operation_trait::<arith::Mul, dyn HirLowering>();
        info.register_operation_trait::<arith::MulOverflowing, dyn HirLowering>();
        info.register_operation_trait::<arith::Exp, dyn HirLowering>();
        info.register_operation_trait::<arith::Div, dyn HirLowering>();
        info.register_operation_trait::<arith::Sdiv, dyn HirLowering>();
        info.register_operation_trait::<arith::Mod, dyn HirLowering>();
        info.register_operation_trait::<arith::Smod, dyn HirLowering>();
        info.register_operation_trait::<arith::Divmod, dyn HirLowering>();
        info.register_operation_trait::<arith::Sdivmod, dyn HirLowering>();
        info.register_operation_trait::<arith::And, dyn HirLowering>();
        info.register_operation_trait::<arith::Or, dyn HirLowering>();
        info.register_operation_trait::<arith::Xor, dyn HirLowering>();
        info.register_operation_trait::<arith::Band, dyn HirLowering>();
        info.register_operation_trait::<arith::Bor, dyn HirLowering>();
        info.register_operation_trait::<arith::Bxor, dyn HirLowering>();
        info.register_operation_trait::<arith::Shl, dyn HirLowering>();
        info.register_operation_trait::<arith::Shr, dyn HirLowering>();
        info.register_operation_trait::<arith::Ashr, dyn HirLowering>();
        info.register_operation_trait::<arith::Rotl, dyn HirLowering>();
        info.register_operation_trait::<arith::Rotr, dyn HirLowering>();
        info.register_operation_trait::<arith::Eq, dyn HirLowering>();
        info.register_operation_trait::<arith::Neq, dyn HirLowering>();
        info.register_operation_trait::<arith::Gt, dyn HirLowering>();
        info.register_operation_trait::<arith::Gte, dyn HirLowering>();
        info.register_operation_trait::<arith::Lt, dyn HirLowering>();
        info.register_operation_trait::<arith::Lte, dyn HirLowering>();
        info.register_operation_trait::<arith::Min, dyn HirLowering>();
        info.register_operation_trait::<arith::Max, dyn HirLowering>();
        info.register_operation_trait::<arith::Trunc, dyn HirLowering>();
        info.register_operation_trait::<arith::Zext, dyn HirLowering>();
        info.register_operation_trait::<arith::Sext, dyn HirLowering>();
        info.register_operation_trait::<arith::Incr, dyn HirLowering>();
        info.register_operation_trait::<arith::Neg, dyn HirLowering>();
        info.register_operation_trait::<arith::Inv, dyn HirLowering>();
        info.register_operation_trait::<arith::Ilog2, dyn HirLowering>();
        info.register_operation_trait::<arith::Pow2, dyn HirLowering>();
        info.register_operation_trait::<arith::Not, dyn HirLowering>();
        info.register_operation_trait::<arith::Bnot, dyn HirLowering>();
        info.register_operation_trait::<arith::IsOdd, dyn HirLowering>();
        info.register_operation_trait::<arith::Popcnt, dyn HirLowering>();
        info.register_operation_trait::<arith::Clz, dyn HirLowering>();
        info.register_operation_trait::<arith::Ctz, dyn HirLowering>();
        info.register_operation_trait::<arith::Clo, dyn HirLowering>();
        info.register_operation_trait::<arith::Cto, dyn HirLowering>();
        info.register_operation_trait::<arith::Join, dyn HirLowering>();
        info.register_operation_trait::<arith::Split, dyn HirLowering>();
    });
    context.register_dialect_hook::<cf::ControlFlowDialect, _>(|info, _context| {
        info.register_operation_trait::<cf::Select, dyn HirLowering>();
        info.register_operation_trait::<cf::CondBr, dyn HirLowering>();
    });
    context.register_dialect_hook::<scf::ScfDialect, _>(|info, _context| {
        info.register_operation_trait::<scf::If, dyn HirLowering>();
        info.register_operation_trait::<scf::While, dyn HirLowering>();
        info.register_operation_trait::<scf::IndexSwitch, dyn HirLowering>();
        info.register_operation_trait::<scf::Condition, dyn HirLowering>();
        info.register_operation_trait::<scf::Yield, dyn HirLowering>();
    });
    context.register_dialect_hook::<ub::UndefinedBehaviorDialect, _>(|info, _context| {
        info.register_operation_trait::<ub::Unreachable, dyn HirLowering>();
        info.register_operation_trait::<ub::Poison, dyn HirLowering>();
    });
    context.register_dialect_hook::<hir::HirDialect, _>(|info, _context| {
        info.register_operation_trait::<hir::Assert, dyn HirLowering>();
        info.register_operation_trait::<hir::Assertz, dyn HirLowering>();
        info.register_operation_trait::<hir::AssertEq, dyn HirLowering>();
        info.register_operation_trait::<hir::PtrToInt, dyn HirLowering>();
        info.register_operation_trait::<hir::IntToPtr, dyn HirLowering>();
        info.register_operation_trait::<hir::Cast, dyn HirLowering>();
        info.register_operation_trait::<hir::Bitcast, dyn HirLowering>();
        //info.register_operation_trait::<hir::ConstantBytes, dyn HirLowering>();
        info.register_operation_trait::<hir::Exec, dyn HirLowering>();
        info.register_operation_trait::<hir::Call, dyn HirLowering>();
        info.register_operation_trait::<hir::Store, dyn HirLowering>();
        info.register_operation_trait::<hir::StoreLocal, dyn HirLowering>();
        info.register_operation_trait::<hir::Load, dyn HirLowering>();
        info.register_operation_trait::<hir::LoadLocal, dyn HirLowering>();
        info.register_operation_trait::<hir::MemGrow, dyn HirLowering>();
        info.register_operation_trait::<hir::MemSize, dyn HirLowering>();
        info.register_operation_trait::<hir::MemSet, dyn HirLowering>();
        info.register_operation_trait::<hir::MemCpy, dyn HirLowering>();
        info.register_operation_trait::<hir::Breakpoint, dyn HirLowering>();
    });
}
