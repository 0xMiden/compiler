//! Cases whose value is that a spill slot is a MEMORY LOCATION, not a
//! register: it must be per activation, it must not overlap the function's own
//! stack arrays, and it must survive calls to callees that spill and own large
//! frames themselves.
//!
//! Two layout facts make these guards rather than open questions, and they are
//! what the cases assert end to end. First, a spill slot is an ordinary HIR
//! function local — `TransformSpills` allocates one per spilled value and
//! rewrites each reload into a `hir.load_local` of it — and the emitter
//! addresses every local with `locaddr` (`OpEmitter::local_address`,
//! `codegen/masm/src/emit/mem.rs:122`, offset from
//! `LocalVariable::absolute_offset`), which is FMP-relative: the assembler
//! advances FMP by the procedure's WORD-aligned local count on entry
//! (`codegen/masm/src/lower/component.rs:1574`), so every activation of a
//! procedure gets its own slots by construction. Second, those slots live in
//! the VM's frame region — FMP starts at element address 2^31
//! (`miden_core::FMP_INIT_VALUE`) — while the guest's stack arrays live in the
//! wasm linear memory the compiler maps to element addresses below 2^19 (a
//! 1 MiB shadow stack growing down from byte 0x100000, where the data segments
//! start). The two regions cannot overlap, so no `[u64; N]` frame array can
//! alias a spill slot however large it is.
//!
//! What is NOT structural is everything around them: the spill analysis
//! decides what is live across a call, cfg-to-scf rewrites the regions the
//! reloads sit in, and the same emitter addresses the frame arrays and the
//! slots. So every case here value-checks the composition on a pinned grid —
//! recursion depths, the frame extremes, every call level — and every one of
//! them was trace-verified to spill in the function it claims to
//! (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`; the counts are
//! in each doc comment as spills / reloads / spill-slot `load_local`s).
//!
//! Recursion is only expressible through a function-pointer table: the
//! ASSEMBLER's linker rejects a direct or mutual call-graph cycle ("found a
//! cycle in the call graph", `miden-assembly/src/linker/errors.rs:41`), so the
//! `rec_*` cases close their cycles with `call_indirect` (`hir.exec_indirect`
//! -> `dynexec`, which stays in the caller's memory context and therefore
//! shares one FMP chain with it — only the new-context `dyncall` resets FMP).
//! Each dispatch takes two arguments so the argument-blindness panic of
//! `calls::indirect_spill` (the spill analysis reads only operand group 0)
//! stays out of the way.

use super::super::harness::{run_case, run_case_with_inputs};

/// Per-activation spill slots under recursion: an eight-u64 cluster live
/// across the recursive call (which sits between the two loops that share the
/// cluster), the cycle closed through a three-entry function-pointer table,
/// depth `input1 % 6`. Spill evidence in `rec`: 3 spills, 6 reloads, 5 split
/// edges — all six reloads land in split blocks and are erased again (the
/// stale-dominator-tree erasure `pressure::zero_trip_overflow` blames its
/// panic on), so the surviving-reload half of the hypothesis is `rec_freight`
/// below.
#[test]
fn rec_slots() {
    run_case("rec_slots", include_str!("../cases/case_rec_slots.rs"));
}

/// Pinned depths 0..5 plus the table-index extremes for `rec_slots`: if the
/// callee's slots were the caller's, the answer would change with the depth.
#[test]
fn rec_slots_edges() {
    run_case_with_inputs(
        "rec_slots_edges",
        include_str!("../cases/case_rec_slots.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 7),
            (3, 0x8000_0000),
            (4, 0xffff_ffff),
            (5, 0x7fff_ffff),
            (11, 3),
            (0xffff_ffff, 0xffff_ffff),
        ],
    );
}

