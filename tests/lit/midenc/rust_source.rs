//! RUN: midenc -Zlint -Canalyze-only -Zcargo-frontmatter %s 2>&1 | filecheck %s
//! CHECK: unconstrained external call result reaches u32-presuming operation
//! CHECK: pipe_words_to_memory
//! CHECK: unconstrained value is passed as a call argument here
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
    let advice = intrinsics::advice::adv_push_mapvaln(Word::default());
    let (_, out) = pipe_words_to_memory(advice);
    out
}
