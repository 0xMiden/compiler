//! Memory-effect ordering across the middle end (campaign 30): can a pass
//! merge, drop, move or fold a load or store it must not?
//!
//! Every case is a straight line of "load, opaque write, load again" (W1),
//! store-then-load forwarding at every lane and width (W2), program-order
//! sequencing under scheduler pressure (W3), bulk-op lowering (W4) or a
//! `static`/`static mut` read after an in-place write (W6). The opaque write
//! is always something LLVM cannot see through — an `#[inline(never)]`
//! helper, `black_box(&mut _)`, a volatile access, an atomic static, or a
//! bulk op with a runtime range — because at O2 LLVM removes every redundant
//! load it can prove redundant, and a case whose second load was already
//! folded away natively proves nothing about the compiler's passes.

use super::super::harness::{run_case, run_case_with_inputs};

/// W1: five opaque-write kinds (`&mut u32` / `*mut u32` / `&mut [u32; 16]`
/// helpers, `black_box(&mut _)`, `write_volatile`) each between two
/// `hir.load`s of the same runtime-indexed word, no branch in between. IR
/// evidence (`-Z print-ir-after-pass=cse` + `MIDENC_TRACE='pass:cse=trace'`):
/// the whole probe sequence lands in ONE block carrying 24 `hir.load`s, 4
/// `hir.store`s and 3 `hir.exec`s, and the pass changes the entrypoint's
/// `hir.load` count not at all (25 -> 25) while merging 5 `hir.load_local`s.
/// Each pair is separated either by a `hir.exec` — which implements no
/// `MemoryEffectOpInterface`, so `has_other_side_effecting_op_in_between`
/// assumes a write — or by a `hir.store`, declared `Write`.
#[test]
fn cse_reload() {
    run_case("mo_cse_reload", include_str!("../cases/case_mo_cse_reload.rs"));
}

/// Pinned grid for `cse_reload`: the write landing ON the loaded word
/// (`i == j`, low byte 0x00/0x11/0xff), on the NEIGHBOURING word
/// (`j == i + 1`, 0x10/0x21), on a far word of the same array, and the
/// kind-5 `write_volatile` target wrapping onto the probe.
#[test]
fn cse_reload_edges() {
    run_case_with_inputs(
        "mo_cse_reload",
        include_str!("../cases/case_mo_cse_reload.rs"),
        &[
            (0, 0x0000_0000),
            (1, 0x0000_0011),
            (0xffff_ffff, 0x0000_00ff),
            (0x8000_0000, 0x0000_0010),
            (7, 0x0000_0021),
            (0x7fff_ffff, 0x0000_000f),
            (0x1234_5678, 0x0000_00f0),
            (3, 0x0000_0088),
        ],
    );
}

/// W1 at mixed widths: u8, i8 (`wasm.i32_load_8s`), u16/i16 at odd offsets,
/// unaligned u32 and u64, plus a cross-width pair (u8 before the write, the
/// containing u32 after it). IR evidence: after `cse` every one of the
/// twelve `hir.load`s survives (12 -> 12, with 2 `hir.load_local`s merged),
/// and the probe block holds 11 loads beside the 7 `hir.exec` pokes. The
/// signed sub-word reads appear as `wasm.i32_load_8s` / `wasm.i32_load_16s`,
/// which declare `Read` on their address operand exactly like `hir.load`.
#[test]
fn cse_widths() {
    run_case("mo_cse_widths", include_str!("../cases/case_mo_cse_widths.rs"));
}

/// Pinned grid for `cse_widths`: the poked byte equal to the probe offset,
/// one past it (so a u16/u32/u64 read straddles the write), three past it
/// (the last byte of the probed u32), the element-straddling offsets 3 and
/// 31, and a poke far outside every footprint.
#[test]
fn cse_widths_edges() {
    run_case_with_inputs(
        "mo_cse_widths",
        include_str!("../cases/case_mo_cse_widths.rs"),
        &[
            (0, 0x0000_0000),
            (1, 0x0000_0020),
            (0xffff_ffff, 0x0000_0043),
            (0x8000_0000, 0x0000_0063),
            (5, 0x0000_0003),
            (0x0100_0193, 0x0000_001f),
            (0x9e37_79b9, 0x0000_03ff),
            (17, 0x0000_0202),
        ],
    );
}

