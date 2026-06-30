//! Test that HIR includes source locations for function parameters
//!
//! RUN: env RUSTFLAGS="-Copt-level=0 -Cdebuginfo=2" midenc %s --entrypoint=function_metadata::multiply -Zprint-hir-source-locations --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
//!
//! Check that function has source location annotations
//! CHECK-LABEL: builtin.function{{.*}}@multiply
//! CHECK: loc({{.*}}function_metadata.rs:{{[0-9]+}}
#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::wasm32::unreachable() }
}

#[unsafe(no_mangle)]
pub extern "C" fn multiply(x: u32, y: u32) -> u32 {
    x * y
}
