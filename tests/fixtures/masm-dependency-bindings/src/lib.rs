//! A Rust program that calls a Miden Assembly package of its own: the `masm-dep` path dependency
//! `miden-project.toml` declares. `build.rs` generates the package's bindings and links their
//! stubs; `tests/integration/src/end_to_end/masm_dependency_bindings.rs` runs `entrypoint`.

#![no_std]
#![feature(alloc_error_handler)]
// The generated bindings declare their procedures with `#[linkage = "extern_weak"]`.
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]

extern crate alloc;

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    core::arch::wasm32::unreachable()
}

mod masm_dep {
    // Generated names follow MASM's spelling, wrappers take one parameter per operand, and the
    // generated items carry the MASM path as their doc rather than prose.
    #![allow(
        non_camel_case_types,
        non_snake_case,
        clippy::too_many_arguments,
        missing_docs
    )]
    include!(concat!(env!("OUT_DIR"), "/masm-dep.rs"));
}

/// Returns `a + b + 2 * x`, computed by the `masm-dep` procedures `add_pair` and `double`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(a: miden::Felt, b: miden::Felt, x: u32) -> miden::Felt {
    let sum = masm_dep::add_pair(masm_dep::Pair { a, b });
    let doubled = masm_dep::double(x);
    sum + miden::Felt::from_u32(doubled)
}
