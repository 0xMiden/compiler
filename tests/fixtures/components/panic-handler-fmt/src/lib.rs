//! Fixture for formatting panic messages without allocation.

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::string::String;
use miden::debug::MidenDebug;

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    miden::debug::println("custom panic handler invoked");
    miden::debug::printing_panic_handler(info)
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    core::arch::wasm32::unreachable()
}

// Exercises debug formatting for a nested struct.
#[derive(Debug)]
struct Limits {
    threshold: u32,
}

// Exercises debug formatting for a variety of field types.
#[derive(MidenDebug)]
struct Input<'a> {
    value: u32,
    limits: Limits,
    above_threshold: bool,
    #[miden_debug(raw_str)]
    label: &'static str,
    #[miden_debug(raw_str)]
    owned_label: String,
    optional_value: Option<u32>,
    missing_value: Option<u32>,
    samples: [u32; 3],
    slice: &'a [u32],
}

/// Returns the input above the threshold and panics otherwise.
#[unsafe(no_mangle)]
pub fn entrypoint(x: u32) -> u32 {
    let slice_values = [x, 100];
    let input = Input {
        value: x,
        limits: Limits { threshold: 100 },
        above_threshold: x > 100,
        label: "input",
        owned_label: String::from("owned input"),
        optional_value: Some(x),
        missing_value: None,
        samples: [x, 0, 100],
        slice: &slice_values,
    };
    // Keep every field materialized without invoking string formatting.
    core::hint::black_box(&input);
    assert!(input.value > input.limits.threshold, "invalid input: {:?}", input);
    x
}

/// Panics with a formatted message exceeding the panic buffer capacity.
#[unsafe(no_mangle)]
pub fn overflow_entrypoint(x: u32) -> u32 {
    const LONG_MESSAGE: &str = concat!(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    assert!(x > 100, "input {}: {}", x, core::hint::black_box(LONG_MESSAGE));
    x
}
