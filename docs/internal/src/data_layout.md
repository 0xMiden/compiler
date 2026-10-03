# Data layout

Wasm uses byte addresses, while Miden Assembly addresses field elements.
HIR pointer types distinguish byte and element address spaces; byte addressing
is the default. Handwritten MASM that exchanges pointers or aggregates
with compiled code must use the compiler's target layout, not the host Rust
layout of `Felt` or `Word`.

## Addresses and integers

A Wasm pointer is a 32-bit byte address. Four target bytes occupy one VM memory
element; a VM word contains four elements and therefore spans 16 target bytes.
For a byte address `p`, the containing element is `p / 4`, and the byte offset
inside it is `p % 4`. Byte extraction and insertion use little-endian bit offsets.
For example, byte address 6 selects bits 16–23 of element 1.

An integer wider than 32 bits occupies consecutive 32-bit limbs, with the least
significant limb at the lowest address. An `i64` or `u64` occupies two limbs;
signed values use the same two's-complement bit pattern as Wasm. Loads and stores
of smaller integers extract or update the corresponding bytes without replacing
adjacent bytes. Unaligned accesses may span multiple elements. An alignment
assertion in a generated access is a promise that callers must satisfy. The Wasm
frontend enforces alignment hints as promises, a deliberate target-specific
deviation from Wasm semantics. Aligned element-sized accesses can use element
pointers directly; see [address preparation](https://github.com/0xMiden/compiler/blob/main/dialects/wasm/src/mem.rs).

[Memory lowering](https://github.com/0xMiden/compiler/blob/main/codegen/masm/src/emit/mem.rs) implements these accesses;
[the memory intrinsics](https://github.com/0xMiden/compiler/blob/main/codegen/masm/intrinsics/mem.masm) implement their
shared address and heap operations. Aggregate offsets and padding come from HIR
`Type` layout (`midenc-hir-type`), rather than the host compiler's `size_of`.

## Field elements

The Wasm frontend maps `f32` to HIR `Felt`. This is the SDK's carrier for a Miden
field element through Wasm; it does not give compiled code IEEE-754 floating-point
semantics. A field element occupies one target memory element. Its value can
exceed 32 bits even though that element spans four target byte addresses.
Accesses that must preserve full field values need field-aware operations;
reinterpreting an element as four ordinary integer bytes cannot preserve every
field value. See [Wasm type translation](https://github.com/0xMiden/compiler/blob/main/frontend/wasm/src/module/types.rs).

A field element must not pass through a 64-bit integer *operation*: `shl`, `or`,
`shr` and the like work on 32-bit limbs with `u32` instructions, which trap on a
field value above `u32::MAX`. (A 64-bit *copy* is safe: an aligned `i64.load` or
`i64.store` moves its two elements without interpreting them.) LLVM can produce
such an operation without the source asking for one: its IR-level passes may
carry two adjacent field elements as one `i64`, assembled with
`i64.extend_i32_u`, `i64.shl` and `i64.or` and taken apart with `i64.shr_u` and
`i32.wrap_i64`.

This hazard is narrowed, not closed. [MASM legalization](https://github.com/0xMiden/compiler/blob/main/codegen/masm/src/legalization.rs)
splits a 64-bit store whose value is assembled that way, and a 64-bit load used
only for its two halves, into 32-bit accesses — but only where the assembled
value feeds the store directly. A pair that reaches its store through a join (a
block argument, an `i64` local) is still assembled with trapping operations.
The generated bindings avoid the shape by reading a felt-only result whole.
Separately, every Rust build for Miden turns LLVM's store merging off
(`-C llvm-args=-combiner-store-merging=false`); that removes 64-bit copies and
constant pairs, a size and cycle saving, and does not bear on this hazard.

## Heap model

The compiler's memory intrinsics manage a heap above a configured byte address.
Initialization records that base, sets the top to the base, and sets the logical
page count to zero. A page is 65,536 bytes. `memory.size` reports this logical heap
count; `memory.grow` adds pages and returns the old count, or `u32::MAX` on failure.
Failed growth preserves the metadata. This compiler-managed heap model is not a full
implementation of Wasm's declared linear-memory limits and initial page count.

Heap metadata begins at VM element address `0x40000000`, above the range reachable
by a 32-bit byte pointer. The maximum heap top is byte address `0xfffffffc`.
Procedure locals use a separate VM address range beginning at `2^31` elements.
Keep these units explicit when changing bounds: an element address and a byte
address with the same numeric value do not identify the same location.

## Component calls

Component interfaces add canonical ABI layout rules to the core Wasm types.
Records and arrays require aligned field offsets; variants require a discriminant
and an aligned payload. Flattening an interface value into call arguments is a
separate operation from laying it out in memory. Do not infer either layout from
a Rust struct's host representation.

[Canonical ABI loading and storing](https://github.com/0xMiden/compiler/blob/main/frontend/wasm/src/component/canon_abi_utils.rs)
uses component type information to reconstruct values across calls. It rejects
unsupported types, including lists in cross-context calls. Extend the paired
load/store paths and their round-trip tests together when adding a supported type.
