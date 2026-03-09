#![no_std]
#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![deny(warnings)]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

mod builders;
mod canonicalization;
mod ops;
pub mod transforms;

use midenc_hir::{
    DialectInfo,
    derive::{Dialect, DialectRegistration},
};

pub use self::{builders::StructuredControlFlowOpBuilder, ops::*};

#[derive(Debug, Dialect, DialectRegistration)]
pub struct ScfDialect {
    #[dialect(info)]
    info: DialectInfo,
}
