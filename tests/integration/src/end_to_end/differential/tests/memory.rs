//! Loads/stores, memcpy/fill, data segments, and memory intrinsics.

use super::super::harness::{run_case, run_case_with_inputs};

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
#[ignore = "native/MASM divergence: memory.copy with overlapping dst > src ranges; the VM now \
            hard-aborts via the miden-core-lib memcopy overlap assert (hash-coded error \
            14467508661128000855, re-verified 2026-08-27; e.g. inputs (4294967295, 194795201) — \
            original repro (91264998, 3811523388) in pre-split mem_copy). Un-ignore when the \
            lowering handles overlapping copies with memmove semantics"]
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

/// Compare two zero-growth results without assuming a Wasm initial page count.
#[test]
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

/// Exercises a large `.bss` static spanning the region where compiler-managed memory (globals,
/// function tables, heap) would land if the layout ignored the wasm minimum memory size.
#[test]
fn static_bss() {
    run_case("static_bss", include_str!("../cases/case_static_bss.rs"));
}

/// Wasm-local shapes for the `Local2Reg` pass: an unused parameter (its entry
/// `store_local` is a dead store to erase), a zero-parameter zero-local helper
/// (the no-locals early return), and a by-value array parameter whose
/// single-use pointer local is promoted.
#[test]
fn local_shapes() {
    run_case("local_shapes", include_str!("../cases/case_local_shapes.rs"));
}

/// Sub-word lanes (campaign 13): volatile byte stores at every lane of four
/// words read back whole (element-space u32 loads of a byte buffer) and in
/// permuted lane order, halfword stores/loads at byte offsets 0..3 (odd =
/// unaligned, 3 = element-straddling), bytes out of a frame-stored u64 at a
/// runtime lane, and negative i8/i16 lanes sign-extended through 32- and
/// 64-bit loads; the whole record is hashed so a clobbered lane shows.
#[test]
fn lane_bytes() {
    run_case("lane_bytes", include_str!("../cases/case_lane_bytes.rs"));
}

/// Packed and over-aligned records (campaign 13): a 21-byte
/// `#[repr(C, packed)]` record in a runtime-indexed `[Packed; 4]` puts every
/// u16/u32/u64/i16 field at all four byte offsets within an element
/// (unaligned `load_u16`/`load_sw`/`load_dw` and their store twins), inside
/// an `align(32)` wrapper beside a `#[repr(C)]` record with padding holes.
#[test]
fn packed_fields() {
    run_case("packed_fields", include_str!("../cases/case_packed_fields.rs"));
}

/// Word-straddling wide values (campaign 13): u64 at byte offsets 4 mod 8
/// (`i64.load/store` with 4-byte alignment at an odd element address), u128
/// at offsets 4/8/12/16 (i64 halves straddling Miden words), i64 at odd byte
/// offsets (three-element reassembly + arithmetic shift), u64 at every byte
/// offset; all written back at other straddling offsets and the whole
/// buffer hashed.
#[test]
fn straddle_wide() {
    run_case("straddle_wide", include_str!("../cases/case_straddle_wide.rs"));
}

/// Runtime-length copies and fills (campaign 13): lengths from
/// {0,1,3,4,5,7,8,9,15,16,17,31,32,33,2,6} at src/dst byte offsets 0..3 —
/// static -> stack, stack -> stack into the middle of a larger buffer, a
/// disjoint in-buffer `copy_within`, a non-zero `fill`, and an element-typed
/// copy; both destinations hashed whole so wrong length/direction or a fill
/// spilling past its range changes the result.
#[test]
fn copy_ladder() {
    run_case("copy_ladder", include_str!("../cases/case_copy_ladder.rs"));
}

/// Mixed static data segments (campaign 13): [u8; 13], [u16; 7], [u32; 5],
/// [u64; 3], a packed static record, a `&str`, a zero atomic (.bss) beside a
/// non-zero one (.data) updated across two `#[inline(never)]` calls and
/// restored, a 4 KiB byte table indexed by both inputs, and interior slices
/// of statics passed to a helper.
#[test]
fn segment_mix() {
    run_case("segment_mix", include_str!("../cases/case_segment_mix.rs"));
}

/// Large stack frame (campaign 13): `[u64; 200]` + `[u8; 900]` locals whose
/// addresses escape to `#[inline(never)]` helpers (one with its own 512-byte
/// frame below the caller's) beside promotable scalars live across the
/// calls; every element is folded into the result.
#[test]
fn big_frame() {
    run_case("big_frame", include_str!("../cases/case_big_frame.rs"));
}

/// Slicing and pointer arithmetic (campaign 13): runtime sub-slices,
/// `chunks_exact(3)`, `windows(5)`, `split_at_mut`, reversed in-place
/// rewrite, element swaps, and `binary_search` over a static sorted table
/// (slice rotation excluded: its overlapping memmove is the known
/// `mem_overlap` bug).
#[test]
fn slice_ops() {
    run_case("slice_ops", include_str!("../cases/case_slice_ops.rs"));
}

/// Atomic and volatile traffic (campaign 13): `AtomicU32`/`AtomicU64`/
/// `AtomicU8`/`AtomicU16` load/store/fetch_add/fetch_xor/swap/
/// compare_exchange sequences on `.data` statics (restored), and
/// `read_volatile`/`write_volatile`/`read_unaligned`/`write_unaligned` of
/// u8/u16/u32 at runtime byte offsets of a frame buffer.
#[test]
fn atomics_volatile() {
    run_case("atomics_volatile", include_str!("../cases/case_atomics_volatile.rs"));
}

