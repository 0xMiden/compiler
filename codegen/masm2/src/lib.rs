#![feature(debug_closure_helpers)]
#![feature(assert_matches)]
#![feature(const_type_id)]
#![feature(array_chunks)]
#![feature(iter_array_chunks)]

extern crate alloc;

mod artifact;
mod emit;
mod emitter;
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
    lower::ToMasmComponent,
    stack::{Constraint, Operand, OperandStack},
};