/// The same recursion carrying the full campaign-20 freight recipe — a u64
/// cluster consumed by one wide right-leaning non-reassociable chain per loop
/// body plus masked rotate count bands used before, inside and after both
/// loops — at the largest rung that compiles. Here a spill slot is really READ
/// back after the recursive call returns: 4 spills, 5 reloads, one converted
/// to a spill-slot `hir.load_local` (the other four erased on split edges).
///
/// The rung is (3 cluster values, 3 bands). The ladder around it, all with the
/// dispatch between the two loops: (2, 2), (2, 4) and (3, 3) compile and pass;
/// (4, 2) panics with the in-window arity-2 `NoSolution` (`lowering.rs:109`,
/// `arith.rotl`, `[Copy, Copy]`, exactly 16 felts on the stack) and (4, 4),
/// (6, 6), (8, 8) panic with the over-full stack (`emit/mod.rs:623`, indexes
/// 11 / 13 / 14). Cluster values are the expensive axis in a recursive frame,
/// bands the cheap one.
#[test]
fn rec_freight() {
    run_case("rec_freight", include_str!("../cases/case_rec_freight.rs"));
}

/// Mutual recursion with asymmetric freight: `ra` (three-u64 cluster, one
/// loop, no bands) and `rb` (the same cluster across two loops with the
/// dispatch between them, plus four count bands used before, inside both
/// bodies and after) call each other through the table, so at every second
/// level the callee spills strictly more than its caller and then consumes the
/// callee's result together with its own reloaded cluster. The asymmetry is
/// carried by loops and bands rather than by cluster size because `ra` with a
/// four-u64 cluster and `rb` with six or eight both hit the same arity-2
/// `NoSolution` as the `rec_freight` (4, 2) rung.
#[test]
fn rec_mutual() {
    run_case("rec_mutual", include_str!("../cases/case_rec_mutual.rs"));
}

/// Pinned depths 0..4 entered through both recursive functions for
/// `rec_mutual`.
#[test]
fn rec_mutual_edges() {
    run_case_with_inputs(
        "rec_mutual_edges",
        include_str!("../cases/case_rec_mutual.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (2, 2),
            (3, 3),
            (4, 0),
            (4, 1),
            (9, 0xffff_ffff),
            (0xffff_ffff, 0x8000_0000),
        ],
    );
}

/// Spill slots beside a 256 KiB frame array (`[MaybeUninit<u32>; 65536]`): a
/// stride-1637 stripe written before the spills, the array's first and last
/// element written at a runtime index WHILE the cluster is spilled, the stripe
/// checksummed after the reloads, then the mirror order (spill, write,
/// reload). 28 spills / 37 reloads / 18 spill-slot `load_local`s / 4 split
/// edges, and the locals table grows from 22 entries after Local2Reg to 40
/// after TransformSpills — the eighteen slots are locals 22..39, above every
/// user local, and none of them can reach the frame array in linear memory.
#[test]
fn frame_spills() {
    run_case("frame_spills", include_str!("../cases/case_frame_spills.rs"));
}

/// Pinned frame extremes (`input1` bit 0 selects the index-0 vs index-65535
/// write) and trip counts 2..4 for `frame_spills`.
#[test]
fn frame_spills_edges() {
    run_case_with_inputs(
        "frame_spills_edges",
        include_str!("../cases/case_frame_spills.rs"),
        &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 2),
            (0xffff_ffff, 0xffff_ffff),
            (0x8000_0000, 0x7fff_ffff),
        ],
    );
}

/// Callee clobbering: three call levels, each with its own u64 cluster spilled
/// across the call to the next level, each owning a `[u32; 256]` frame it
/// `memset`s and then overwrites with a runtime-length `memcpy`, each reading
/// its own cluster back after the callee returns. All three levels really
/// spill — `lvl_a` 8 spills / 10 reloads / 6 `load_local`s, `lvl_b` 2 / 2 / 1,
/// `lvl_c` 8 / 8 / 7 — so the caller's slots are live while the callee fills
/// its whole frame.
#[test]
fn call_clobber() {
    run_case("call_clobber", include_str!("../cases/case_call_clobber.rs"));
}

/// Pinned copy lengths and trip counts for every level of `call_clobber`.
#[test]
fn call_clobber_edges() {
    run_case_with_inputs(
        "call_clobber_edges",
        include_str!("../cases/case_call_clobber.rs"),
        &[(0, 0), (1, 1), (2, 2), (3, 3), (6, 0xffff_ffff), (0xffff_ffff, 0)],
    );
}

