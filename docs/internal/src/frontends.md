# Frontends

The [frontend registry](https://github.com/0xMiden/compiler/blob/main/midenc-compile/src/pipeline/registry.rs) supports
Rust projects, Wasm, HIR, and Miden Assembly. A project target's root extension
selects its frontend. Source-file inputs are wrapped in a synthesized project;
project manifests carry explicit targets and dependencies.

## Rust

The Rust project frontend invokes Cargo to produce Wasm and then uses the Wasm
frontend. A standalone Rust source file uses the standalone compiler path.
Compiler flags, target configuration, and SDK dependencies must agree with the
compiler's Wasm conventions, especially the use of `f32` as a field-element
carrier. Use the SDK's target configuration when building source intended for
Miden rather than assuming arbitrary Rust-generated Wasm is compatible.

## WebAssembly

The [Wasm frontend](https://github.com/0xMiden/compiler/tree/main/frontend/wasm) validates and translates core modules
and components into HIR. Module translation handles core instructions and data;
component translation handles imports, exports, and canonical ABI adaptation.
The resulting HIR enters the shared transformation and MASM backend.

Supported Wasm is constrained by the Miden target: `f32` carries field elements,
and memory growth uses the compiler-managed heap model. See
[data layout](data_layout.md) for these semantics. The
[instruction translator](https://github.com/0xMiden/compiler/blob/main/frontend/wasm/src/code_translator/mod.rs) and
[component translator](https://github.com/0xMiden/compiler/blob/main/frontend/wasm/src/component/translator.rs) reject
operations outside their supported subset. Do not infer support for an instruction solely from wasmparser recognizing it;
translation, lowering, and execution must all implement its behavior.

## HIR and Miden Assembly

HIR inputs are parsed into a context and use the shared HIR backend. They are
useful for focused transformation and lowering tests without a Rust or Wasm
producer. MASM inputs bypass HIR transformations and enter assembly directly.
Their available checkpoints therefore differ from HIR-producing frontends;
request only artifacts declared by the selected frontend's route.
