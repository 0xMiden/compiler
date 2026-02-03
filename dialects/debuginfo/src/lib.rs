#![no_std]
#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![deny(warnings)]

//! # DebugInfo Dialect
//!
//! A first-class dialect for tracking source-level debug information through
//! compiler transformations. Inspired by [Mojo's DebugInfo dialect], this
//! dialect makes debug variable tracking a first-class citizen of the IR,
//! using SSA use-def chains to enforce correctness.
//!
//! ## Motivation
//!
//! Traditional approaches to debug info in MLIR-like compilers (e.g., Flang/FIR)
//! treat debug information as metadata or attributes — second-class citizens that
//! transforms are free to silently drop. The consequences:
//!
//! - Transforms can silently lose debug info with no verifier catching it
//! - No mechanism forces transform authors to update debug info
//! - Debug info quality degrades as the optimizer gets more aggressive
//!
//! ## Approach: SSA-Based Debug Info
//!
//! This dialect defines debug operations as real IR operations with SSA operands:
//!
//! - **`debuginfo.value`** — Records the current value of a source variable.
//!   Uses an SSA value operand, so deleting the value without updating debug
//!   uses is a hard error.
//!
//! - **`debuginfo.declare`** — Records the storage address of a source variable.
//!   Similarly uses an SSA operand for the address.
//!
//! - **`debuginfo.kill`** — Marks a variable as dead, giving the debugger precise
//!   lifetime boundaries instead of scope-based heuristics.
//!
//! ## Transform Hooks
//!
//! The [`transform`] module provides utilities that make it easy for transform
//! authors to maintain debug info:
//!
//! - **Simple replacements** are handled automatically via `replace_all_uses_with`
//! - **Complex transforms** use [`salvage_debug_info`](transform::salvage_debug_info)
//!   where the transform author only describes the *inverse* of their transformation
//! - **Value deletion** without a replacement emits `debuginfo.kill` automatically
//!
//! ## Design Pillars (from Mojo)
//!
//! 1. **SSA use-def chains** — debug values participate in standard use-def tracking
//! 2. **Expression trees** — `DIExpressionAttr` describes how to recover source values
//!    from transformed IR values (encode the inverse transformation)
//! 3. **Explicit lifetimes** — `debuginfo.kill` for precise variable death points
//!
//! [Mojo's DebugInfo dialect]: https://llvm.org/devmtg/2024-04/slides/TechnicalTalks/MojoDebugging.pdf

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

use alloc::boxed::Box;

mod builders;
mod ops;
pub mod transform;

use midenc_hir::{
    AttributeValue, Builder, Dialect, DialectInfo, DialectRegistration, OperationRef, SourceSpan,
    Type,
};

pub use self::{builders::DebugInfoOpBuilder, ops::*};

/// The DebugInfo dialect — first-class debug variable tracking.
///
/// This dialect provides operations for tracking source-level variables through
/// compiler transformations using SSA semantics. Unlike metadata-based approaches,
/// debug info here participates in standard use-def chains, making it impossible
/// for transforms to silently drop debug information.
#[derive(Debug)]
pub struct DebugInfoDialect {
    info: DialectInfo,
}

impl DebugInfoDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl DialectRegistration for DebugInfoDialect {
    const NAMESPACE: &'static str = "debuginfo";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::DebugValue>();
        info.register_operation::<ops::DebugDeclare>();
        info.register_operation::<ops::DebugKill>();
    }
}

impl Dialect for DebugInfoDialect {
    #[inline]
    fn info(&self) -> &DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        _builder: &mut dyn Builder,
        _attr: Box<dyn AttributeValue>,
        _ty: &Type,
        _span: SourceSpan,
    ) -> Option<OperationRef> {
        // Debug info operations don't produce values that can be constants
        None
    }
}
