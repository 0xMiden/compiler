//! Cases about OPERATION EQUIVALENCE as CSE uses it: the commutative-operand
//! multiset that upstream fab7b7db0 added to `hir/src/ir/operation/
//! equivalence.rs`, and the reverse-order region erasure the same commit added
//! to `hir/src/patterns/rewriter.rs`.
//!
//! The hard part of this area is producing the input at all. LLVM's EarlyCSE
//! and GVN merge `x + y` with `y + x` whenever both are visible on the same
//! SSA values under dominance, so a hand-written pair of operand orders never
//! survives to the wasm — three straight-line attempts (plain source order,
//! `wrapping_add` beside `unchecked_add`, and a value `black_box` between the
//! copies) all came out of the guest as ONE `i32.add`. The hatch that works is
//! **volatile byte reads**:
//!
//!  - two volatile reads of one address are distinct LLVM values, so LLVM
//!    keeps both commutative ops, and the wasm stack order follows the
//!    (unreorderable) volatile load order — which is how the second op reaches
//!    HIR with its operands swapped;
//!  - in HIR the volatility is gone, so the reloads are ordinary `hir.load`s;
//!  - a 1-BYTE access carries no alignment check. `prepare_addr`
//!    (`dialects/wasm/src/mem.rs`) only calls `enforce_alignment` when
//!    `memarg.align > 0`, and that is what emits the `hir.assertz` — a Write —
//!    that sits between any two wider loads and blocks their merge.
//!
//! So CSE merges the byte reloads first, which makes the operand SSA values of
//! the two later commutative ops identical, and only THEN can the commutative
//! multiset key match. Every `comm_*` case below is that shape; every
//! `noncomm_*` case is the same shape over an op that is NOT `Commutative`,
//! where a merge would be a miscompile visible in the value.
//!
//! Evidence per case was taken with
//! `MIDENC_DIFF_FLAGS='-Z print-ir-after-pass=cse'` and
//! `MIDENC_TRACE='pass:cse=trace,rewriter=trace'`, which print the function
//! before and after the pass and every `replaced <op> with <op>` the rewriter
//! listener saw; the op counts quoted below are per HELPER function, not per
//! module.

use super::super::harness::{run_case, run_case_with_inputs};

/// W0 reach probe: three structurally identical `if`s in one function.
///
/// CSE sees ZERO region-bearing ops. The op-name histogram of `entrypoint`
/// before `cse` is `hir.load_local` 21, `hir.store_local` 11,
/// `arith.constant` 8, `arith.band` 7, `hir.exec` 6, `cf.br` 6,
/// `arith.rotl` 4, `cf.cond_br` 3, `arith.neq` 3, `arith.bxor` 2,
/// `arith.add` 2 — no `scf.*` of any kind, because the wasm frontend emits
/// none and `lift-control-flow` runs six passes after `cse`. The `after` dump
/// is byte-identical to the `before` one and the rewriter listener prints no
/// `replaced` line inside this function.
///
/// The `scf.if`s appear only in the `lift-control-flow` dump (`scf.if` 3,
/// `scf.yield` 6), and the post-lift `canonicalizer`, `sink-operand-defs` and
/// `transform-spills` dumps all carry the same `scf.if` 3 / `scf.yield` 6. So
/// the structural REGION comparison in `equivalence.rs` stays unreachable from
/// plain Rust after the rebase, and what fab7b7db0 changed on this path is the
/// commutative multiset alone.
#[test]
fn twin_if() {
    run_case("cse_twin_if", include_str!("../cases/case_cse_twin_if.rs"));
}

