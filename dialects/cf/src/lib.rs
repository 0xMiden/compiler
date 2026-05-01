#![no_std]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![deny(warnings)]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

mod builders;
mod canonicalization;
mod ops;

use midenc_hir::{
    DialectInfo,
    derive::{Dialect, DialectRegistration},
};

pub use self::{builders::ControlFlowOpBuilder, ops::*};

#[derive(Debug, Dialect, DialectRegistration)]
#[dialect(name = "cf")]
pub struct ControlFlowDialect {
    #[dialect(info)]
    info: DialectInfo,
}
