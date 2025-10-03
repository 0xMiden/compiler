# Compiling Rust To WebAssembly

This chapter will walk you through compiling a Rust crate to a WebAssembly (Wasm) module
in binary (i.e. `.wasm`) form. The Miden compiler has a frontend which can take such
modules and compile them on to Miden Assembly, which will be covered in the next chapter.

## Setup

First, let's set up a simple Rust project that contains an implementation of the Fibonacci
function (I know, it's overdone, but we're trying to keep things as simple as possible to
make it easier to show the results at each step, so bear with me):

Start by creating a new library crate:

    cargo new --lib wasm-fib && cd wasm-fib

To compile to WebAssembly, you must have the appropriate Rust toolchain installed, so let's add
a toolchain file to our project root so that `rustup` and `cargo` will know what we need, and use
them by default:

    cat <<EOF > rust-toolchain.toml
    [toolchain]
    channel = "stable"
    targets = ["wasm32-wasip1"]
    EOF

Next, edit the `Cargo.toml` file as follows:

```toml
[package]
name = "wasm-fib"
version = "0.1.0"
edition = "2021"

[lib]
# Build this crate as a self-contained, C-style dynamic library
# This is required to emit the proper Wasm module type
crate-type = ["cdylib"]

[dependencies]
# Use a tiny allocator in place of the default one, if we want
# to make use of types in the `alloc` crate, e.g. String. We
# don't need that now, but it's good information to have in hand.
#miden-sdk-alloc = "0.0.5"

# When we build for Wasm, we'll use the release profile
[profile.release]
# Explicitly disable panic infrastructure on Wasm, as
# there is no proper support for them anyway, and it
# ensures that panics do not pull in a bunch of standard
# library code unintentionally
panic = "abort"
# Enable debug information so that we get useful debugging output
debug = true
# Optimize the output for size
opt-level = "z"
```

Most of these things are done to keep the generated code size as small as possible. Miden is a target
where the conventional wisdom about performance should be treated very carefully: we're almost always
going to benefit from less code, even if conventionally that code would be less efficient, simply due
to the difference in proving time accumulated due to extra instructions. That said, there are no hard
and fast rules, but these defaults are good ones to start with.

> [!TIP]
> We reference a simple bump allocator provided by `miden-sdk-alloc` above, but any simple
> allocator will do. The trade offs made by these small allocators are not generally suitable for
> long-running, or allocation-heavy applications, as they "leak" memory (generally because they
> make little to no attempt to recover freed allocations), however they are very useful for
> one-shot programs that do minimal allocation, which is going to be the typical case for Miden
> programs.

Next, edit `src/lib.rs` as shown below:

```rust
// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::{string::String, vec::Vec};

// If we wanted to use the types mentioned above, it would also be
// a good idea to use the allocator we pulled in as a dependency
// in Cargo.toml, like so:
//#[global_allocator]
//static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

// Required for no-std crates
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Compiles to a trap instruction in WebAssembly
    core::arch::wasm32::unreachable()
}

// Marking the function no_mangle ensures that it is exported
// from the compiled binary as `fib`, otherwise it would have
// a mangled name that has no stable form.
//
// You can specify a different name from the library than the
// name in the source code using the `#[export_name = "foo"]`
// attribute, which will make the function callable as `foo`
// externally (in this example)
#[no_mangle]
pub fn fib(n: u32) -> u32 {
    let mut a = 0;
    let mut b = 1;
    for _ in 0..n {
        let c = a + b;
        a = b;
        b = c;
    }
    a
}
```

This exports our `fib` function from the library, making it callable from within a larger Miden program.

All that remains is to compile to WebAssembly:

    cargo build --release --target=wasm32-wasip1

This places a `wasm_fib.wasm` file under the `target/wasm32-wasip1/release/` directory, which
we can then examine with [wasm2wat](https://github.com/WebAssembly/wabt) to set the code we generated:

    wasm2wat target/wasm32-wasip1/release/wasm_fib.wasm

Which dumps the following output (may differ slightly on your machine, depending on the specific compiler version):

```wat
(module $wasm_fib.wasm
  (type (;0;) (func (param i32) (result i32)))
  (func $fib (type 0) (param i32) (result i32)
    (local i32 i32 i32)
    i32.const 0
    local.set 1
    i32.const 1
    local.set 2
    loop (result i32)  ;; label = @1
      local.get 2
      local.set 3
      block  ;; label = @2
        local.get 0
        br_if 0 (;@2;)
        local.get 1
        return
      end
      local.get 0
      i32.const -1
      i32.add
      local.set 0
      local.get 1
      local.get 3
      i32.add
      local.set 2
      local.get 3
      local.set 1
      br 0 (;@1;)
    end)
  (memory (;0;) 16)
  (global $__stack_pointer (mut i32) (i32.const 1048576))
  (export "memory" (memory 0))
  (export "fib" (func $fib)))
```

Success!

## Next steps

In [Compiling WebAssembly to Miden Assembly](wasm_to_masm.md), we walk through how to take the
WebAssembly module we just compiled, and lower it to Miden Assembly using `midenc`!
