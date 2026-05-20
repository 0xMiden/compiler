//! RUN: midenc -Zlint -Canalyze-only -Zcargo-frontmatter %s 2>&1 | filecheck %s
//!
//! ```cargo
//! [dependencies]
//! miden-sdk-alloc = { path = "../../../sdk/alloc" }
//! miden-stdlib-sys = { path = "../../../sdk/stdlib-sys" }
//! ```
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![allow(unused_imports)]

extern crate alloc;
extern crate miden_stdlib_sys;

use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, intrinsics, pipe_words_to_memory};

#[global_allocator]
static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

#[panic_handler]
fn unreachable_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn unreachable_alloc_error(_info: core::alloc::Layout) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn entrypoint() -> Vec<Felt> {
    // CHECK: unconstrained external call result reaches operation requiring a constrained value
    // CHECK-LABEL: let advice = intrinsics::advice::adv_push_mapvaln(Word::default());
    let advice = intrinsics::advice::adv_push_mapvaln(Word::default());
    let (_, out) = pipe_words_to_memory(advice);
    // CHECK: unconstrained value is passed as a call argument here
    // CHECK-NEXT: unconstrained advice from an external call is consumed here as a constrained value
    // CHECK: add an explicit range check
    out
}
