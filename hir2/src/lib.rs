#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(coerce_unsized)]
#![feature(unsize)]
#![feature(ptr_metadata)]
#![feature(layout_for_ptr)]
#![feature(slice_ptr_get)]
#![feature(specialization)]
#![feature(rustc_attrs)]
#![feature(debug_closure_helpers)]
#![allow(incomplete_features)]
#![allow(internal_features)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod attributes;
mod core;
pub mod demangle;
pub mod derive;
pub mod dialects;
pub mod formatter;

pub use self::{attributes::*, core::*};