/// Spills live across the bulk frame operations that write the frame around
/// them: a runtime-length `core::ptr::write_bytes` (`memory.fill` ->
/// `memset`), a 40-byte `copy_from_slice` below the inline-copy threshold, a
/// 200-byte one above it (`memory.copy` -> `memcpy`), and a 32-byte fill below
/// the bulk-fill threshold, all inside the loop the cluster is spilled across.
/// 13 spills / 14 reloads / 12 spill-slot `load_local`s, so the reloads that
/// follow the fills really do read slots.
#[test]
fn fill_spills() {
    run_case("fill_spills", include_str!("../cases/case_fill_spills.rs"));
}

/// Pinned trip counts and fill/copy offsets for `fill_spills`.
#[test]
fn fill_spills_edges() {
    run_case_with_inputs(
        "fill_spills_edges",
        include_str!("../cases/case_fill_spills.rs"),
        &[(0, 0), (1, 1), (255, 2), (0xffff_ffff, 0xffff_ffff), (0x8000_0000, 3)],
    );
}

/// Spills across the two call shapes that write memory behind the caller's
/// back: a two-argument function-pointer dispatch and two calls returning wide
/// records through a hidden return-area pointer (`[u64; 4]` and
/// `(u64, u64, u64)`), whose callees write into the caller's frame while its
/// eight-u64 cluster is spilled. The heaviest spiller in the module: 44 spills
/// / 55 reloads / 43 spill-slot `load_local`s / 2 split edges.
#[test]
fn spill_sret() {
    run_case("spill_sret", include_str!("../cases/case_spill_sret.rs"));
}

/// Pinned dispatch indexes and trip counts for `spill_sret`.
#[test]
fn spill_sret_edges() {
    run_case_with_inputs(
        "spill_sret_edges",
        include_str!("../cases/case_spill_sret.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (4, 3),
            (0xffff_ffff, 0xffff_ffff),
            (0x8000_0000, 0x7fff_ffff),
        ],
    );
}

/// Deep recursion x frame size, the largest rung that FITS: nine activations
/// of a `[MaybeUninit<u64>; 8192]` frame (65 536 bytes each, 589 824 bytes
/// total) recursing through the table, each writing and reading back its
/// first, last and one runtime-indexed element around the recursive call. The
/// guest shadow stack is 1 MiB — `__stack_pointer` is initialized to 1048576
/// in the guest wasm and grows down towards 0, with the data segments starting
/// at 0x100000 — so this rung stays inside it, and `wasmtime` runs the same
/// harness-built wasm without a trap.
#[test]
fn deep_frames() {
    run_case("deep_frames", include_str!("../cases/case_deep_frames.rs"));
}

/// The first rung PAST the guest shadow stack, kept because what it documents
/// is a diagnostic gap rather than a wrong answer: nine activations of a
/// `[MaybeUninit<u64>; 16384]` frame ask for 1 179 648 bytes of a 1 MiB stack,
/// so `__stack_pointer` wraps below zero and the deepest frame lands at
/// 0xFFFE0000. Every access from it is out of bounds for the declared wasm
/// memory (17 pages), and `wasmtime` on exactly this harness-built wasm traps:
/// "memory fault at wasm address 0xfffffff8 in linear memory of size 0x110000
/// / wasm trap: out of bounds memory access" — 0xfffffff8 is the last element
/// of the deepest frame. The Miden pipeline neither traps nor diagnoses: it
/// maps the wrapped byte address into element space just under 2^30, a region
/// nothing else uses, so the program silently computes the right answer here
/// and would silently corrupt data in a program whose memory lived there.
/// The native side survives because a libtest thread stack is 2 MiB; the next
/// rung up (2 MiB of frames) overflows it and aborts the whole test process,
/// which is why the ladder stops here.
#[test]
fn deep_overrun() {
    run_case("deep_overrun", include_str!("../cases/case_deep_overrun.rs"));
}
