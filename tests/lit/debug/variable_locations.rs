//! Test that debug info tracks source locations in a loop
//!
//! RUN: midenc %s --entrypoint=variable_locations::entrypoint -Zprint-hir-source-locations --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
//!
//! Check that function has source location annotations
//! CHECK-LABEL: builtin.function{{.*}}@entrypoint
//! CHECK: loc({{.*}}variable_locations.rs:{{[0-9]+}}
#![no_std]
#![no_main]
#![allow(unused_unsafe)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::wasm32::unreachable() }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(n: u32) -> u32 {
    let mut sum = 0u32;
    let mut i = 0u32;
    while i <= n {
        sum = sum + i;
        i = i + 1;
    }
    sum
}
