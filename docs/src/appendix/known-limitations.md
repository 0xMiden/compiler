# Known limitations

> [!TIP]
> See the [issue tracker](https://github.com/0xpolygonmiden/compiler/issues) for information
> on known bugs. This document focuses on missing/incomplete features, rather than bugs.

The compiler is still in its early stages of development, so there are various features that are
unimplemented, or only partially implemented, and the test suite is still limited in scope, so
we are still finding bugs on a regular basis. We are rapidly improving this situation, but it is
important to be aware of this when using the compiler.

The features discussed below are broken up into sections, to make them easier to navigate and
reference.

## Rust language support

### Floating point types

- Status: **Unsupported**
- Tracking Issue: N/A
- Release Milestone: N/A

In order to represent `Felt` "natively" in Rust, we were forced to piggy-back on the `f32` type,
which is propagated through to WebAssembly, and allows us to handle those values specially.

As a result, floating-point types in Rust are not supported at all. Any attempt to use them will
result in a compilation error. We considered this a fair design tradeoff, as floating point math
is unused/rare in the context in which Miden is used, in comparison to fixed-point or field
arithmetic. In addition, implementing floating-point operations in software on the Miden VM would
be extraordinarily expensive, which generally works against the purpose for using floats in the
first place.

At this point in time, we have no plans to support floats, but this may change if we are able to
find a better/more natural representation for `Felt` in WebAssembly.

### Function call indirection

- Status: **Unimplemented**
- Tracking Issue: [#32](https://github.com/0xPolygonMiden/compiler/issues/32)
- Release Milestone: [Beta 1](https://github.com/0xPolygonMiden/compiler/milestone/4)

This feature corresponds to `call_indirect` in WebAssembly, and is associated with Rust features
such as trait objects (which use indirection to call trait methods), and closures. Note that the
Rust compiler is able to erase the indirection associated with certain abstractions statically
in some cases, shown below. If Rust is unable to statically resolve all call targets, then `midenc`
will raise an error when it encounters any use of `call_indirect`.

> [!WARNING]
> The following examples rely on `rustc`/LLVM inlining enough code to be able to convert indirect
> calls to direct calls. This may require you to enable link-time optimization with `lto = "fat"`
> and compile all of the code in the crate together with `codegen-units = 1`, in order to maximize
> the amount of inlining that can occur. Even then, it may not be possible to remove some forms of
> indirection, in which case you will need to find another workaround.

#### Iterator lowered to loop

```rust
pub fn is_zeroed(bytes: &[u8; 32]) -> bool {
    // Rust is able to convert this to a loop, erasing the closure completely
    bytes.iter().copied().all(|b| b == 0)
}
```

#### Monomorphization + inlining

```rust
pub fn call<F, T>(fun: F) -> T
where
    F: Fn() -> T,
{
    fun()
}

#[inline(never)]
pub fn foo() -> bool { true }

fn main() {
    // Rust is able to inline the body of `call` after monomorphization, which results in
    // the call to `foo` being resolved statically.
    call(foo)
}
```

#### Inlined trait impl

```rust
pub trait Foo {
    fn is_foo(&self) -> bool;
}

impl Foo for u32 {
    #[inline(never)]
    fn is_foo(&self) -> bool { true }
}

fn has_foo(items: &[dyn Foo]) -> bool {
    items.iter().any(|item| item.is_foo())
}

fn main() -> u32 {
    // Rust inlines `has_foo`, converts the iterator chain to a loop, and is able to realize
    // that the `dyn Foo` items are actually `u32`, and resolves the call to `is_foo` to
    // `<u32 as Foo>::is_foo`.
    let foo: &dyn Foo = &u32::MAX as &dyn Foo;
    has_foo(&[foo]) as u32
}
```

### Miden SDK

- Status: **Incomplete**
- Tracking Issue: [#159](https://github.com/0xPolygonMiden/compiler/issues/159) and [#158](https://github.com/0xPolygonMiden/compiler/issues/158)
- Release Milestone: [Beta 1](https://github.com/0xPolygonMiden/compiler/milestone/4)

The Miden SDK for Rust, is a Rust crate that provides the implementation of native Miden types, as
well as bindings to the Miden standard library and transaction kernel APIs.

Currently, only a very limited subset of the API surface has had bindings implemented. This means
that there is a fair amount of native Miden functionality that is not yet available from Rust. We
will be expanding the SDK rapidly over the next few weeks and months, but for the time being, if
you encounter a missing API that you need, let us know, so we can ensure it is prioritized above
APIs which are lesser used.

### Rust/Miden FFI (foreign function interface) and interop

- Status: **Internal Use Only**
- Tracking Issue: [#304](https://github.com/0xPolygonMiden/compiler/issues/304)
- Release Milestone: TBD

While the compiler has functionality to link against native Miden Assembly libraries, binding
against procedures exported from those libraries from Rust can require glue code to be emitted
by the compiler in some cases, and the set of procedures for which this is done is currently
restricted to a hardcoded whitelist of known Miden procedures.

This affects any procedure which returns a type larger than `u32` (excluding `Felt`, which for
this purpose has the same size). For example, returing a Miden `Word` from a procedure, a common
return type, is not compatible with Rust's ABI - it will attempt to generate code which allocates
stack space in the caller, which it expects the callee to write to, inserting a new parameter at
the start of the parameter list, and expecting nothing to be returned by value. The compiler handles
situations like these using a set of ABI "transformation strategies", which lift/lower differences
between the Rust and Miden ABIs at call boundaries.

To expose the FFI machinery for use with any Miden procedure, we need type signatures for those
procedures at a minimum, and in some cases we may require details of the calling convention/ABI.
This metadata does not currently exist, but is on the roadmap for inclusion into Miden Assembly
and Miden packaging. Once present, we can open up the FFI for general use.

## Core Miden functionality

### Dynamic procedure invocation

- Status: **Unimplemented**
- Tracking Issue: [#32](https://github.com/0xPolygonMiden/compiler/issues/32)
- Release Milestone: [Beta 1](https://github.com/0xPolygonMiden/compiler/milestone/4)

This is a dependency of [Function Call Indirection](#function-call-indirection) described above,
and is the mechanism by which we can perform indirect calls in Miden. In order to implement support
for indirect calls in the Wasm frontend, we need underlying support for `dynexec`, which is not yet
implemented.

This feature adds support for lowering indirect calls to `dynexec` or `dyncall` instructions,
depending on the ABI of the callee. `dyncall` has an additional dependency on support for
[Cross-Context Procedure Invocation](#cross-context-procedure-invocation).

A known issue with this feature is that `dyn(exec|call)` consumes a word on the operand stack
for the hash of the callee being invoked, but this word _remains_ on the stack when entering the
callee, which has the effect of requiring procedures to have a different ABI depending on whether
they expect to be dynamically-invoked or not.

Our solution to that issue is to generate stubs which are used as the target of `dyn(exec|call)`,
the body of which drop the callee hash, fix up the operand stack as necessary, and then uses a
simple `exec` or `call` to invoke the "real" callee. We will emit a single stub for every function
which has its "address" taken, and use the hash of the stub in place of the actual callee hash.

### Cross-context procedure invocation

- Status: **Unimplemented**
- Tracking Issue: [#303](https://github.com/0xPolygonMiden/compiler/issues/303)
- Release Milestone: [Beta 2](https://github.com/0xPolygonMiden/compiler/milestone/5)

This is required in order to support representing Miden accounts and note scripts in Rust, and
compilation to Miden Assembly.

Currently, you can write code in Rust that is very close to how accounts and note scripts will
look like in the language, but it is not possible to actually implement either of those in Rust
today. The reasons for this are covered in depth in the tracking issue linked above, but to
briefly summarize, the primary issue has to do with the fact that Rust programs are compiled
for a "shared-everything" environment, i.e. you can pass references to memory from caller to
callee, write to caller memory from the callee, etc. In Miden however, contexts are "shared-nothing"
units of isolation, and thus cross-context operations, such as performing a `call` from a note script
to a method on an account, are not compatible with the usual calling conventions used by Rust and
LLVM.

The solution to this relies on compiling the Rust code for the `wasm32-wasip2` target, which emits
a new kind of WebAssembly module, known as a _component_. These components adhere to the rules of
the [WebAssembly Component Model](https://component-model.bytecodealliance.org/). Of primary
interest to us, is the fact that components in this model are "shared-nothing", and the ABI used to
communicate across component boundaries, is specially designed to enforce shared-nothing  semantics
on caller and callee. In addition to compiling for a specific Wasm target, we also rely on some
additional tooling for describing component interfaces, types, and to generate Rust bindings for
those descriptions, to ensure that calls across the boundary remain opaque, even to the linker,
which ensures that the assumptions of the caller and callee with regard to what address space they
operate in are preserved (i.e. a callee can never be inlined into the caller, and thus end up
executing in the caller's context rather than the expected callee context).

This is one of our top priorities, as it is critical to being able to use Rust to compile code for
the Miden rollup, but it is also the most complex feature on our roadmap, hence why it is scheduled
for our Beta 2 milestone, rather than Beta 1 (the next release), as it depends on multiple other
subfeatures being implemented first.

## Packaging

### Package format

- Status: **Experimental**
- Tracking Issue: [#121](https://github.com/0xPolygonMiden/compiler/issues/121)
- Release Milestone: [Beta 1](https://github.com/0xPolygonMiden/compiler/milestone/4)

This feature represents the ability to compile and distribute a single artifact that contains
the compiled MAST, and all required and optional metadata to make linking against, and executing
packages as convenient as a dynamic library or executable.

The compiler currently produces, by default, an experimental implementation of a package format
that meets the minimum requirements to support libraries and programs compiled from Rust:

- Name and semantic version information
- Content digest
- The compiled MAST and metadata about the procedures exported from it
- Read-only data segments and their hashes (if needed by the program, used to load data into the
advice provider when a program is loaded, and to write those segments into linear memory when the
program starts)
- Dependency information (optional, specifies what libraries were linked against during compilation)
- Debug information (optional)

However, this package format is not yet understood by the Miden VM itself. This means you cannot,
currently, compile a package and then run it using `miden run` directly. Instead, you can use
`midenc run` to load and run code from a package, as the compiler ships with the VM embedded for
use with the interactive debugger, and provides native support for packaging on top of it. You can
also use `midenc debug` to execute your program interactively in the debugger, depending on your
needs. See [Debugging Programs](../usage/debugger.md) for more information on how to use the
debugger, and `midenc help run` for more information on executing programs with the `midenc run`
command.

While it is possible to emit raw MAST from `midenc`, rather than the experimental package format,
the resulting artifact cannot be run without some fragile and error-prone manual setup, in order
to ensure that the advice provider is correctly initialized with any read-only data segments. For
now, it is recommended that you use the `midenc` tooling for testing programs, until the format
is stabilized.
