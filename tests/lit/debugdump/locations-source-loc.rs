//! Test that .debug_loc section shows DebugVar entries with source locations
//! from a real Rust project compiled with debug info.
//!
//! RUN: env RUSTFLAGS="-Cdebuginfo=2" midenc %s --release --debug full -o %t/out.masp
//! RUN: miden-objtool dump debug-info %t/out.masp --section locations | filecheck %s
//!
//! Variables tracked in Wasm locals are described once by their stable frame-pointer-relative
//! location; no volatile operand-stack snapshots are emitted for local reads/writes, since the
//! standing FMP location stays valid while the local is updated.
//!
//! CHECK: .debug_loc contents (DebugVar entries from MAST):
//! CHECK: Total DebugVar entries: 4
//! CHECK: Unique variable names: 3
//!
//! Check variable "arg0" - parameter from test_assertion function
//! CHECK: Variable: "arg0"
//! CHECK: 1 location entries:
//! CHECK: FMP-4 (param #1) @ {{.*}}locations-source-loc.rs
//!
//! Check variable "local3" - from panic handler
//! CHECK: Variable: "local3"
//! CHECK: 1 location entries:
//! CHECK: FMP-1
//!
//! Check variable "x" - parameter from entrypoint function
//! CHECK: Variable: "x"
//! CHECK: 2 location entries:
//! CHECK: FMP-4 (param #1) @ {{.*}}locations-source-loc.rs
//! CHECK: FMP-4 (param #1) @ {{.*}}locations-source-loc.rs

#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_assertion(x: u32) -> u32 {
    assert!(x > 100, "x should be greater than 100");

    x
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn entrypoint(x: u32) -> u32 {
    test_assertion(x)
}