/// W1 with bulk opaque writes: `copy_from_slice` from a static,
/// `fill`/`write_bytes`, `copy_nonoverlapping`, a disjoint `copy_within`,
/// and an opaquely zero-length `fill`, each between two loads of the same
/// byte. IR evidence: after `cse` all ten `hir.load`s survive (10 -> 10) and
/// the three `hir.mem_cpy` (Read on source, Write on destination) and three
/// `hir.mem_set` (Write on destination) are still there. Note what this case
/// does NOT test: LLVM guards every runtime-length bulk op with its own
/// `len != 0` branch, so the entrypoint has 27 blocks and each load pair
/// straddles a branch — CSE's same-block requirement already rules out the
/// merge. The effect check itself is exercised by `cse_reload`, `cse_widths`
/// and `cse_atomic`, whose probes are single-block; this case checks the
/// VALUES across the bulk lowerings.
#[test]
fn cse_bulk() {
    run_case("mo_cse_bulk", include_str!("../cases/case_mo_cse_bulk.rs"));
}

/// Pinned grid for `cse_bulk`: length 0 (every bulk op empty), length 1 at
/// the probed byte, the destination one byte before/after the probe, the
/// destination covering the probe from the left, and the largest length (8)
/// at offset 0.
#[test]
fn cse_bulk_edges() {
    run_case_with_inputs(
        "mo_cse_bulk",
        include_str!("../cases/case_mo_cse_bulk.rs"),
        &[
            (0, 0x0000_0000),
            (0x0000_0020, 0x0000_0000),
            (0x0000_0120, 0x0000_0021),
            (0x0000_0020, 0x0000_0201),
            (0x0000_0100, 0x0000_0008),
            (0xffff_ffff, 0x0000_03ff),
            (0x9e37_79b9, 0x0000_0155),
            (0x0100_0193, 0x0000_02aa),
        ],
    );
}

/// W1, the cross-object direction: an atomic RMW and a `static mut` bump
/// between two frame loads (values must AGREE), a frame store between two
/// reads of the statics (same in reverse), and a `write_volatile` of the
/// loaded word itself (values must DIFFER). IR evidence: after `cse` the
/// frame loads around the two helper calls are still two `hir.load`s (22 ->
/// 22 over the function, 21 loads / 4 stores / 3 execs in the probe block) —
/// the `hir.exec` in between has no `MemoryEffectOpInterface`, so CSE treats
/// it as a write and refuses the merge even though the merge would have been
/// legal here.
#[test]
fn cse_atomic() {
    run_case("mo_cse_atomic", include_str!("../cases/case_mo_cse_atomic.rs"));
}

/// W2: store-then-load at every lane and width of a 4-aligned `[u32; 9]` —
/// u32 store then u8/i8 lane k, four u8 lane stores then the whole word, a
/// u16 store at byte offset 0..3 (3 straddles the element boundary) then both
/// touched words, and a u32 store then an unaligned u32 load one byte later —
/// each probe once bare and once with a pinned `hir.exec` between the store
/// and the load.
#[test]
fn fwd_lanes() {
    run_case("mo_fwd_lanes", include_str!("../cases/case_mo_fwd_lanes.rs"));
}

/// Pinned grid for `fwd_lanes`: every byte lane (k = 0..3) of the first, a
/// middle and the last usable word, so the u16 store at lane 3 straddles the
/// element boundary at each of them.
#[test]
fn fwd_lanes_edges() {
    run_case_with_inputs(
        "mo_fwd_lanes",
        include_str!("../cases/case_mo_fwd_lanes.rs"),
        &[
            (0x0100_0193, 0x0000_0000),
            (0x0100_0193, 0x0000_0008),
            (0x0100_0193, 0x0000_0010),
            (0x0100_0193, 0x0000_0018),
            (0xffff_ffff, 0x0000_001b),
            (1, 0x0000_001f),
            (0x8000_0000, 0x0000_0004),
            (0x9e37_79b9, 0x0000_0015),
        ],
    );
}

/// W2: one `#[repr(C, align(4))]` buffer seen through a `*mut u8` and a
/// `*mut u32` view plus `align_to_mut::<u32>()`, written through one view and
/// read through the other in both orders, with and without a pinned call in
/// between. The alignment attribute is load-bearing: a bare `[u8; N]` would
/// split `align_to` by the runtime address, which is a layout difference
/// between the two targets, not a compiler one.
#[test]
fn alias_views() {
    run_case("mo_alias_views", include_str!("../cases/case_mo_alias_views.rs"));
}

