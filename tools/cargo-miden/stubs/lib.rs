#![no_std]

// The stub crate provides unreachable bodies for functions that are
// lowered by the frontend. It intentionally excludes base stubs now.
// Base stubs are compiled and linked by the `miden-base-sys` crate
// via its own build.rs to avoid double definitions.

mod intrinsics;
mod stdlib;

// Base stubs have moved to `miden-base-sys` build.rs
