# Code generation

The [compilation pipeline](https://github.com/0xMiden/compiler/blob/main/midenc-compile/src/pipeline/mod.rs) dispatches
each target to its registered frontend. Frontends declare their own ordered
checkpoint routes. A checkpoint exposes an artifact to observers and can stop
the target when the requested compilation goal is reached. `--emit` selects
artifacts to write; `--stop-after` selects how far to compile.

For a HIR-producing frontend, the [shared backend](https://github.com/0xMiden/compiler/blob/main/midenc-compile/src/pipeline/backend.rs)
analyzes initial HIR, runs transformations, and lowers the transformed HIR to MASM.
The [assembly driver](https://github.com/0xMiden/compiler/blob/main/midenc-compile/src/pipeline/assembly.rs) then builds
the program or library package. MASM inputs enter at this assembly stage. Project dependencies can use different frontends from the
root target; their artifacts must not be mistaken for the root's requested output.

## HIR ownership and transformations

HIR operations and values belong to a `Context`. A live component reference is
only usable while its context remains alive. An observer that needs a textual
snapshot should render at the corresponding checkpoint. Retaining live HIR also
requires retaining the owning context.

Transformations mutate HIR. The backend has a separate entry point for already
transformed HIR so that callers do not rerun transformations when they continue
to MASM. Consumers of intermediate artifacts should observe one compilation,
rather than restarting it to obtain each representation.

## MASM lowering

[Lowering](https://github.com/0xMiden/compiler/blob/main/codegen/masm/src/lower/lowering.rs) translates supported HIR
operations using the [instruction emitter](https://github.com/0xMiden/compiler/tree/main/codegen/masm/src/emit).
The emitter tracks the operand stack and arranges values for each operation's
stack contract. Multi-element values, signed arithmetic, and byte-addressed
memory need explicit lowering because a VM field element is not a Wasm integer
or byte pointer. Shared assembly procedures live in
[the intrinsics directory](https://github.com/0xMiden/compiler/tree/main/codegen/masm/intrinsics).

When adding an operation, cover its HIR typing and effects, any interpreter
implementation, lowering, and execution behavior. For a Wasm instruction, compare
boundary values and traps against the Wasm reference executor in the integration
tests. A textual MASM snapshot alone cannot establish that the resulting stack or
trap behavior is correct.
