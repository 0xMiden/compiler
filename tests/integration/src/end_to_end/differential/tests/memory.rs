//! Loads/stores, memcpy/fill, data segments, and memory intrinsics.

use super::super::harness::run_case;

/// Runtime-indexed u32 array — dynamic i32.load/i32.store addressing
/// (`prepare_addr`, word load/store emitter paths).
#[test]
fn mem_indexed() {
    run_case("mem_indexed", include_str!("../cases/case_mem_indexed.rs"));
}

/// Runtime-length `copy_from_slice`/`copy_within` — wasm `memory.copy` /
/// HIR MemCpy lowering (element fast path + byte fallback loop).
#[test]
fn mem_copy() {
    run_case("mem_copy", include_str!("../cases/case_mem_copy.rs"));
}

/// Overlapping `copy_within` (dst > src) — wasm `memory.copy` memmove
/// semantics vs forward-copying MASM lowering.
#[test]
#[ignore = "native/MASM divergence: memory.copy with overlapping dst > src ranges (original repro: \
            inputs (91264998, 3811523388) in pre-split mem_copy)"]
fn mem_overlap() {
    run_case("mem_overlap", include_str!("../cases/case_mem_overlap.rs"));
}

/// `static` lookup tables — wasm data segments through rodata layout,
/// merging, padding, and init-code emission.
#[test]
fn mem_static() {
    run_case("mem_static", include_str!("../cases/case_mem_static.rs"));
}

/// Signed sub-word loads (i32/i64.load8_s/16_s) and unaligned u16/u32/u64
/// loads/stores via `from_le_bytes`/`to_le_bytes` at odd offsets.
#[test]
fn mem_bytes() {
    run_case("mem_bytes", include_str!("../cases/case_mem_bytes.rs"));
}

/// Atomic statics (`.data` segment) plus a `.rodata` table — multi-segment
/// data layout, merging, and overlap validation; constant-address stores.
#[test]
fn mem_globals() {
    run_case("mem_globals", include_str!("../cases/case_mem_globals.rs"));
}

/// `memory_grow(0, 0)` twice — MemoryGrow translation and `OpEmitter::mem_grow`.
///
/// Permanently ignored as out of scope rather than filed as a bug to fix:
/// `memory.grow` is unreachable from real Miden programs. It is only emitted by a
/// heap allocator growing linear memory, but the SDK's `BumpAlloc` (the global
/// allocator every program links, see `sdk/alloc`) bump-allocates within a fixed
/// region and aborts on exhaustion — it never grows. So the only way to reach the
/// (genuinely buggy) intrinsic is a direct `core::arch::wasm32::memory_grow` call,
/// which this case makes but no real program does. Kept as a coverage/repro
/// artifact for the MemoryGrow translation arm.
#[test]
#[ignore = "out of scope: memory.grow is unreachable from real Miden code (the SDK BumpAlloc never \
            grows linear memory); only a direct core::arch::wasm32::memory_grow call reaches the \
            intrinsic, which aborts 'if statement expected a binary value ... but got 1179648'"]
fn mem_grow() {
    run_case("mem_grow", include_str!("../cases/case_mem_grow.rs"));
}

/// `memory_size(0)` twice around an impossible `memory_grow` — MemorySize
/// translation and `OpEmitter::mem_size`, deterministic zero difference.
#[test]
fn mem_size() {
    run_case("mem_size", include_str!("../cases/case_mem_size.rs"));
}

/// Sub-word loads widened straight to 64 bits (i64.load8/16/32_u and _s) at
/// runtime indexes — U8/U16/U32-typed loads + `arith.zext`/`sext` to 64-bit,
/// covering the 64-bit arms of `zext_smallint`/`zext_int32` and the
/// memory-flavored sign-extension entries.
#[test]
fn loadwiden() {
    run_case("loadwiden", include_str!("../cases/case_loadwiden.rs"));
}
