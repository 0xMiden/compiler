mod component;
mod fpi;
mod lowering;
mod native_ptr;
mod utils;

const DEBUG_VAR_KILL_SENTINEL: &[u8] = b"\0miden.debug.kill";

pub use self::{component::ToMasmComponent, lowering::HirLowering, native_ptr::NativePtr};
