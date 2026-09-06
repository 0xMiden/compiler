//! Integration tests that decode account metadata or note scripts using protocol types
//! (`miden-protocol`).
//!
//! These tests are separated from `midenc-integration-tests` because the compiler tier (`midenc`,
//! `cargo-miden`, and their test crates) must not depend on the protocol crates; this crate is
//! part of the protocol-linked test tier instead, alongside `midenc-integration-network-tests`.
#![deny(warnings)]

#[cfg(test)]
mod end_to_end;
#[cfg(test)]
mod sdk;