/// Pinned grid for [`twin_if`]: both `if` conditions are bits of the inputs,
/// so these pairs take all four arm combinations plus the third `if`'s two
/// arms, with the boundary values on the captured `a`/`b`.
#[test]
fn twin_if_edges() {
    run_case_with_inputs(
        "cse_twin_if",
        include_str!("../cases/case_cse_twin_if.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 0),
            (3, 1),
            (u32::MAX, u32::MAX),
            (0x8000_0000, 0x8000_0000),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W1, `arith.add` and `arith.mul` in both operand orders.
///
/// wasm evidence (`add_swap`): `i32.load8_u; i32.load8_u offset=1; i32.add`
/// followed by `i32.load8_u offset=1; i32.load8_u; i32.add` — both orders
/// survive to the frontend. CSE evidence: `hir.load` 4 -> 2, `arith.add`
/// 4 -> 2 and `arith.mul` 2 -> 1 in `add_swap` (the surviving add is the
/// address `p + 1`); `arith.mul` 4 -> 2 and `hir.load` 4 -> 2 in `mul_swap`.
/// The rewriter listener prints `replaced op with Some(%99): arith.add` for
/// the swapped copy and `replaced op with Some(%101): arith.mul` for the
/// widening multiply, after the `replaced ... hir.load_local` /
/// `... hir.load` lines that make their operands identical.
///
/// `add_swap_if` is the control: the swapped copy sits in a DOMINATED block
/// instead of a later statement, and nothing merges (`arith.add` 4 -> 4,
/// `hir.load` 4 -> 4) — CSE's memory-read candidates require
/// `existing.parent() == op.parent()`, so a reload in another block never
/// becomes one value and the adds never get matching operands. The merge is
/// unchanged without guest DWARF: the `debug = 0` dump of `add_swap` and
/// `mul_swap` has the same counts, op for op.
#[test]
fn comm_arith() {
    run_case("cse_comm_arith", include_str!("../cases/case_cse_comm_arith.rs"));
}

/// Pinned grid for [`comm_arith`]: the probe operands are the low bytes of the
/// inputs, so these pairs give the byte pairs (0,0), (x,x), (x,!x), (255,1),
/// (128,128) and two mixed ones, on both the `p` and the `p+2` probe.
#[test]
fn comm_arith_edges() {
    run_case_with_inputs(
        "cse_comm_arith",
        include_str!("../cases/case_cse_comm_arith.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x00ff_00ff, 0x0001_0001),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W1, `arith.band` / `arith.bor` / `arith.bxor` in both operand orders.
///
/// CSE evidence per helper, all with `hir.load` 4 -> 2 underneath:
/// `arith.band` 5 -> 3 in `band_swap` (three of those are the rotate masks,
/// so the two probe `band`s became one), `arith.bor` 2 -> 1 in `bor_swap`,
/// `arith.bxor` 2 -> 1 in `bxor_swap`, each with its
/// `replaced op with Some(...): arith.b*` line.
///
/// `arith.and` / `or` / `xor` — the i1 logical members of the
/// `Commutative` list — are unreachable: the wasm frontend maps `I32And` /
/// `I32Or` / `I32Xor` to `band` / `bor` / `bxor` and never builds the logical
/// forms.
#[test]
fn comm_bits() {
    run_case("cse_comm_bits", include_str!("../cases/case_cse_comm_bits.rs"));
}

/// Pinned grid for [`comm_bits`]: the same byte-pair boundary set as
/// [`comm_arith_edges`], where `&`/`|`/`^` of (x, !x) and (x, x) are the
/// values a wrong merge would change.
#[test]
fn comm_bits_edges() {
    run_case_with_inputs(
        "cse_comm_bits",
        include_str!("../cases/case_cse_comm_bits.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x00ff_00ff, 0x0001_0001),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W1, `arith.eq` / `arith.neq` in both operand orders.
///
/// CSE evidence: `arith.eq` 2 -> 1 and `hir.load` 4 -> 2 in `eq_swap`,
/// `arith.neq` 2 -> 1 and `hir.load` 4 -> 2 in `neq_swap`; in `eq_neq_mix`
/// the two `arith.eq` merge with each other (2 -> 1, `hir.load` 6 -> 2) while
/// the `arith.neq` over the same swapped pair stays at 1 (the op NAME is the
/// first thing `is_equivalent_with_mapping` compares).
///
/// The flags are combined with a bare `wrapping_sub` on purpose: a `bool << k`
/// makes LLVM materialize the flag through a `cf.select` of two constants,
/// which parks the probe bytes in wasm locals — and the `hir.store_local` that
/// results is a Write that blocks the reload merge, leaving the whole shape
/// unproven. That is exactly what the first version of this case did.
///
/// `arith.min` / `arith.max` carry the `Commutative` trait too but have no
/// plain-Rust producer at all: wasm has no i32 min/max operator, the frontend
/// never calls `builder.min`/`max`, and `core::cmp::min`/`max` lower to a
/// compare plus a select. They are dropped from this campaign.
#[test]
fn comm_cmp() {
    run_case("cse_comm_cmp", include_str!("../cases/case_cse_comm_cmp.rs"));
}

/// Pinned grid for [`comm_cmp`]: equal bytes, unequal bytes, and the
/// boundaries — `==`/`!=` only change value when the pair flips between equal
/// and unequal, so the equal-pair rows are the load-bearing ones.
#[test]
fn comm_cmp_edges() {
    run_case_with_inputs(
        "cse_comm_cmp",
        include_str!("../cases/case_cse_comm_cmp.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x00ff_00ff, 0x0001_0001),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W2 oracle: `arith.sub` / `shl` / `shr` / `ashr` with swapped operands must
/// NOT merge.
///
/// Exactly the `comm_arith` shape over ops that do not carry `Commutative`.
/// CSE evidence: `arith.sub` 3 -> 3 in `sub_swap` and no `replaced ...
/// arith.sub` line, while the `hir.load`s behind them DO merge (4 -> 2) — so
/// the operands are provably identical and the ops still do not match. Same
/// for `shl_swap` (`arith.shl` 2 -> 2, `hir.load` 4 -> 2) and `shr_swap`
/// (`arith.shr` 2 -> 2, `hir.load` 4 -> 2).
///
/// Two caveats recorded rather than papered over. `ashr_swap` reaches HIR as
/// `arith.shr` on a signed operand type — no `arith.ashr` appears in any dump
/// of this corpus, so the `Ashr` op has no producer here — and its loads do
/// not merge (`wasm.i32_load_8s` 2 -> 2, `hir.load` 2 -> 2), so its 2 -> 2
/// shift count is a weaker non-merge than the others. `sub_flags` puts
/// `wrapping_sub` beside `unchecked_sub` on the same pair in the same order:
/// the LLVM flags differ but wasm has one `i32.sub` and the frontend always
/// builds `arith.sub` with the `wrapping` overflow property. Nothing merges
/// there either (`arith.sub` 3 -> 3, `hir.load` 4 -> 4), because the `>` /
/// `select` pair parks the bytes in locals. Swapping the operands of
/// `unchecked_sub` would be UB in the source program, so that half of the
/// question has no legal producer at all.
#[test]
fn noncomm_arith() {
    run_case("cse_noncomm_arith", include_str!("../cases/case_cse_noncomm_arith.rs"));
}

/// Pinned grid for [`noncomm_arith`], with the (b, a) reflections: `a - b` and
/// `b - a` differ on every unequal pair, so a wrong merge changes the value on
/// every row but the equal ones.
#[test]
fn noncomm_arith_edges() {
    run_case_with_inputs(
        "cse_noncomm_arith",
        include_str!("../cases/case_cse_noncomm_arith.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x0000_0055, 0x0000_00aa),
            (0x00ff_00ff, 0x0001_0001),
            (0x0001_0001, 0x00ff_00ff),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (1, u32::MAX),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W2 oracle for the division family: `arith.div` / `sdiv` / `mod` / `smod`
/// with swapped operands must NOT merge.
///
/// CSE evidence: `arith.div` 2 -> 2 in `udiv_swap`, `arith.mod` 2 -> 2 in
/// `umod_swap`, and the same in the two signed helpers, each with `hir.load`
/// 4 -> 2 underneath — the reloads merged, the divisions did not.
///
/// `arith.sdiv` and `arith.smod` have no plain-Rust producer: the frontend
/// maps `I32DivS` to `builder.div` (on signed operand types) and `I32RemS` to
/// the `wasm.i32_rem_s` expansion, and no dump in this corpus contains either
/// op. Neither is `Commutative` anyway.
///
/// Divisors are runtime values in the non-zero band `(byte & 7) + 1`, and the
/// signed dividend is a small negative range, so neither division by zero nor
/// `MIN / -1` is reachable.
#[test]
fn noncomm_divrem() {
    run_case("cse_noncomm_divrem", include_str!("../cases/case_cse_noncomm_divrem.rs"));
}

/// Pinned grid for [`noncomm_divrem`] with the reflections: the band is
/// `(byte & 7) + 1`, so these rows pin the quotient pairs (1,8), (8,1), (1,1),
/// (8,8), (3,5) and (5,3) — every one of which has `a / b != b / a`.
#[test]
fn noncomm_divrem_edges() {
    run_case_with_inputs(
        "cse_noncomm_divrem",
        include_str!("../cases/case_cse_noncomm_divrem.rs"),
        &[
            (0, 0),
            (0x0000_0007, 0x0000_0000),
            (0x0000_0000, 0x0000_0007),
            (0x0000_0007, 0x0000_0007),
            (0x0000_0002, 0x0000_0004),
            (0x0000_0004, 0x0000_0002),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W2 oracle for the ordering comparisons: `arith.lt` / `lte` / `gt` / `gte`
/// with swapped operands must NOT merge, at both signednesses.
///
/// CSE evidence, one op per helper so the counts are unambiguous:
/// `arith.lt` 2 -> 2, `arith.lte` 2 -> 2, `arith.gt` 2 -> 2 and
/// `arith.gte` 2 -> 2 in the four unsigned helpers, each with `hir.load`
/// 4 -> 2 underneath; `arith.lt` 2 -> 2 and `arith.gte` 2 -> 2 in the signed
/// pair, with `wasm.i32_load_8s` 4 -> 2. The operands provably became one
/// value and the ops still did not match, so the non-merge is the op's doing.
/// Only `Eq` and `Neq` carry `Commutative`; the four orderings do not.
#[test]
fn noncomm_cmp() {
    run_case("cse_noncomm_cmp", include_str!("../cases/case_cse_noncomm_cmp.rs"));
}

/// Pinned grid for [`noncomm_cmp`] with the reflections: the `<`/`<=` pair
/// disagrees exactly on the equal rows and the `(a, b)` / `(b, a)` rows are
/// what a wrong merge would collapse; the signed helpers read the same bytes
/// as `i8`, so 0x80 and 0xff pin the sign boundary.
#[test]
fn noncomm_cmp_edges() {
    run_case_with_inputs(
        "cse_noncomm_cmp",
        include_str!("../cases/case_cse_noncomm_cmp.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x0000_0055, 0x0000_00aa),
            (0x0080_0080, 0x0000_007f),
            (0x0000_007f, 0x0080_0080),
            (u32::MAX, 1),
            (1, u32::MAX),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W3 multiset corners.
///
/// `equal_but_distinct`: `a * b` against `c * a` where `c` is a load of a
/// DIFFERENT address that the entrypoint keeps byte-equal to `b`. The key is
/// SSA identity (`DefaultValueHasher` / `DefaultValueEquivalence`), so nothing
/// merges — `arith.mul` 2 -> 2 with `hir.load` 4 -> 3 (only the two reads of
/// `p` merge; the read of `q` stays its own value).
///
/// `three_plus_one`: three `*` over the pair {a, b} in mixed orders plus one
/// over {a, a} — `arith.mul` 4 -> 2 with `hir.load` 8 -> 2: the three collapse
/// to one and the {a, a} op stays, so [h(a), h(a)] does not match
/// [h(a), h(b)]. The ops have to be `*`: LLVM rewrites `a + a` to `a << 1`, so
/// an `arith.add` with two equal operands never reaches HIR.
///
/// The POSITIVE half of the {a, a} corner — two `a * a` merging with each
/// other — has no producer, and the attempt is worth recording: duplicating
/// one value on the wasm stack needs a `local.tee`, and giving each operand
/// its own volatile load makes LLVM park the bytes in locals instead. Either
/// way the resulting `hir.store_local` is a Write that blocks the reload
/// merge the shape depends on.
#[test]
fn multiset() {
    run_case("cse_multiset", include_str!("../cases/case_cse_multiset.rs"));
}

/// Pinned grid for [`multiset`]: each probe byte is written to two cells, so
/// `equal_but_distinct` always sees operands that agree at runtime and are
/// distinct in SSA; the rows pin the byte pairs (0,0), (x,x), (x,!x), (255,1)
/// and (128,128), where a wrong `a * b` / `c * a` merge would change `s2` only
/// when `a != b`.
#[test]
fn multiset_edges() {
    run_case_with_inputs(
        "cse_multiset",
        include_str!("../cases/case_cse_multiset.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x00ff_00ff, 0x0001_0001),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W3: a commutative op whose operands are themselves commutative results in
/// swapped order — `(a + b) * (c ^ d)` against `(d ^ c) * (b + a)`, with the
/// swapped copy placed AFTER the original because CSE visits a block front to
/// back. The outer `arith.mul` can only match once the inner `arith.add` and
/// `arith.bxor` have merged.
///
/// CSE evidence: `hir.load` 8 -> 4, `arith.add` 8 -> 4 (four of those are
/// address adds, so the two probe adds became one), `arith.bxor` 2 -> 1 and
/// `arith.mul` 2 -> 1 — the outer merge happens in the SAME pass run, because
/// `simplify_block` replaces an operand in place and the later op's key is
/// computed after its operands were rewritten.
#[test]
fn nested_swap() {
    run_case("cse_nested_swap", include_str!("../cases/case_cse_nested_swap.rs"));
}

/// Pinned grid for [`nested_swap`]: four probe bytes per call, so these rows
/// pin all-zero, all-ones, the alternating patterns, the sign-boundary byte
/// and two mixed ones — the outer product is what a wrong inner merge moves.
#[test]
fn nested_swap_edges() {
    run_case_with_inputs(
        "cse_nested_swap",
        include_str!("../cases/case_cse_nested_swap.rs"),
        &[
            (0, 0),
            (u32::MAX, u32::MAX),
            (0xaaaa_aaaa, 0x5555_5555),
            (0x8080_8080, 0x8080_8080),
            (0x00ff_00ff, 0x0001_0001),
            (1, u32::MAX),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W3: the same swapped-operand shape with an OPAQUE WRITE between the two
/// candidates. The reloads must not merge across the write, so the two
/// commutative ops must not merge either — and here a wrong merge is a real
/// miscompile, because the write changes the bytes between the two reads.
///
/// CSE evidence: in `call_barrier` the `hir.exec` of the `#[inline(never)]`
/// poke sits between the two load pairs and `hir.load` stays 4 -> 4 with
/// `arith.add` 4 -> 4; in `store_barrier` the barrier is a `hir.store` and
/// `hir.load` 4 -> 4 with `arith.band` 6 -> 6; `far_barrier` writes a byte
/// NEITHER read touches and still blocks the merge (`hir.load` 4 -> 4,
/// `arith.bxor` 2 -> 2) — CSE has no alias analysis, and
/// `has_other_side_effecting_op_in_between` stops at the first Write of any
/// address.
#[test]
fn reload_write() {
    run_case("cse_reload_write", include_str!("../cases/case_cse_reload_write.rs"));
}

/// Pinned grid for [`reload_write`]: the poked byte equals the probe byte
/// (`input1 >> 8`), so these rows put the write value on top of, below, and
/// far from the two reads — including the rows where the write makes the two
/// sums differ, which is what a wrong merge would hide.
#[test]
fn reload_write_edges() {
    run_case_with_inputs(
        "cse_reload_write",
        include_str!("../cases/case_cse_reload_write.rs"),
        &[
            (0, 0),
            (0x00ff_00ff, 0x00ff_00ff),
            (0x0000_00aa, 0x0000_0055),
            (0x00ff_00ff, 0x0001_0001),
            (0x0080_0080, 0x0080_0080),
            (u32::MAX, 1),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// W4: reverse-order erasure of a region-bearing op.
///
/// The producer is the cfg-to-scf exit-dispatch cascade: a loop with an EMPTY
/// `continue` arm leaves a result column with no consumer, so
/// `WhileUnusedResult` and `IndexSwitchRemoveUnusedResults` rebuild the region
/// op without that column and ERASE the original — regions, blocks and nested
/// definitions and all, through `Rewriter::erase_op`'s `erase_tree`, which
/// since fab7b7db0 walks each nested block's ops back to front so users go
/// before their definitions.
///
/// FINDING (tooling, not codegen): this case cannot be traced with
/// `MIDENC_TRACE='rewriter=trace'`. The compile then panics at
/// `hir/src/program_point.rs:486:63` with
/// `AliasingViolationError { kind: Immutable, location: dialects/scf/src/
/// canonicalization/if_remove_unused_results.rs:86:32 }`, i.e. the
/// `TracingRewriterListener` borrows an operation that
/// `IfRemoveUnusedResults` already holds mutably. It is the trace alone:
/// `midenc --release` on the same wasm exits 0, and so does the same run with
/// `MIDENC_TRACE='pass:canonicalizer=trace'` and
/// `-Z print-ir-after-pass=canonicalizer`. The case therefore stays
/// un-ignored (it compiles and its values match on every configuration); the
/// erase evidence below was taken on `dead_outer`, which traces cleanly.
#[test]
fn dead_region() {
    run_case("cse_dead_region", include_str!("../cases/case_cse_dead_region.rs"));
}

/// Pinned grid for [`dead_region`]: `input2`'s low eight bits decide which
/// trips take the empty `continue` arm in loops 1 and 2, so these rows cover
/// all-continue, never-continue, the two alternating patterns and two mixed
/// ones — one value per continuation mix through all three loops.
#[test]
fn dead_region_edges() {
    run_case_with_inputs(
        "cse_dead_region",
        include_str!("../cases/case_cse_dead_region.rs"),
        &[
            (0, 0),
            (0, u32::MAX),
            (1, 0xaaaa_aaaa),
            (u32::MAX, 0x5555_5555),
            (0x1234_5678, 0xdead_beef),
            (0x8000_0000, 0x0000_00ff),
        ],
    );
}

/// W4 second shape: the erased region op's nested ops use values defined in
/// the same block BEFORE it (`base` and `salt`), and both are still used after
/// the loops. `erase_tree` must drop only the nested uses, so an over-eager
/// erase shows up as a wrong value rather than a panic.
///
/// Evidence (`-Z print-ir-after-pass=canonicalizer` with
/// `MIDENC_TRACE='pass:canonicalizer=trace,rewriter=trace'`): the post-lift
/// canonicalizer prints 7 `erased op scf.if`, 2 `erased op scf.while` and 16
/// `erased op scf.yield` lines. The `erase_tree` order is visible around each
/// `convert-trivial-if-to-select` rewrite — `erased op scf.yield`,
/// `erased ^block31`, `erased op scf.yield`, `erased ^block52`, then
/// `erased op scf.if` — nested ops before their blocks, blocks in post-order,
/// the region op last. The `while-remove-unused-args` erases go the other way
/// round: the pattern MOVES the body ops into the replacement first, so the
/// `scf.while` it erases has empty regions by then. `%base` and `%salt` keep
/// their users in the enclosing block through all of it.
#[test]
fn dead_outer() {
    run_case("cse_dead_outer", include_str!("../cases/case_cse_dead_outer.rs"));
}

/// Pinned grid for [`dead_outer`]: both loops' `continue` arms are driven by
/// input bits, and the final value adds `base` and subtracts `salt`, so each
/// row checks the captured definitions survived under a different mix.
#[test]
fn dead_outer_edges() {
    run_case_with_inputs(
        "cse_dead_outer",
        include_str!("../cases/case_cse_dead_outer.rs"),
        &[
            (0, 0),
            (0, u32::MAX),
            (u32::MAX, 0),
            (0xaaaa_aaaa, 0x5555_5555),
            (0x1234_5678, 0xdead_beef),
            (0x8000_0000, 0x8000_0000),
        ],
    );
}
