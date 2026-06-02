//! Target-driven dialect conversion infrastructure.
//!
//! This module provides the generic pieces used to legalize HIR from one set of dialects to
//! another: conversion targets, conversion patterns, type conversion, signature helpers, and the
//! full-conversion driver. Concrete lowering pipelines own their target definitions and pattern
//! population, while this module owns the common legality and rewrite orchestration.
//!
//! The initial driver intentionally does not provide rollback. Conversion patterns must separate
//! matching from mutation: returning `Ok(false)` means no IR mutation occurred, returning
//! `Ok(true)` means the pattern rewrote the IR, and returning `Err` aborts conversion without
//! relying on the framework to undo partial changes.

mod diagnostics;
mod driver;
mod legalization_graph;
mod pattern;
mod pattern_set;
mod rewriter;
mod signature_conversion;
mod target;
mod type_converter;

pub use self::{
    diagnostics::*, driver::*, legalization_graph::*, pattern::*, pattern_set::*, rewriter::*,
    signature_conversion::*, target::*, type_converter::*,
};
