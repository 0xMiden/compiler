//! Stored-procedure dispatch tests: an account component calling a sibling component's
//! procedure through a MAST root kept in one of its storage slots.
//!
//! The dispatcher component declares `StorageValue<StoredProcedure<fn(..) -> R>>` slots and calls
//! them with the generated `call` method, which lowers to a `dyncall` into a new context. The
//! dispatcher has no compile-time dependency on the target: the roots are written into its slots
//! off-chain, from the sibling package's manifest (Rust-compiled targets) or from the standards
//! library (MASM targets), exactly as a deployment would.

mod common;
mod masm_standard;
mod overhead;
mod rust_sibling;
mod signatures;
