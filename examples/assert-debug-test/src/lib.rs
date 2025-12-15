//! Example for testing Rust assert! macro source location preservation.
//!
//! Build with:
//!   cargo build --release --target wasm32-unknown-unknown \
//!     --manifest-path examples/assert-debug-test/Cargo.toml
//!
//! Check HIR for source locations:
//!   ./bin/midenc examples/assert-debug-test/target/wasm32-unknown-unknown/release/assert_debug_test.wasm \
//!     --entrypoint=assert_debug_test::test_assert \
//!     -Ztrim-path-prefix=examples/assert-debug-test \
//!     -Zprint-hir-source-locations \
//!     --debug full --emit=hir=-
//!

#![no_std]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[no_mangle]
pub extern "C" fn test_assert(x: u32) -> u32 {
    assert!(x > 100);
    x
}

#[no_mangle]
pub extern "C" fn test_multiple_asserts(a: u32, b: u32) -> u32 {
    assert!(a > 0);

    assert!(b > 0);

    assert!(a != b);

    a + b
}
