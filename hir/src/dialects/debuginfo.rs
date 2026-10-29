//! This module defines a first-class dialect for tracking source-level debug information through
//! compiler transformations.
//!
//! Inspired by [Mojo's DebugInfo dialect], this dialect makes debug variable tracking a first-class
//! citizen of the IR, using SSA use-def chains and checked mutation contracts.
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
//! - **`di.debug_value`** — Records the current value of a source variable. Uses an SSA value operand,
//!   so deleting the value without handling its debug uses is diagnosed.
//!
//! - **`di.debug_declare`** — Records the storage location of a source variable using a debug
//!   expression.
//!
//! - **`di.debug_kill`** — Marks a variable as dead, giving the debugger precise lifetime boundaries
//!   instead of scope-based heuristics.
//!
//! ## Transform Hooks
//!
//! The [`transform`] module provides utilities that make it easy for transform authors to maintain
//! debug info:
//!
//! - **Equivalent replacements** use checked rewriter APIs with [`DebugInfoUpdate::Equivalent`]
//! - **Complex transforms** use [`salvage_debug_info`](transform::salvage_debug_info) where the
//!   transform author only describes the *inverse* of their transformation
//! - **Value deletion** without recovery explicitly invalidates the location with `di.debug_kill`
//!
//! Every pass is audited for unacknowledged record, SSA-producer, and local-storage mutations,
//! even when normal IR verification is disabled. Each rewrite declares a preservation, relocation,
//! equivalence, salvage, invalidation, or unreachable-code discard contract. These checks enforce
//! intentional handling, not the semantic correctness of a transform's equivalence claim.
//!
//! ## Design Pillars (as inherited from Mojo)
//!
//! 1. **SSA use-def chains** — debug values participate in standard use-def tracking
//! 2. **Expression trees** — `DIExpressionAttr` describes how to recover source values from
//!    transformed IR values (encode the inverse transformation)
//! 3. **Explicit lifetimes** — `di.debug_kill` for precise variable death points
//!
//! For historical context, you may be interested in the slides from Mojo's debugging talk, where
//! they discuss its debug info dialect. [You can find that here](https://llvm.org/devmtg/2024-04/slides/TechnicalTalks/MojoDebugging.pdf).
pub mod attributes;
mod builders;
pub mod mutation;
mod ops;
pub mod transform;

pub use self::{
    builders::DIBuilder,
    mutation::{DebugInfoMutation, DebugInfoUpdate, is_debug_record},
    ops::*,
};
use crate::{
    DialectInfo,
    derive::{Dialect, DialectRegistration},
};

/// The DebugInfo dialect — first-class debug variable tracking.
///
/// This dialect provides operations for tracking source-level variables through
/// compiler transformations using SSA semantics and pass-level mutation auditing.
#[derive(Debug, Dialect, DialectRegistration)]
#[dialect(name = "di")]
pub struct DebugInfoDialect {
    #[dialect(info)]
    info: DialectInfo,
}

/// Return the source variable described by a value, declaration, or lifetime-end record.
/// Unlike the audit's private snapshots, this accessor has no mutation or ownership state.
pub fn debug_record_variable(op: &crate::Operation) -> Option<attributes::Variable> {
    if let Some(record) = op.downcast_ref::<DebugValue>() {
        Some(record.variable().as_value().clone())
    } else if let Some(record) = op.downcast_ref::<DebugDeclare>() {
        Some(record.variable().as_value().clone())
    } else {
        op.downcast_ref::<DebugKill>()
            .map(|record| record.variable().as_value().clone())
    }
}
