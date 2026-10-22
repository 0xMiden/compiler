mod component;
#[allow(dead_code)]
pub(crate) mod debug;
// Expansion is tested independently until the component emitter consumes physical plans.
#[allow(dead_code)]
pub(crate) mod expand;
mod fpi;
mod lowering;
mod native_ptr;
pub(crate) mod raw;
#[allow(dead_code)]
pub(crate) mod units;
mod utils;

pub(crate) use lowering::{HirSelection, invocation_target_from_symbol};

pub use self::{component::ToMasmComponent, lowering::HirLowering, native_ptr::NativePtr};