/// Frame close to the 1 MiB shadow-stack limit (campaign 13): a
/// `[MaybeUninit<u32>; 250000]` (1,000,000 bytes) local whose address
/// escapes to a helper with its own 4 KiB frame, so the stack pointer
/// descends to ~0x0B000; a stride-99991 stripe plus the first and last
/// elements are written and read back.
#[test]
fn frame_1m() {
    run_case("frame_1m", include_str!("../cases/case_frame_1m.rs"));
}

/// `#[repr(C, packed(2))]` records (campaign 13): u32/u64/u128 fields with
/// 2-byte alignment promises (`align=1` memargs, the byte-space `mod 2`
/// alignment assert) at 2 mod 4 for odd record indexes, inside an
/// `align(64)` wrapper, written and read back through permuted indexes.
#[test]
fn packed2_fields() {
    run_case("packed2_fields", include_str!("../cases/case_packed2_fields.rs"));
}

/// Narrow stores of 64-bit values and widening loads back (campaign 13):
/// `i64.store8/16/32` at runtime byte offsets 0..3 (`trunc_int64` feeding
/// the unaligned sub-word/word stores) and `i64.load8/16/32_u/_s` at a
/// second runtime offset LLVM cannot prove equal, so nothing is forwarded;
/// the buffer is hashed whole.
#[test]
fn narrow64() {
    run_case("narrow64", include_str!("../cases/case_narrow64.rs"));
}

/// Pointers in data segments and in the frame (campaign 13): static tables
/// of `&[u8]`/`&str`/`&[u16]` fat pointers selected at runtime, interior
/// slices of statics cut at runtime, and a frame-resident array of slice
/// references over runtime sub-ranges of a stack buffer.
#[test]
fn ptr_table() {
    run_case("ptr_table", include_str!("../cases/case_ptr_table.rs"));
}

/// MASM-ONLY TRAP (campaign 13, 2026-09-02): `copy_within` whose runtime
/// destination coincides EXACTLY with its source (shift 0) on u32 element
/// ranges. Natively `memmove(p, p, n)` is a no-op; LLVM keeps the
/// `memory.copy` because the destination is a runtime value. The byte
/// addresses and byte count are 4-aligned, so `OpEmitter::memcpy` takes its
/// element fast path and execs miden-core-lib `memcopy_elements`, whose
/// overlap assert (`wp >= rp + n || rp >= wp + n`) rejects `wp == rp` with
/// `n > 0`: "assertion failed with error message: source and destination
/// ranges must not overlap" (miden-core mem.masm:100), operand stack
/// `[0, 2, 262116, 262116, ..]` = n 2, rp == wp. First random failure:
/// inputs (2340168019, 3869789317); the pinned twin also uses (0, 0)
/// (src 0, n 1). Distinct from `mem_overlap` (dst > src, data must move):
/// here NO byte needs to move, and the byte fallback arm already handles
/// the identical range (`copy_same_bytes` passes by copying each byte onto
/// itself). Bounded by `copy_same_pos_disjoint` (same fast path, disjoint
/// ranges) and `copy_same_bytes`. Un-ignore when the fast path skips (or
/// the core-lib assert accepts) `src == dst`, or when memcpy gains memmove
/// semantics (which fixes `mem_overlap` too).
#[test]
#[ignore = "MASM-only trap: identical-range copy_within (src == dst, n > 0) on 4-aligned u32 \
            ranges takes the memcpy element fast path and aborts in miden-core-lib \
            memcopy_elements with 'source and destination ranges must not overlap' (mem.masm:100), \
            natively a no-op memmove; e.g. inputs (2340168019, 3869789317) and (0, 0). Un-ignore \
            when src == dst is accepted by the fast path or memcpy gains memmove semantics"]
fn copy_same_pos() {
    run_case("copy_same_pos", include_str!("../cases/case_copy_same_pos.rs"));
}

/// Pinned twin of `copy_same_pos`: the smallest identical-range pair (0, 0)
/// (src 0, dst 0, one element) plus the first random failing pair.
#[test]
#[ignore = "MASM-only trap on pinned inputs (0, 0) and (2340168019, 3869789317): miden-core-lib \
            memcopy_elements 'source and destination ranges must not overlap' for an \
            identical-range copy_within; see copy_same_pos"]
fn copy_same_pos_repro() {
    run_case_with_inputs(
        "copy_same_pos",
        include_str!("../cases/case_copy_same_pos.rs"),
        &[(0, 0), (2340168019, 3869789317)],
    );
}

/// Passing bound for `copy_same_pos`: the same case on inputs whose shift
/// bit is set (`input1 >> 2` odd), so the destination is 16 elements past
/// the source — the element fast path with disjoint ranges, every source
/// start (0/4/8) and every length (1..=4 elements).
#[test]
fn copy_same_pos_disjoint() {
    run_case_with_inputs(
        "copy_same_pos",
        include_str!("../cases/case_copy_same_pos.rs"),
        &[
            (4, 0),
            (5, 1),
            (6, 2),
            (7, 3),
            (13, 3),
            (14, 0),
            (0xffff_fff7, 0xffff_ffff),
            (2340168023, 3869789317),
        ],
    );
}

/// Passing sibling of `copy_same_pos`: the identical-range `copy_within` on
/// a byte buffer with odd length/start takes the memcpy byte fallback loop
/// (each byte copied onto itself), which agrees with the native no-op.
#[test]
fn copy_same_bytes() {
    run_case("copy_same_bytes", include_str!("../cases/case_copy_same_bytes.rs"));
}
