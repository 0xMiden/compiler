# Miden SDK

The Miden SDK is a set of Rust crates that provide libraries for developing Miden programs in Rust. 

The SDK is composed of the following crates:

## Miden Standard Library

The `miden-stdlib-sys` crate provides low-level bindings for Miden standard library functionality, and re-exports the unified `Felt` type from the `miden-field` crate.

See [README](stdlib-sys/README.md)

## Miden SDK

The `miden-sdk` crate provides a library for developing account and note script code for the Miden rollup in Rust.

See [README](sdk/README.md)
