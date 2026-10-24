mod component;
pub(crate) mod debug;
pub(crate) mod expand;
mod lowering;
mod native_ptr;
pub(crate) mod raw;
pub(crate) mod units;

pub(crate) use component::{EmissionOwners, finalize_generated};
pub(crate) use lowering::{HirSelection, invocation_target_from_symbol};

pub use self::{
    component::{EmissionNamespace, ToMasmComponent},
    native_ptr::NativePtr,
};
