mod component;
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

pub(crate) use lowering::HirSelection;

pub use self::{component::ToMasmComponent, lowering::HirLowering, native_ptr::NativePtr};
