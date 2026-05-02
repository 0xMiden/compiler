//! This module defines a first-class dialect for tracking source-level debug information through
//! compiler transformations.
//!
//! Inspired by [Mojo's DebugInfo dialect], this dialect makes debug variable tracking a first-class
//! citizen of the IR, using SSA use-def chains to enforce correctness.
//!
//! ## Motivation
//!
//! Traditional approaches to debug info in MLIR-like compilers (e.g. Flang/FIR) treat debug
//! information as metadata or attributes — second-class citizens that transforms are free to
//! silently drop. The consequences:
//!
//! - Transforms can silently lose debug info with no verifier catching it
//! - No mechanism forces transform authors to update debug info
//! - Debug info quality degrades as the optimizer gets more aggressive
//!
//! ## Approach: SSA-Based Debug Info
//!
//! This dialect defines debug operations as real IR operations with SSA operands:
//!
//! - **`di.value`** — Records the current value of a source variable. Uses an SSA value operand,
//!   so deleting the value without updating debug uses is a hard error.
//!
//! - **`di.declare`** — Records the storage address of a source variable. Similarly uses an SSA
//!   operand for the address.
//!
//! - **`di.kill`** — Marks a variable as dead, giving the debugger precise lifetime boundaries
//!   instead of scope-based heuristics.
//!
//! ## Transform Hooks
//!
//! The [`transform`] module provides utilities that make it easy for transform authors to maintain
//! debug info:
//!
//! - **Simple replacements** are handled automatically via `replace_all_uses_with`
//! - **Complex transforms** use [`salvage_debug_info`](transform::salvage_debug_info) where the
//!   transform author only describes the *inverse* of their transformation
//! - **Value deletion** without a replacement emits `di.kill` automatically
//!
//! ## Design Pillars (as inherited from Mojo)
//!
//! 1. **SSA use-def chains** — debug values participate in standard use-def tracking
//! 2. **Expression trees** — `DIExpressionAttr` describes how to recover source values from
//!    transformed IR values (encode the inverse transformation)
//! 3. **Explicit lifetimes** — `debuginfo.kill` for precise variable death points
//!
//! For historical context, you may be interested in the slides from Mojo's debugging talk, where
//! they discuss its debug info dialect. [You can find that here](https://llvm.org/devmtg/2024-04/slides/TechnicalTalks/MojoDebugging.pdf).
pub mod attributes;
mod builders;
mod ops;
pub mod transform;

pub use self::{builders::DIBuilder, ops::*};
use crate::{
    DialectInfo,
    derive::{Dialect, DialectRegistration},
};

/// The DebugInfo dialect — first-class debug variable tracking.
///
/// This dialect provides operations for tracking source-level variables through
/// compiler transformations using SSA semantics. Unlike metadata-based approaches,
/// debug info here participates in standard use-def chains, making it impossible
/// for transforms to silently drop debug information.
#[derive(Debug, Dialect, DialectRegistration)]
#[dialect(name = "di")]
pub struct DebugInfoDialect {
    #[dialect(info)]
    info: DialectInfo,
}