/// Pinned grid for `alias_views`: `align_to` split offsets 0..7 (offset 0 =
/// empty prefix, 1..3 = three-, two- and one-byte prefixes) crossed with each
/// byte lane of the u32 view.
#[test]
fn alias_views_edges() {
    run_case_with_inputs(
        "mo_alias_views",
        include_str!("../cases/case_mo_alias_views.rs"),
        &[
            (0, 0x0000_0000),
            (4, 0x0000_0001),
            (8, 0x0000_0002),
            (12, 0x0000_0003),
            (0xffff_ffff, 0x0000_0004),
            (0x9e37_79b9, 0x0000_0025),
            (1, 0x0000_003e),
            (0x8000_0000, 0x0000_0007),
        ],
    );
}

/// W2: in-place element moves at runtime indexes — `slice::swap`,
/// `mem::replace`, `mem::take`, `mem::swap` through `split_at_mut` and
/// `ptr::swap` — on index pairs that may coincide, each with and without a
/// pinned call before the read-back.
#[test]
fn swap_take() {
    run_case("mo_swap_take", include_str!("../cases/case_mo_swap_take.rs"));
}

/// Pinned grid for `swap_take`: i == j (the self-swap every shape must
/// survive), adjacent indexes, the two ends of the array, and the
/// `split_at_mut` halves' extremes.
#[test]
fn swap_take_edges() {
    run_case_with_inputs(
        "mo_swap_take",
        include_str!("../cases/case_mo_swap_take.rs"),
        &[
            (0, 0x0000_0000),
            (1, 0x0000_0011),
            (0xffff_ffff, 0x0000_00bb),
            (0x9e37_79b9, 0x0000_0001),
            (7, 0x0000_0010),
            (0x8000_0000, 0x0000_000b),
            (0x0100_0193, 0x0000_00b0),
            (3, 0x0000_0056),
        ],
    );
}

/// W3: program order inside one expression — `a[i]` read across `a[j] = v`,
/// sum loops writing ahead of and behind their own read cursor, a helper
/// that returns the old value and writes a new one called twice on the SAME
/// slot in one expression, and a `mem::replace` accumulator. No freight: this
/// is the baseline the freight twin is compared against.
#[test]
fn seq_expr() {
    run_case("mo_seq_expr", include_str!("../cases/case_mo_seq_expr.rs"));
}

/// W3 under pressure: the `seq_expr` shapes with the campaign-20 freight —
/// eight u64 values live across every memory probe and consumed in one wide
/// right-leaning expression, three masked rotate-count bands used before and
/// after. Spill evidence
/// (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`): 77 spills, 74
/// "convert reload to load", one edge to split (`^block21` materialized) and
/// 7 erased split reloads, against 38 / 44 / 0 / 2 for the freight-free
/// `seq_expr`. The scheduler spills AROUND the loads and stores without
/// reordering them.
#[test]
fn seq_freight() {
    run_case("mo_seq_freight", include_str!("../cases/case_mo_seq_freight.rs"));
}

/// W4: `copy_from_slice` / `copy_nonoverlapping` between disjoint halves of
/// one 4-aligned buffer at all sixteen combinations of (src % 4, dst % 4) and
/// lengths 0..=17 — the whole input space of the memcpy element fast path
/// (`src % 4 == dst % 4 == count % 4 == 0`) and its byte fallback loop — plus
/// a u32-element copy for the non-byte-pointer arm.
#[test]
fn copy_grid() {
    run_case("mo_copy_grid", include_str!("../cases/case_mo_copy_grid.rs"));
}

/// Pinned grid for `copy_grid`: length 0, the four element-aligned lengths
/// (4/8/12/16) at src % 4 == dst % 4 == 0 (the fast path), the same lengths
/// at mismatched offsets (the fallback), and the odd lengths 1 and 17.
#[test]
fn copy_grid_edges() {
    run_case_with_inputs(
        "mo_copy_grid",
        include_str!("../cases/case_mo_copy_grid.rs"),
        &[
            (0, 0),
            (0, 4),
            (0, 8),
            (0, 16),
            (1, 4),
            (4, 4),
            (5, 12),
            (0, 1),
            (0, 17),
            (15, 17),
        ],
    );
}

