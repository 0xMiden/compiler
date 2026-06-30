//! Differential testing: compile each case as both a host `cdylib` and a MASM
//! package, then compare outputs across random `(u32, u32)` inputs.
//!
//! Cases live in `cases/` and are bodies of a `#[unsafe(no_mangle)] pub
//! extern "C" fn entrypoint(u32, u32) -> u32`. The harness in [`harness`]
//! prepends `#![no_std]` + a panic handler, builds the case both ways, and
//! drives 16 random inputs through proptest. A divergence is a likely
//! compiler bug; the case file itself is the minimal reproducer (proptest
//! shrinking is disabled).

mod harness;
mod tests;
