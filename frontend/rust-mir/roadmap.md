# Roadmap

This file maps the future stages of this frontend beyond the current smoke test.

One principle holds through all stages: the translator consumes only `rustc_public` types. The driver in front of it can change; the translator carries over.

## 1. Full function bodies

Translate any single function without calls: control flow (branches, switches, loops), checked arithmetic, places with projections (fields, indexes, references), and aggregates.

## 2. Calls inside one crate

Translate direct calls between the functions of the input crate, including monomorphic instantiations of local generic functions.

## 3. `Felt` and the Miden SDK

Map `Felt` as a native type and connect the SDK intrinsics. This stage delivers the main promise of the MIR path: no lossy round-trip through WebAssembly types.

## 4. Pipeline integration

Register the frontend in the midenc pipeline as a `Frontend` implementation, hand HIR to the shared MASM backend, and define how the frontend coexists with the current Rust-via-WebAssembly route.

## 5. Execution proof

Compile a program through the full pipeline to MASM, run it on the VM, and compare the behavior with the WebAssembly path.

## 6. Whole-program translation

Read the MIR of dependency crates and discover every reachable function, including generic instantiations from other crates.

**Driver decision point.** Two options: keep the standalone driver and write our own reachability walk on stable MIR, or adopt a rustc codegen-backend dylib to use the compiler's own mono-item collector. Decide here, with a spike on real SDK code: if the reachability walk handles the SDK's generics cleanly, keep the in-process shape that fits the midenc pipeline; if it drowns in collector edge cases, adopt the backend. The translator does not change in either case, and the standalone driver stays as the test harness.

## 7. Honest data layout

Replace the borrowed `wasm32` data layout with a Miden target definition, as its own design conversation.
