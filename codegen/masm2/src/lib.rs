#![feature(debug_closure_helpers)]
#![feature(assert_matches)]
#![feature(const_type_id)]
#![feature(array_chunks)]
#![feature(iter_array_chunks)]

extern crate alloc;

mod artifact;
mod emit;
mod emitter;
pub mod intrinsics;
mod linker;
mod lower;
mod opt;
mod stack;

pub mod masm {
    pub use miden_assembly::{
        ast::*, KernelLibrary, Library, LibraryNamespace, LibraryPath, SourceSpan, Span, Spanned,
    };
}

pub(crate) use self::lower::HirLowering;
pub use self::{
    artifact::MasmComponent,
    lower::ToMasmComponent,
    stack::{Constraint, Operand, OperandStack},
};

pub fn register_dialect_hooks(context: &midenc_hir2::Context) {
    use midenc_dialect_hir as hir;

    context.register_dialect_hook::<hir::HirDialect, _>(|info, _context| {
        info.register_operation_trait::<hir::Assert, dyn HirLowering>();
        info.register_operation_trait::<hir::Assertz, dyn HirLowering>();
        info.register_operation_trait::<hir::AssertEq, dyn HirLowering>();
        info.register_operation_trait::<hir::AssertEqImm, dyn HirLowering>();
        info.register_operation_trait::<hir::Unreachable, dyn HirLowering>();
        //info.register_operation_trait::<hir::Poison, dyn HirLowering>();
        info.register_operation_trait::<hir::Add, dyn HirLowering>();
        info.register_operation_trait::<hir::AddOverflowing, dyn HirLowering>();
        info.register_operation_trait::<hir::Sub, dyn HirLowering>();
        info.register_operation_trait::<hir::SubOverflowing, dyn HirLowering>();
        info.register_operation_trait::<hir::Mul, dyn HirLowering>();
        info.register_operation_trait::<hir::MulOverflowing, dyn HirLowering>();
        info.register_operation_trait::<hir::Exp, dyn HirLowering>();
        info.register_operation_trait::<hir::Div, dyn HirLowering>();
        info.register_operation_trait::<hir::Sdiv, dyn HirLowering>();
        info.register_operation_trait::<hir::Mod, dyn HirLowering>();
        info.register_operation_trait::<hir::Smod, dyn HirLowering>();
        info.register_operation_trait::<hir::Divmod, dyn HirLowering>();
        info.register_operation_trait::<hir::Sdivmod, dyn HirLowering>();
        info.register_operation_trait::<hir::And, dyn HirLowering>();
        info.register_operation_trait::<hir::Or, dyn HirLowering>();
        info.register_operation_trait::<hir::Xor, dyn HirLowering>();
        info.register_operation_trait::<hir::Band, dyn HirLowering>();
        info.register_operation_trait::<hir::Bor, dyn HirLowering>();
        info.register_operation_trait::<hir::Bxor, dyn HirLowering>();
        info.register_operation_trait::<hir::Shl, dyn HirLowering>();
        info.register_operation_trait::<hir::ShlImm, dyn HirLowering>();
        info.register_operation_trait::<hir::Shr, dyn HirLowering>();
        info.register_operation_trait::<hir::Ashr, dyn HirLowering>();
        info.register_operation_trait::<hir::Rotl, dyn HirLowering>();
        info.register_operation_trait::<hir::Rotr, dyn HirLowering>();
        info.register_operation_trait::<hir::Eq, dyn HirLowering>();
        info.register_operation_trait::<hir::Neq, dyn HirLowering>();
        info.register_operation_trait::<hir::Gt, dyn HirLowering>();
        info.register_operation_trait::<hir::Gte, dyn HirLowering>();
        info.register_operation_trait::<hir::Lt, dyn HirLowering>();
        info.register_operation_trait::<hir::Lte, dyn HirLowering>();
        info.register_operation_trait::<hir::Min, dyn HirLowering>();
        info.register_operation_trait::<hir::Max, dyn HirLowering>();
        info.register_operation_trait::<hir::PtrToInt, dyn HirLowering>();
        info.register_operation_trait::<hir::IntToPtr, dyn HirLowering>();
        info.register_operation_trait::<hir::Cast, dyn HirLowering>();
        info.register_operation_trait::<hir::Bitcast, dyn HirLowering>();
        info.register_operation_trait::<hir::Trunc, dyn HirLowering>();
        info.register_operation_trait::<hir::Zext, dyn HirLowering>();
        info.register_operation_trait::<hir::Sext, dyn HirLowering>();
        info.register_operation_trait::<hir::Constant, dyn HirLowering>();
        //info.register_operation_trait::<hir::ConstantBytes, dyn HirLowering>();
        info.register_operation_trait::<hir::Ret, dyn HirLowering>();
        info.register_operation_trait::<hir::RetImm, dyn HirLowering>();
        info.register_operation_trait::<hir::If, dyn HirLowering>();
        info.register_operation_trait::<hir::While, dyn HirLowering>();
        info.register_operation_trait::<hir::IndexSwitch, dyn HirLowering>();
        info.register_operation_trait::<hir::Condition, dyn HirLowering>();
        info.register_operation_trait::<hir::Yield, dyn HirLowering>();
        info.register_operation_trait::<hir::Exec, dyn HirLowering>();
        info.register_operation_trait::<hir::Store, dyn HirLowering>();
        info.register_operation_trait::<hir::Load, dyn HirLowering>();
        info.register_operation_trait::<hir::MemGrow, dyn HirLowering>();
        info.register_operation_trait::<hir::MemSize, dyn HirLowering>();
        info.register_operation_trait::<hir::MemSet, dyn HirLowering>();
        info.register_operation_trait::<hir::MemCpy, dyn HirLowering>();
        info.register_operation_trait::<hir::Select, dyn HirLowering>();
        info.register_operation_trait::<hir::Incr, dyn HirLowering>();
        info.register_operation_trait::<hir::Neg, dyn HirLowering>();
        info.register_operation_trait::<hir::Inv, dyn HirLowering>();
        info.register_operation_trait::<hir::Ilog2, dyn HirLowering>();
        info.register_operation_trait::<hir::Pow2, dyn HirLowering>();
        info.register_operation_trait::<hir::Not, dyn HirLowering>();
        info.register_operation_trait::<hir::Bnot, dyn HirLowering>();
        info.register_operation_trait::<hir::IsOdd, dyn HirLowering>();
        info.register_operation_trait::<hir::Popcnt, dyn HirLowering>();
        info.register_operation_trait::<hir::Clz, dyn HirLowering>();
        info.register_operation_trait::<hir::Ctz, dyn HirLowering>();
        info.register_operation_trait::<hir::Clo, dyn HirLowering>();
        info.register_operation_trait::<hir::Cto, dyn HirLowering>();
    });
}
