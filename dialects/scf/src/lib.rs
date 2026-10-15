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
pub mod transforms;

use midenc_hir::{
    DialectInfo,
    derive::{Dialect, DialectRegistration},
};

pub use self::{builders::StructuredControlFlowOpBuilder, ops::*};

/// Rebuilding a structured control flow op with a subset of its iteration arguments or results.
pub mod rebuild {
    pub use crate::canonicalization::{
        IterArg, keep_map, rebuild_if, rebuild_index_switch, rebuild_while,
    };
}

#[derive(Debug, Dialect, DialectRegistration)]
pub struct ScfDialect {
    #[dialect(info)]
    info: DialectInfo,
}
