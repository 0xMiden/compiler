//! Test that basic debug info source locations are emitted for a simple function
//!
//! RUN: env RUSTFLAGS="-Copt-level=0 -Cdebuginfo=2" midenc %s --entrypoint=simple_debug::add -Zprint-hir-source-locations --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
//!
//! Check that function has source location annotations
//! CHECK-LABEL: builtin.function{{.*}}@add
//! CHECK: loc({{.*}}simple_debug.rs:{{[0-9]+}}
#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::wasm32::unreachable() }
}

#[unsafe(no_mangle)]
pub extern "C" fn add(a: u32, b: u32) -> u32 {
    a + b
}
