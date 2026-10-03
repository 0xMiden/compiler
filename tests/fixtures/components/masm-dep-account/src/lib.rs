//! An account component that binds a Miden Assembly package of its own (`masm-dep`, declared in
//! `miden-project.toml`), consumed as a *dependency* by `../masm-dep-note`. The point of the
//! pair: a Rust project built as another project's dependency goes through a different compiler
//! route than a root project, and its bindings must resolve on that route too
//! (`tests/integration/src/end_to_end/masm_dependency_bindings.rs`).

#![no_std]
#![feature(alloc_error_handler)]
// The generated bindings declare their procedures with `#[linkage = "extern_weak"]`.
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {
    loop {}
}

use bindings::exports::miden::masm_dep_account::*;
use miden::Felt;

miden::generate!();
bindings::export!(MyFoo);

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

struct MyFoo;

impl foo::Guest for MyFoo {
    /// `input + 42`, computed by the `masm-dep` procedures: `add_pair(input, double(21))`.
    fn process_felt(input: Felt) -> Felt {
        let doubled = masm_dep::double(21);
        masm_dep::add_pair(masm_dep::Pair {
            a: input,
            b: Felt::from_u32(doubled),
        })
    }
}
