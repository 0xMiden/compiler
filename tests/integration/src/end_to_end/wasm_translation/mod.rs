//! Contains tests that interpret Wasm instructions, treating the result of the interpreter as
//! source of truth. The same instruction is then compiled into a program executable on Miden VM
//! and it is asserted that executing that program produces the same result/trap as the interpreter.

pub(super) mod i32;
pub(super) mod wasm_interpreter;