/// W4: forward-overlapping `copy_within` (dst < src) at overlap distances
/// 1..=8 with an odd length, so the lowering always takes the byte fallback
/// loop, which copies upward — the correct direction for this overlap, and
/// it agrees with native at every distance.
///
/// Every length here is odd ON PURPOSE. The first draft of this case had a
/// second copy of length `len + 1`, and the pinned grid's `(0x20, 0x38)` rung
/// (distance 4, 16 bytes, 4-aligned on both ends) aborted in
/// `miden-core-lib memcopy_elements` with "source and destination ranges must
/// not overlap" (mem.masm:100, operand stack `[0, 4, 262137, 262136, ..]` =
/// n 4 elements, rp one element above wp). That is the known
/// `memory::mem_overlap` / `memory::copy_same_pos` class — the element fast
/// path has no memmove semantics — and it shows the assert rejects overlap in
/// BOTH directions, not just `dst > src`. Not retried here.
#[test]
fn copy_fwd() {
    run_case("mo_copy_fwd", include_str!("../cases/case_mo_copy_fwd.rs"));
}

/// Pinned grid for `copy_fwd`: every overlap distance 1..=8 at a length that
/// exceeds it (so the ranges really overlap), plus a length below the
/// distance (disjoint) and the zero-word element-path copy.
#[test]
fn copy_fwd_edges() {
    run_case_with_inputs(
        "mo_copy_fwd",
        include_str!("../cases/case_mo_copy_fwd.rs"),
        &[
            (0x0000_0000, 0x0000_0038),
            (0x0000_0008, 0x0000_0038),
            (0x0000_0010, 0x0000_0038),
            (0x0000_0018, 0x0000_0038),
            (0x0000_0020, 0x0000_0038),
            (0x0000_0028, 0x0000_0038),
            (0x0000_0030, 0x0000_0038),
            (0x0000_0038, 0x0000_0038),
            (0x0000_0038, 0x0000_0000),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// W4: `fill` and `write_bytes` over unaligned runtime ranges at every
/// (start % 4, length % 4), a `memset` of a range inside one element read
/// back at u8/i8/u16/u32 width, and an element-typed `[u32]::fill`.
/// `OpEmitter::memset` is a per-byte load/mask/or/store loop, so the lanes
/// outside the range are the thing under test.
#[test]
fn fill_ranges() {
    run_case("mo_fill_ranges", include_str!("../cases/case_mo_fill_ranges.rs"));
}

/// Pinned grid for `fill_ranges`: length 0, length 1 at each start % 4, a
/// range that covers exactly one element, one that straddles two, and the
/// in-element memset at each of its six lengths.
#[test]
fn fill_ranges_edges() {
    run_case_with_inputs(
        "mo_fill_ranges",
        include_str!("../cases/case_mo_fill_ranges.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (0, 4),
            (1, 4),
            (3, 6),
            (0x0000_0020, 0x0000_0207),
            (0x0000_00c0, 0x0000_0403),
        ],
    );
}

/// W4: stores and loads immediately around a bulk op — a byte written inside
/// the source range just before the copy and read back at both the source and
/// the destination just after, the same with a fill, an element-aligned copy
/// with a wide store before and a wide load after, and a copy whose source
/// was produced by a fill.
#[test]
fn copy_seam() {
    run_case("mo_copy_seam", include_str!("../cases/case_mo_copy_seam.rs"));
}

/// W6: an immutable `.rodata` table read at a constant and at a runtime index
/// across writes to its `static mut` `.data` twin, the twin written and read
/// back in one block with and without a pinned call, and an `AtomicU32`
/// `swap` folded into the result. IR evidence: after
/// `sparse-conditional-constant-propagation` the dump is identical to its
/// input (27 `hir.load`s in, 27 out), and both `canonicalizer` runs keep the
/// same 27 loads and 10 stores while collapsing 183 -> 55 `arith.constant`s.
/// No memory model exists in either pass and there is no `Foldable` impl for
/// `hir.load`/`hir.store`, so a `static` is never constant-folded through a
/// load. Every mutable static is restored before returning.
#[test]
fn static_write() {
    run_case("mo_static_write", include_str!("../cases/case_mo_static_write.rs"));
}

/// Pinned grid for `static_write`: the constant index 5 as the runtime index
/// too, the two ends of the table, and the wrap of the second poke's
/// `(i + 1) & 15` onto index 0.
#[test]
fn static_write_edges() {
    run_case_with_inputs(
        "mo_static_write",
        include_str!("../cases/case_mo_static_write.rs"),
        &[
            (0, 0x0000_0005),
            (1, 0x0000_0000),
            (0xffff_ffff, 0x0000_000f),
            (0x9e37_79b9, 0x0000_0004),
            (0x0100_0193, 0x0000_000e),
            (0x8000_0000, 0x0000_0001),
        ],
    );
}
