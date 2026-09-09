//! Program-scale composites (campaign 17, 2026-09-03): realistic `no_std`
//! programs — the kind a Miden user would compile — each folding its whole
//! state into the result, so that long-range interactions between passes
//! (spills x lifting x memory x arithmetic over hundreds of ops) run
//! differentially. Every program has a pinned `_edges` twin and was
//! deep-fuzzed with `FUZZA_INPUT_PAIRS=512` before being kept.
//!
//! Campaign 21 (2026-09-09) adds a second family, written the same way but
//! aimed at the structured-control-flow shapes campaign 20 measured the spill
//! freight cliff on (`interact.rs`): return-heavy search loops nested in an
//! outer loop, in-loop wide dispatch over u64 bookkeeping words, asymmetric
//! diamonds in hot loops, two-pass algorithms sharing their shift constants,
//! three-level nests whose deepest arm is the only consumer of the
//! accumulated words, and a chain of early-`break` scans as the control. Each
//! carries the state and the constants an algorithm of its kind really has —
//! and seven of the twelve fail to COMPILE at the default configuration, so
//! they are kept as `#[ignore]`d user-impact evidence next to the largest
//! variant of the same program that does compile (`prog_*_guard`).

use super::super::harness::{run_case, run_case_with_flags, run_case_with_inputs};

/// SHA-256 compression: the real 64-word message schedule and 64 rounds
/// (K in `.rodata`) over the padded single-block message of the two inputs
/// and a second digest-derived block at an input-derived byte rotation —
/// eight working variables live through the round loop, a 64-word stack
/// schedule, big-endian byte assembly, both digests XOR-folded.
#[test]
fn prog_sha256() {
    run_case("prog_sha256", include_str!("../cases/case_prog_sha256.rs"));
}

/// Pinned inputs for `prog_sha256`: zero and all-ones blocks, both rotation
/// boundaries (`input2 % 61` at 0 and 60), the equal pair and mixed seeds.
#[test]
fn prog_sha256_edges() {
    run_case_with_inputs(
        "prog_sha256_edges",
        include_str!("../cases/case_prog_sha256.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
            (5, 60),
            (61, 122),
            (0x1234_5678, 0xdead_beef),
            (0x8000_0000, 0x7fff_ffff),
            (1, 1),
        ],
    );
}

/// Keccak-f[800] sponge: 25 u32 lanes, 22 rounds of theta / rho-pi / chi /
/// iota with `.rodata` round-constant, rotation and lane-permutation
/// tables, absorbing two input-derived 16-lane blocks (runtime message
/// length 1..=16 lanes) and squeezing three times; the whole state folded.
#[test]
fn prog_keccak800() {
    run_case("prog_keccak800", include_str!("../cases/case_prog_keccak800.rs"));
}

/// Pinned inputs for `prog_keccak800`: both message-length boundaries
/// (`input2 % 16` at 0 and 15), zero / all-ones / equal pairs and seeds.
#[test]
fn prog_keccak800_edges() {
    run_case_with_inputs(
        "prog_keccak800_edges",
        include_str!("../cases/case_prog_keccak800.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x55aa_55aa, 0x55aa_55aa),
            (0x55aa_55aa, 15),
            (7, 0x7fff_ffff),
            (0x8000_0000, 16),
            (1, 1),
        ],
    );
}

/// Hash mixers: MurmurHash3-x86-32, xxHash32 and FNV-1a over a 64-byte
/// xorshift-filled stack buffer, hashing a runtime-length prefix (17..=64,
/// every tail length) and a runtime-offset window (unaligned 4-byte reads);
/// three digests and a repeat-equality flag folded.
#[test]
fn prog_hashmix() {
    run_case("prog_hashmix", include_str!("../cases/case_prog_hashmix.rs"));
}

/// Pinned inputs for `prog_hashmix`: prefix length 17 and 64, every tail
/// length, the largest window offset (`input1 % 13 == 12`), zero / all-ones
/// / equal pairs.
#[test]
fn prog_hashmix_edges() {
    run_case_with_inputs(
        "prog_hashmix_edges",
        include_str!("../cases/case_prog_hashmix.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xdead_beef, 0xdead_beef),
            (3, 47),
            (0xdead_beef, 46),
            (12, 45),
            (12, 0),
            (25, 44),
        ],
    );
}

/// Bytecode interpreter: an 18-opcode stack VM (dense `match` dispatch in
/// the fetch loop) executing one of four `.rodata` program images selected
/// by the inputs, which also seed the operand stack and the VM memory —
/// counted loops, subroutine calls through a return stack, stack shuffles,
/// indexed memory walks, fault codes and a step budget, all folded.
#[test]
fn prog_stack_vm() {
    run_case("prog_stack_vm", include_str!("../cases/case_prog_stack_vm.rs"));
}

/// Pinned inputs for `prog_stack_vm`: each program with zero, few and full
/// trip counts, the step-budget fault, an immediate `mem_walk` stop
/// (`input2 % 16 == 15`), zero / all-ones / equal pairs.
#[test]
fn prog_stack_vm_edges() {
    run_case_with_inputs(
        "prog_stack_vm_edges",
        include_str!("../cases/case_prog_stack_vm.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5679, 0x1234_5679),
            (1, 2),
            (2, 3),
            (3, 15),
            (156, 5),
            (41, 7),
            (82, 9),
            (15, 0),
        ],
    );
}

/// 256-bit big-integer arithmetic on 8 x u32 limbs: carry / borrow chains,
/// a schoolbook 512-bit product through u64 products cross-checked against
/// the same product on 4 x u64 limbs with u128 multiply-accumulate chains,
/// long division by a runtime divisor multiplied back, and a Montgomery
/// reduction modulo the secp256k1 prime with a Newton-computed inverse
/// limb; self-check flags and every intermediate limb folded.
#[test]
fn prog_bignum() {
    run_case("prog_bignum", include_str!("../cases/case_prog_bignum.rs"));
}

/// Pinned inputs for `prog_bignum`: divisor 1 and `u32::MAX`, the near-
/// modulus operand and the all-ones operand (odd inputs), random limbs
/// (even inputs), zero / all-ones / equal pairs.
#[test]
fn prog_bignum_edges() {
    run_case_with_inputs(
        "prog_bignum_edges",
        include_str!("../cases/case_prog_bignum.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xdead_beef, 0xdead_beef),
            (1, 1),
            (2, 4),
            (0x8000_0000, 0x7fff_ffff),
            (0x0100_0000, 3),
        ],
    );
}

/// Table-driven decoder: a canonical Huffman lookahead table built at
/// runtime from `.rodata` code lengths decodes an input-derived bitstream
/// into LZ tokens (nibble / byte literals, disjoint `copy_within`
/// back-references, xor-delta byte-loop copies, skips, `.rodata`
/// dictionary copies) written into a 256-byte stack buffer; the buffer,
/// symbol histogram and decoder registers hashed.
#[test]
fn prog_decoder() {
    run_case("prog_decoder", include_str!("../cases/case_prog_decoder.rs"));
}

/// Pinned inputs for `prog_decoder`: zero / all-ones / equal pairs and
/// mixed seeds reaching every token kind.
#[test]
fn prog_decoder_edges() {
    run_case_with_inputs(
        "prog_decoder_edges",
        include_str!("../cases/case_prog_decoder.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x5bd1_e995, 0x5bd1_e995),
            (7, 0x0f0f_0f0f),
            (0x1234_5678, 0x8765_4321),
            (100, 200),
            (0x8000_0000, 0x7fff_ffff),
        ],
    );
}

/// Sorting and searching: a `[u32; 64]` xorshift-filled stack array
/// (with planted duplicates) sorted by insertion sort, heap sort and a
/// bottom-up merge sort alternating with a scratch buffer, then sortedness
/// and pairwise-equality checks, lower-bound binary searches for a present
/// and an absent key, a linear rank cross-checked against the search; the
/// sorted array and every flag folded.
#[test]
fn prog_sorts() {
    run_case("prog_sorts", include_str!("../cases/case_prog_sorts.rs"));
}

/// Pinned inputs for `prog_sorts`: the last key index (`input2 % 64 ==
/// 63`), the smallest and largest rank keys, zero / all-ones / equal pairs.
#[test]
fn prog_sorts_edges() {
    run_case_with_inputs(
        "prog_sorts_edges",
        include_str!("../cases/case_prog_sorts.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xabcd_ef01, 0xabcd_ef01),
            (1, 63),
            (7, 0),
            (0x8000_0000, 0x7fff_ffff),
            (12345, 678),
        ],
    );
}

/// Fixed-point DSP: Q16.16 multiply / divide chains (i64 products and
/// dividends, rounding, saturation, zero guards), a Newton reciprocal, a
/// Newton square root cross-checked against a digit-by-digit 64-bit
/// integer square root, a 16-tap FIR with an i64 accumulator over a
/// 48-sample signal, and a 16-step vectoring CORDIC atan2 / magnitude in
/// every quadrant; every result and check flag folded. Passes at the
/// default level and at `--optimize=size-min`; at `--optimize=max` it is
/// the `prog_fixedpoint_o3` compile panic below.
#[test]
fn prog_fixedpoint() {
    run_case("prog_fixedpoint", include_str!("../cases/case_prog_fixedpoint.rs"));
}

/// Pinned inputs for `prog_fixedpoint`: every CORDIC quadrant and both
/// axes (sign boundaries of both inputs), the zero radicand, the smallest
/// and largest reciprocal operands, equal pairs.
#[test]
fn prog_fixedpoint_edges() {
    run_case_with_inputs(
        "prog_fixedpoint_edges",
        include_str!("../cases/case_prog_fixedpoint.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0000, 0x8000_0000),
            (0, 0x8000_0000),
            (0x7fff_ffff, 0),
            (0x8000_0000, 0x7fff_ffff),
            (0xffff, 16),
            (0x1234_5678, 0x8765_4321),
        ],
    );
}

/// Tokenizer + shunting-yard parser + arena AST + two evaluators: one of
/// eight `.rodata` infix expressions (numbers, a variable, parentheses,
/// eight operators over five precedence levels) is tokenized, converted
/// to postfix with an operator stack, built into an index-linked stack
/// arena and evaluated in arena order and by an explicit-stack post-order
/// walk with zero-guarded i32 division / remainder; results, counts and
/// hashes of the postfix buffer and the arena folded.
#[test]
fn prog_parser() {
    run_case("prog_parser", include_str!("../cases/case_prog_parser.rs"));
}

/// Pinned inputs for `prog_parser`: every expression (`input1 % 8`) with
/// the variable at 0 / MIN / -1 / MAX (division by zero, `MIN % 7`,
/// wrapping products), zero / all-ones / equal pairs.
#[test]
fn prog_parser_edges() {
    run_case_with_inputs(
        "prog_parser_edges",
        include_str!("../cases/case_prog_parser.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
            (1, 2),
            (2, 0x8000_0000),
            (3, 5),
            (4, 0xffff_ffff),
            (5, 0x8000_0000),
            (6, 0x8000_0000),
            (7, 0x7fff_ffff),
        ],
    );
}

/// Shortest paths: an O(n^2) Dijkstra over a 32-node `.rodata` adjacency
/// matrix (built by a `const fn` LCG) with input-derived edge weights from
/// an input-selected source, distance / visited / predecessor arrays on
/// the stack, a predecessor walk back from an input-selected target and a
/// second run from the other input's source; distances, the path hash,
/// hop and settled counts folded.
#[test]
fn prog_dijkstra() {
    run_case("prog_dijkstra", include_str!("../cases/case_prog_dijkstra.rs"));
}

/// Pinned inputs for `prog_dijkstra`: source == target (zero hops), the
/// last node as source and as target, zero / all-ones / equal pairs.
#[test]
fn prog_dijkstra_edges() {
    run_case_with_inputs(
        "prog_dijkstra_edges",
        include_str!("../cases/case_prog_dijkstra.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
            (31, 0xffff_ffff),
            (0x3ff, 0),
            (37, 0xffff),
            (549, 7),
        ],
    );
}

/// Sequence-alignment DPs: Levenshtein (both argument orders), LCS and
/// Needleman-Wunsch (signed scores) as two-row stack DPs over two
/// four-letter strings of input-derived lengths 0..=32, a Hamming distance
/// on the common prefix and relation flags; distances, the last DP row
/// and both strings folded.
#[test]
fn prog_editdist() {
    run_case("prog_editdist", include_str!("../cases/case_prog_editdist.rs"));
}

/// Pinned inputs for `prog_editdist`: both strings empty, one empty, both
/// full-length, identical strings (equal pairs), zero / all-ones pairs.
#[test]
fn prog_editdist_edges() {
    run_case_with_inputs(
        "prog_editdist_edges",
        include_str!("../cases/case_prog_editdist.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (1, 1),
            (32, 32),
            (32, 0),
            (0, 32),
            (0x8000_0000, 0x7fff_ffff),
            (16, 31),
        ],
    );
}

/// Checksums and shift registers: table-driven CRC-32 (`const fn` `.rodata`
/// table) cross-checked against a bitwise CRC-32, CRC-16/CCITT, a CRC-8
/// through a runtime-built stack table with an input-selected polynomial,
/// Adler-32, a 32-bit Galois LFSR and a 16-bit Fibonacci LFSR over a
/// runtime-length (0..=64) xorshift buffer; every checksum and generator
/// state folded.
#[test]
fn prog_crc() {
    run_case("prog_crc", include_str!("../cases/case_prog_crc.rs"));
}

/// Pinned inputs for `prog_crc`: length 0, 1 and 64, every CRC-8
/// polynomial variant, the longest LFSR run, zero / all-ones / equal pairs.
#[test]
fn prog_crc_edges() {
    run_case_with_inputs(
        "prog_crc_edges",
        include_str!("../cases/case_prog_crc.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1b87_3593, 0x1b87_3593),
            (0x30, 64),
            (0x10, 33),
            (0x20, 5),
            (47, 1),
        ],
    );
}

/// `core::fmt` on the VM: a `fmt::Write` stack buffer receives `write!`
/// of the inputs in decimal, lower / upper / zero-padded hex, signed with
/// an explicit sign, width-padded and aligned (runtime width), alternate
/// binary, octal, `Debug` of a runtime-length byte slice and of a tuple —
/// `Arguments`, `Formatter`, `pad_integral`, `DebugList` and `dyn Write`
/// vtable dispatch — then the decimal / hex / signed fields are parsed back
/// with `str::parse` and `from_str_radix`; text, length, round-trip flags
/// and the overflow status folded.
#[test]
fn prog_fmt() {
    run_case("prog_fmt", include_str!("../cases/case_prog_fmt.rs"));
}

/// Pinned inputs for `prog_fmt`: `i32::MIN` / -1 / MAX signed fields, the
/// widest decimal, the longest `Debug` slice, every runtime width class,
/// zero / all-ones / equal pairs.
#[test]
fn prog_fmt_edges() {
    run_case_with_inputs(
        "prog_fmt_edges",
        include_str!("../cases/case_prog_fmt.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0000, 0x8000_0000),
            (12345, 67890),
            (999, 0x7fff_ffff),
            (255, 11),
            (1, 3),
            (0x1234_5678, 0x8000_0001),
        ],
    );
}

/// Bytecode interpreter with a handler table: the `prog_stack_vm` VM with
/// its state in a struct passed by `&mut` to opcode handlers dispatched
/// through a `.rodata` table of function pointers (one `call_indirect`
/// per step with the receiver as the only argument), `Option`-returning
/// pop / push helpers and closure-parameterized binary operations; the
/// whole VM state is folded.
#[test]
fn prog_vm_table() {
    run_case("prog_vm_table", include_str!("../cases/case_prog_vm_table.rs"));
}

/// Pinned inputs for `prog_vm_table`: each program with zero, few and
/// full trip counts, the step-budget fault, an immediate `mem_walk` stop,
/// zero / all-ones / equal pairs.
#[test]
fn prog_vm_table_edges() {
    run_case_with_inputs(
        "prog_vm_table_edges",
        include_str!("../cases/case_prog_vm_table.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5679, 0x1234_5679),
            (2, 1),
            (3, 2),
            (15, 3),
            (5, 512),
            (7, 257),
            (9, 322),
            (0, 3),
        ],
    );
}

/// Iterator pipelines and non-recursive `core` slice algorithms over a
/// `[u32; 64]` stack array: `rotate_left` by a runtime count, `reverse`,
/// `split_at_mut` + `iter_mut().zip()`, `swap`, `fill`, `copy_from_slice`,
/// a slice heap sort, `binary_search`, `windows`, `chunks_exact`,
/// `rev().enumerate()`, `max_by_key`, `position` / `rposition`, `filter`,
/// `step_by`, `take_while`, `skip`, `cycle().take()`, `nth`, `find`, `min`,
/// `any` and `Option` combinators over `checked_*` chains (`sort_unstable`
/// and `select_nth_unstable` are recursive in `core` and hit the linker's
/// call-graph cycle check — probe deleted).
#[test]
fn prog_iters() {
    run_case("prog_iters", include_str!("../cases/case_prog_iters.rs"));
}

/// Pinned inputs for `prog_iters`: `skip` / `nth` past the end, the
/// overflowing `checked_pow`, the zero-length `cycle`, the last index,
/// zero / all-ones / equal pairs.
#[test]
fn prog_iters_edges() {
    run_case_with_inputs(
        "prog_iters_edges",
        include_str!("../cases/case_prog_iters.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xabcd_ef01, 0xabcd_ef01),
            (69, 199),
            (79, 24),
            (63, 63),
            (0x8000_0000, 0x7fff_ffff),
            (13, 6),
        ],
    );
}

/// CONFIGURATION-DEPENDENT COMPILE-TIME COMPILER PANIC (safe Rust, campaign
/// 17, 2026-09-03): the realistic `prog_fixedpoint` program built with
/// `--optimize=max` (guest opt-level 3; the default opt-level 2 and
/// `--optimize=size-min` pass, and the 512-pair default sweep is clean)
/// panics in the MASM emitter: `invalid operand stack index (11): requires
/// access to more than 16 elements` at codegen/masm/src/stack.rs:540
/// (reported at emit/mod.rs:623) from `OperandStack::dup` <-
/// `OpEmitter::copy_operand_to_position` <- the operand solver <-
/// `StoreLocal::schedule_operands` inside the FIR loop's `scf.while`
/// body: the emitter's physical operand stack holds more than 16 felts
/// within the top twelve operands. Mechanism (`MIDENC_TRACE=
/// 'pass:spills=trace,analysis:spills=trace'`): the spill analysis decides
/// 21 spills / 26 reloads and ONE edge to split — the FIR loop's backedge
/// `end(^block63) -> start(^block9)` — the transform creates the split
/// block, places four reloads of loop-invariant values in it, and then
/// `erase unused reload` removes exactly those four (the SSA
/// reconstruction runs on the analysis-time dominator tree that does not
/// contain the split block) — the known F6 defect (`zero_trip_frontier` /
/// `zero_trip_overflow`, tests/pressure.rs; `dispatch_pressure`'s backedge
/// split in tests/calls.rs) surfacing at a THIRD site: not `frontier.rs:123`
/// and not the solver's `NoSolution`, but the emitter's 16-felt window
/// assert, because the first op that touches the unrelieved stack is an
/// arity-1 `store_local` whose `Copy` solution is applied without a felt
/// budget check. No "unused phi" warning (not F1), no `hir.exec_indirect`
/// (not F11), in-contract input. Panic-only. Minimized reproducer:
/// `fir_cordic_o3` below (the same signature; the ladder is in its doc
/// comment). Un-ignore together with the F6 reproducers.
#[test]
#[ignore = "compiler panic at --optimize=max: 'invalid operand stack index (11): requires access \
            to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 (OperandStack::dup from \
            StoreLocal::schedule_operands) — F6 class: the four reloads on the FIR loop's split \
            backedge are erased as unused; compile-time, no inputs involved"]
fn prog_fixedpoint_o3() {
    run_case_with_flags(
        "prog_fixedpoint_o3",
        include_str!("../cases/case_prog_fixedpoint.rs"),
        &["--optimize=max"],
    );
}

/// The minimized `prog_fixedpoint` shape at the default configuration
/// (opt-level 2, the tap loop kept rolled): an 8-tap Q16.16 FIR with an
/// i64 multiply-accumulate over a 48-sample stack array, rounded and
/// saturated, followed by an 8-iteration vectoring CORDIC — passes.
#[test]
fn fir_cordic() {
    run_case("fir_cordic", include_str!("../cases/case_fir_cordic.rs"));
}

/// COMPILE-TIME COMPILER PANIC at `--optimize=max` — the minimized form of
/// `prog_fixedpoint_o3` (same signature, `invalid operand stack index (11)`
/// at emit/mod.rs:623 from `StoreLocal::schedule_operands` in the FIR
/// loop body). Ingredients, each necessary (ladder of 24 generated rungs,
/// `scratch/c17gen_fp.py`): the tap loop unrolled by LLVM at opt-level 3
/// into ONE block of i64 multiply-accumulates over a runtime-indexed
/// stack array (>= 8 taps; 6 pass, 4 pass), a saturating rounded output
/// (`sat32((acc + (1 << 15)) >> 16)`: the plain `(acc >> 16) as i32`
/// passes, an i32 accumulator passes), the samples in a stack ARRAY
/// (computing each sample inline passes), and a following CORDIC of
/// enough iterations (8 taps: 8 iterations panic, 4 pass; 16 taps: 2
/// panic, 1 passes; the `.rodata` angle table is not needed). The
/// loop-invariant values shared by the array fill, the loop and the
/// CORDIC are spilled at the loop header, reloaded on the split backedge,
/// and the split-block reloads are erased (F6) — leaving the loop body
/// over the 16-felt window. Bounded by `fir_cordic_guard_o3` (8 taps, 4
/// iterations, passes at max) and `fir_cordic` (this file at the default
/// level, passes; 512-pair-clean sibling `prog_fixedpoint`). Compile-time
/// — no inputs involved. Un-ignore together with the F6 reproducers.
#[test]
#[ignore = "compiler panic at --optimize=max: 'invalid operand stack index (11): requires access \
            to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 — F6 class (erased \
            split-backedge reloads) at the emitter's window assert; compile-time, no inputs \
            involved"]
fn fir_cordic_o3() {
    run_case_with_flags(
        "fir_cordic_o3",
        include_str!("../cases/case_fir_cordic.rs"),
        &["--optimize=max"],
    );
}

/// Passing `--optimize=max` guard for `fir_cordic_o3`: the same 8-tap FIR
/// with a 4-iteration CORDIC compiles and matches native at max.
#[test]
fn fir_cordic_guard_o3() {
    run_case_with_flags(
        "fir_cordic_guard_o3",
        include_str!("../cases/case_fir_cordic_guard.rs"),
        &["--optimize=max"],
    );
}

// --------------------------------------------------------------------------
// Campaign 21 (2026-09-09): realistic programs written for the freight-cliff
// shapes. Per program: the cliff shape, the u64 state count, the number of
// shared rotate/shift constants, and the outcome at the default level,
// `--optimize=size-min`, `--optimize=max` and `--optimize=basic`.
// --------------------------------------------------------------------------

/// UTF-8 validator (cliff shape: return-heavy inner loop nested in an outer
/// loop; 5 u64 statistics, 4 shared rotate constants): two 40-byte buffers of
/// encoded code points are validated in an outer pass loop, and the
/// per-code-point inner loop returns a distinct error class for a bad leading
/// byte, a truncated sequence, a bad continuation byte and a surrogate /
/// out-of-range code point, each of which combines all five statistics — a
/// code-point checksum, a width fold, a running FNV hash, a maximum and a
/// class mask — in one expression. Passes at the default level, at
/// `--optimize=size-min`, `max` and `basic`.
#[test]
fn prog_utf8() {
    run_case("prog_utf8", include_str!("../cases/case_prog_utf8.rs"));
}

/// Per-error-class pinned grid for [`prog_utf8`]: `input2`'s low bits pick the
/// planted malformation, so these pairs pin the bad leading byte (tag 1), the
/// truncated tail (2), the bad continuation byte (3), the surrogate (4) and
/// the all-valid stream (5), plus zero / all-ones / equal pairs.
#[test]
fn prog_utf8_edges() {
    run_case_with_inputs(
        "prog_utf8_edges",
        include_str!("../cases/case_prog_utf8.rs"),
        &[
            (0, 1),
            (0, 6),
            (0, 2),
            (0, 3),
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
        ],
    );
}

/// Glob matcher (cliff shape: return-heavy inner loop nested in an outer loop;
/// 5 u64 statistics, 4 shared rotate constants): three patterns with `*`, `?`
/// and `[a-c]` classes are matched against a 32-byte text by the classic
/// backtracking loop, which returns early on an exhausted step budget, a
/// malformed class, a text that runs out and a match — every exit combining
/// the position hash, star mask, class fold, wildcard count and step checksum.
/// Passes at all four optimization levels.
#[test]
fn prog_glob() {
    run_case("prog_glob", include_str!("../cases/case_prog_glob.rs"));
}

/// Per-exit pinned grid for [`prog_glob`]: the budget exit (tag 1), the
/// malformed class (2), the exhausted text (3), the match (4) and the
/// no-match fallthrough (5), plus zero / all-ones / equal pairs.
#[test]
fn prog_glob_edges() {
    run_case_with_inputs(
        "prog_glob_edges",
        include_str!("../cases/case_prog_glob.rs"),
        &[
            (0, 8),
            (0, 1),
            (0, 18),
            (0, 3),
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
        ],
    );
}

/// Bytecode interpreter with 64-bit bookkeeping (cliff shape: in-loop wide
/// dispatch; 6 u64 bookkeeping words, 6 shared rotate constants): a 20-opcode
/// stack machine runs one of three `.rodata` images, dispatching on the opcode
/// with a dense `match` in which four opcodes share the default body and the
/// opcodes 20..31 are invalid (the images contain none), and every arm updates
/// the same state hash, checksum, gas counter, flag mask, high-water mark and
/// trace fold, which a per-step commitment expression then combines. Passes at
/// all four optimization levels — the dispatch shape survives what the same
/// state does not survive in `prog_sponge` / `prog_histogram`.
#[test]
fn prog_bcvm64() {
    run_case("prog_bcvm64", include_str!("../cases/case_prog_bcvm64.rs"));
}

/// Pinned grid for [`prog_bcvm64`]: each `.rodata` image (`input1 % 3`), the
/// smallest and largest step budgets (`input2 % 40`), the opcode-rotation
/// extremes (`input2 >> 28`), the gas-exhaustion break, zero / all-ones /
/// equal pairs.
#[test]
fn prog_bcvm64_edges() {
    run_case_with_inputs(
        "prog_bcvm64_edges",
        include_str!("../cases/case_prog_bcvm64.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 39),
            (4, 0x8000_0000),
            (5, 79),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
        ],
    );
}

/// Framing state machine (cliff shape: in-loop wide dispatch; 8 u64
/// accumulators, 6 shared rotate constants): a sixteen-state serial-protocol
/// parser consumes a 48-byte stream one byte at a time, three of its states
/// sharing the resync body, and every state arm updates the same frame hash,
/// payload hash, CRC accumulator, escape fold, byte and frame counters, error
/// mask and timing fold, which a watchdog expression at the bottom of the loop
/// combines. Passes at all four optimization levels.
#[test]
fn prog_states() {
    run_case("prog_states", include_str!("../cases/case_prog_states.rs"));
}

/// Pinned grid for [`prog_states`]: the mode bits plant the payload length,
/// the escape byte (`input2 & 8`) and the bad second header byte
/// (`input2 & 4`), so these pairs pin the clean frame, the escaped frame, the
/// resync path and their combinations, plus zero / all-ones / equal pairs.
#[test]
fn prog_states_edges() {
    run_case_with_inputs(
        "prog_states_edges",
        include_str!("../cases/case_prog_states.rs"),
        &[
            (0, 0),
            (1, 4),
            (2, 8),
            (3, 12),
            (0x5a5a_5a5a, 0x5a5a_5a5a),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
        ],
    );
}

/// Table validation pipeline (cliff shape: chain of early-`break` scans — the
/// most freight-tolerant shape, kept as the control; 6 u64 statistics, 4
/// shared rotate constants): five sequential scans over a 48-entry u64 table
/// (first out-of-range entry, duplicate low word, monotonicity, first entry
/// over the quota, checksum of the accepted prefix) each break as soon as they
/// have their answer and all share the same constants and statistics. Passes
/// at all four optimization levels, as campaign 20's ranking predicts.
#[test]
fn prog_scanchain() {
    run_case("prog_scanchain", include_str!("../cases/case_prog_scanchain.rs"));
}

/// Pinned grid for [`prog_scanchain`]: `input2` sets the range limit (0 makes
/// the first scan break immediately, all-ones lets it run to the end) and
/// `input1` the quota, so these pairs pin break / no-break for each scan, plus
/// zero / all-ones / equal pairs.
#[test]
fn prog_scanchain_edges() {
    run_case_with_inputs(
        "prog_scanchain_edges",
        include_str!("../cases/case_prog_scanchain.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (1, 1),
            (0x9e37_79b9, 0x9e37_79b9),
            (7, 0x7fff_ffff),
            (0x8000_0000, 0x8000_0000),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 21, 2026-09-09). Rabin-Karp substring scanner (cliff shape:
/// return-heavy inner loop nested in an outer loop; 6 u64 fingerprint words,
/// 5 shared rotate constants): three needles are searched for in a 64-byte
/// buffer with a rolling hash, the inner scan loop returning early on a
/// verified match, on an exhausted false-positive budget and on a sentinel
/// byte, with the six-word Bloom probe — the only place all six fingerprint
/// words are live at once — evaluated on every iteration. Building it panics
/// with `invalid operand stack index (9): requires access to more than 16
/// elements` at codegen/masm/src/emit/mod.rs:623 (`OperandStack::dup` from the
/// operand solver). Classification F6, not F2: the spills trace
/// (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`) shows 20 spills,
/// 43 reloads, TWO "edges to split" and 26 `erase unused reload` lines for the
/// split-block reloads (the SSA reconstruction walks the dominator tree cached
/// before the transform's own splits), plus three "unused phi" warnings.
/// Per level: default PANIC (index 9), `--optimize=size-min` PASSES,
/// `--optimize=max` PANIC (index 9), `--optimize=basic` PANIC (index 10).
/// Bounding sibling: `prog_rkscan_guard` — the same scanner with three rotate
/// constants instead of five and four fingerprint words instead of six —
/// compiles and matches native at the default level, at size-min and at max.
/// The ladder in between still panics at the default level: six→four words
/// (index 9), four→three (index 10), three→two (index 9). Five→three rotate
/// constants alone (six words kept) passes at the default level and at
/// size-min but panics at max (frontier.rs:123) and at basic (index 10), so it
/// is the crossing COUNT BANDS, not the word count, that carries this program
/// over the window. Compile-time — no inputs involved. Un-ignore together with
/// the other F6 reproducers (`pressure::zero_trip_frontier`).
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'invalid operand stack index (9): \
            requires access to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 — F6 \
            class (26 erased split-edge reloads in the spills trace); compile-time, no inputs \
            involved"]
fn prog_rkscan() {
    run_case("prog_rkscan", include_str!("../cases/case_prog_rkscan.rs"));
}

/// The largest variant of `prog_rkscan` that compiles at the default level:
/// three distinct rotate constants instead of five and four fingerprint words
/// instead of six. Passes at the default level, `--optimize=size-min` and
/// `--optimize=max`; still panics at `--optimize=basic` (see
/// `prog_rkscan_guard_o1`).
#[test]
fn prog_rkscan_guard() {
    run_case("prog_rkscan_guard", include_str!("../cases/case_prog_rkscan_guard.rs"));
}

/// Per-exit pinned grid for [`prog_rkscan_guard`]: the verified match (tag 1),
/// the false-positive budget (2), the sentinel byte (3) and the exhausted
/// search (4, `input2`'s top bit forcing external-only patterns), plus zero /
/// all-ones / equal pairs.
#[test]
fn prog_rkscan_guard_edges() {
    run_case_with_inputs(
        "prog_rkscan_guard_edges",
        include_str!("../cases/case_prog_rkscan_guard.rs"),
        &[
            (0, 0),
            (0, 4),
            (768, 0),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
            (1, 1),
        ],
    );
}

/// CONFIGURATION-DEPENDENT COMPILE-TIME COMPILER PANIC: the reduced
/// `prog_rkscan_guard` still panics at `--optimize=basic` with `invalid
/// operand stack index (10): requires access to more than 16 elements` at
/// codegen/masm/src/emit/mod.rs:623 — the same F6 site as the full program.
/// Kept so a corpus-wide `--optimize=basic` sweep does not rediscover it as a
/// new finding. Compile-time — no inputs involved.
#[test]
#[ignore = "compiler panic at --optimize=basic: 'invalid operand stack index (10): requires access \
            to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 — F6 class; \
            compile-time, no inputs involved"]
fn prog_rkscan_guard_o1() {
    run_case_with_flags(
        "prog_rkscan_guard_o1",
        include_str!("../cases/case_prog_rkscan_guard.rs"),
        &["--optimize=basic"],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 21, 2026-09-09). LEB128 record decoder (cliff shape: return-heavy
/// inner loop nested in an outer loop; 6 u64 running values, 4 shared shift
/// constants): a 48-byte frame of varints is decoded by a continuation-byte
/// loop with four error returns (truncated frame, 64-bit overflow, overlong
/// encoding, reserved marker), each combining the six running values — sum,
/// xor fold, minimum, maximum, running hash and checksum — in one expression.
/// Building it panics with `AliasingViolationError { kind: Mutable, location:
/// hir/src/ir/operation.rs:877 }` at hir/src/patterns/rewriter.rs:335.
/// Classification F12: the last line the pattern driver logs before the panic
/// (`MIDENC_TRACE='pattern-rewrite-driver=trace'`) is `trying to match
/// 'remove-loop-invariant-args-from-before-block' dialect=scf op=while`. The
/// documented F12 producer was a labeled `continue` over a loop containing a
/// call, at `-Oz` where LLVM stops inlining it; this program has neither a
/// labeled `continue` nor a call in the loop, and fails at the DEFAULT level.
/// Per level: default PANIC, `--optimize=size-min` PANIC, `--optimize=max`
/// PASSES, `--optimize=basic` PANIC. Bounding sibling: `prog_varint_guard` —
/// the same decoder with two running values — compiles at the default level,
/// at max and at basic; four values still panic, and even ONE value still
/// panics at size-min (`prog_varint_guard_oz`), so the state count moves the
/// default-level boundary but not the size-min one. Compile-time — no inputs
/// involved. Un-ignore when the rewriter stops taking a mutable borrow of an
/// operation it is already borrowing.
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'AliasingViolationError { kind: Mutable, \
            location: hir/src/ir/operation.rs:877 }' at hir/src/patterns/rewriter.rs:335 while \
            matching 'remove-loop-invariant-args-from-before-block' — F12 class; compile-time, no \
            inputs involved"]
fn prog_varint() {
    run_case("prog_varint", include_str!("../cases/case_prog_varint.rs"));
}

/// The largest variant of `prog_varint` that compiles at the default level:
/// two running values instead of six. Passes at the default level,
/// `--optimize=max` and `--optimize=basic`; still panics at
/// `--optimize=size-min` (see `prog_varint_guard_oz`).
#[test]
fn prog_varint_guard() {
    run_case("prog_varint_guard", include_str!("../cases/case_prog_varint_guard.rs"));
}

/// Per-error-class pinned grid for [`prog_varint_guard`]: the truncated frame
/// (tag 1), the 64-bit overflow (2), the overlong encoding (3), the reserved
/// marker (4) and the clean frame (5), plus zero / all-ones / equal pairs.
#[test]
fn prog_varint_guard_edges() {
    run_case_with_inputs(
        "prog_varint_guard_edges",
        include_str!("../cases/case_prog_varint_guard.rs"),
        &[
            (0, 0),
            (0, 8),
            (0, 2),
            (30208, 0),
            (0, 1),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
        ],
    );
}

/// CONFIGURATION-DEPENDENT COMPILE-TIME COMPILER PANIC: the reduced
/// `prog_varint_guard` still panics at `--optimize=size-min` in the F12 class
/// (`AliasingViolationError` at hir/src/patterns/rewriter.rs:335 while
/// matching `remove-loop-invariant-args-from-before-block`), and so does a
/// further-reduced variant with a single running value — the size-min failure
/// does not depend on how much u64 state the loop carries. Compile-time — no
/// inputs involved.
#[test]
#[ignore = "compiler panic at --optimize=size-min: 'AliasingViolationError { kind: Mutable, \
            location: hir/src/ir/operation.rs:877 }' at hir/src/patterns/rewriter.rs:335 — F12 \
            class; compile-time, no inputs involved"]
fn prog_varint_guard_oz() {
    run_case_with_flags(
        "prog_varint_guard_oz",
        include_str!("../cases/case_prog_varint_guard.rs"),
        &["--optimize=size-min"],
    );
}

/// COMPILE-TIME COMPILER PANIC AT EVERY OPTIMIZATION LEVEL (safe Rust,
/// campaign 21, 2026-09-09). Conditional-round Feistel mixer (cliff shape:
/// asymmetric diamond in a hot loop; 6 u64 round keys, 4 shared rotate
/// constants): 32 counter blocks are encrypted by a Feistel network whose
/// round function runs only on the iterations the schedule selects, so the
/// then-arm reads all six round keys and the else-arm none — the pressure
/// asymmetry the spill analysis has to reconcile on the join. Building it
/// panics with `failed to schedule operands: [%22, %146] for inst
/// 'arith.rotl' with error: NoSolution, constraints: [Copy, Move]` at
/// codegen/masm/src/lower/lowering.rs:109 over a NINETEEN-felt operand stack
/// (six u64 words and seven u32 count bands). Classification F6, not F2: the
/// stack is over the K = 16 cap the spill analysis is supposed to enforce and
/// the spills trace shows 23 spills, 21 reloads, two "edges to split" and six
/// `erase unused reload` lines; the Copy-constrained operand %22 is one of the
/// count bands stranded at index 11. Per level: default, `--optimize=size-min`,
/// `max` and `basic` ALL panic at the same site. Bounding sibling:
/// `prog_feistel_guard` — the same network with TWO round keys — compiles and
/// matches native at all four levels; four keys still panic at all four.
/// Compile-time — no inputs involved. Un-ignore together with the other F6
/// reproducers.
#[test]
#[ignore = "compiler panic at every optimization level: 'failed to schedule operands ... with \
            error: NoSolution' on 'arith.rotl' [Copy, Move] over a 19-felt operand stack at \
            codegen/masm/src/lower/lowering.rs:109 — F6 class (six erased split-edge reloads); \
            compile-time, no inputs involved"]
fn prog_feistel() {
    run_case("prog_feistel", include_str!("../cases/case_prog_feistel.rs"));
}

/// The largest variant of `prog_feistel` that compiles: two round keys instead
/// of six. Passes at the default level, `--optimize=size-min`, `max` and
/// `basic`.
#[test]
fn prog_feistel_guard() {
    run_case("prog_feistel_guard", include_str!("../cases/case_prog_feistel_guard.rs"));
}

/// Pinned grid for [`prog_feistel_guard`]: the schedule predicate
/// (`((block >> 5) ^ i) % 97 < 48`) decides which iterations take the
/// key-reading arm, so these seeds pin all-active, all-skipped and mixed
/// sequences, plus zero / all-ones / equal pairs.
#[test]
fn prog_feistel_guard_edges() {
    run_case_with_inputs(
        "prog_feistel_guard_edges",
        include_str!("../cases/case_prog_feistel_guard.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (1, 1),
            (0x9e37_79b9, 0x9e37_79b9),
            (0x8000_0000, 0x7fff_ffff),
            (0x1234_5678, 0xdead_beef),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 21, 2026-09-09). Run-length + delta encoder (cliff shape:
/// asymmetric diamond in a hot loop; 5 u64 statistics, 5 shared rotate
/// constants): a 64-sample signal is encoded into a token buffer, the common
/// arm only extending the current run while the escape arm — a literal block
/// that cannot be run-encoded — reads and rewrites all five encoder
/// statistics (dictionary hash, entropy fold, delta accumulator, escape
/// counter and checksum). Building it panics with `AliasingViolationError {
/// kind: Mutable, location: hir/src/ir/operation.rs:877 }` at
/// hir/src/patterns/rewriter.rs:335 — F12, confirmed the same way as
/// `prog_varint` (the driver's last line is `trying to match
/// 'remove-loop-invariant-args-from-before-block' dialect=scf op=while`), and
/// again with no labeled `continue` and no call in the loop. Per level:
/// default PANIC, `--optimize=size-min` PASSES, `--optimize=max` PANIC,
/// `--optimize=basic` PASSES. Bounding sibling: `prog_rle_guard` — the same
/// encoder with three statistics — compiles at all four levels. Compile-time
/// — no inputs involved.
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'AliasingViolationError { kind: Mutable, \
            location: hir/src/ir/operation.rs:877 }' at hir/src/patterns/rewriter.rs:335 while \
            matching 'remove-loop-invariant-args-from-before-block' — F12 class; compile-time, no \
            inputs involved"]
fn prog_rle() {
    run_case("prog_rle", include_str!("../cases/case_prog_rle.rs"));
}

/// The largest variant of `prog_rle` that compiles: three encoder statistics
/// instead of five. Passes at all four optimization levels.
#[test]
fn prog_rle_guard() {
    run_case("prog_rle_guard", include_str!("../cases/case_prog_rle_guard.rs"));
}

/// Pinned grid for [`prog_rle_guard`]: `input2 % 8` controls how run-heavy the
/// signal is, so these pairs pin the all-literal signal (no runs, every
/// iteration takes the escape arm), the run-heavy signal and the mixtures,
/// plus zero / all-ones / equal pairs.
#[test]
fn prog_rle_guard_edges() {
    run_case_with_inputs(
        "prog_rle_guard_edges",
        include_str!("../cases/case_prog_rle_guard.rs"),
        &[
            (0, 0),
            (1, 7),
            (0x1234_5678, 3),
            (5, 6),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xdead_beef, 0xdead_beef),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 21, 2026-09-09). Two-pass entropy-coder front-end (cliff shape:
/// sequential loops sharing constants; 6 u64 statistics, 8 shared shift
/// constants): a histogram pass over a 96-byte buffer builds sixteen buckets
/// and six statistics, and a second pass turns the histogram into cumulative
/// offsets and emits a packed code stream, reusing the SAME shift constants
/// the first pass used for bucket selection — so every constant is live from
/// before the first loop, across it and into the second. Building it panics
/// with `failed to schedule operands: [%1218, %695] for inst 'arith.rotl' with
/// error: NoSolution, constraints: [Copy, Move]` at
/// codegen/masm/src/lower/lowering.rs:109 (F6, the `zero_trip_overflow`
/// signature: the count bands stay live past their spills because the
/// split-edge reloads are erased; spills trace: one split edge, thirty-five
/// `erase unused reload` lines). Per level: default PANIC (lowering.rs:109),
/// `--optimize=size-min` PASSES, `--optimize=max` PANIC (lowering.rs:109),
/// `--optimize=basic` PANIC (`invalid operand stack index (12)` at
/// emit/mod.rs:623). Bounding sibling: `prog_histogram_guard` — the same two
/// passes with SIX distinct shift constants instead of eight — compiles and
/// matches native at all four levels. Compile-time — no inputs involved.
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'failed to schedule operands ... with \
            error: NoSolution' on 'arith.rotl' [Copy, Move] at \
            codegen/masm/src/lower/lowering.rs:109 — F6 class; compile-time, no inputs involved"]
fn prog_histogram() {
    run_case("prog_histogram", include_str!("../cases/case_prog_histogram.rs"));
}

/// The largest variant of `prog_histogram` that compiles: six distinct shift
/// constants instead of eight. Passes at all four optimization levels.
#[test]
fn prog_histogram_guard() {
    run_case("prog_histogram_guard", include_str!("../cases/case_prog_histogram_guard.rs"));
}

/// Pinned grid for [`prog_histogram_guard`]: `input2 % 65` sets the processed
/// length (32 at 0, 96 at 64), so these pairs pin the shortest and longest
/// buffer, plus zero / all-ones / equal pairs.
#[test]
fn prog_histogram_guard_edges() {
    run_case_with_inputs(
        "prog_histogram_guard_edges",
        include_str!("../cases/case_prog_histogram_guard.rs"),
        &[
            (0, 0),
            (1, 64),
            (7, 32),
            (0x1234_5678, 1),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x9e37_79b9, 0x9e37_79b9),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT THE DEFAULT CONFIGURATION (safe Rust,
/// campaign 21, 2026-09-09). Eight-lane sponge hash (cliff shape: sequential
/// loops sharing constants; 8 u64 lanes, 8 shared rotation offsets): an absorb
/// loop XORs message blocks into the rate lanes and runs six permutation
/// rounds (column mix, lane rotations, chi), and a squeeze loop then extracts
/// four output words with the SAME eight rotation offsets — the shape every
/// Keccak-like permutation in Rust has, with no 128-bit arithmetic anywhere.
/// Building it panics with `invalid operand stack index (14): requires access
/// to more than 16 elements` at codegen/masm/src/emit/mod.rs:623 (F6, the
/// `prog_fixedpoint_o3` signature at the DEFAULT level rather than at max;
/// spills trace: four split edges, eighteen `erase unused reload` lines).
/// Per level: default PANIC (index 14), `--optimize=size-min` PASSES,
/// `--optimize=max` PANIC (`NoSolution` on `arith.rotl` at lowering.rs:109),
/// `--optimize=basic` PANIC (index 15). Bounding sibling:
/// `prog_sponge_guard` — the same eight-lane sponge with FOUR distinct
/// rotation offsets instead of eight — compiles and matches native at all four
/// levels, so it is the number of distinct rotation constants, not the number
/// of lanes, that decides. Compile-time — no inputs involved.
#[test]
#[ignore = "compiler panic at the DEFAULT configuration: 'invalid operand stack index (14): \
            requires access to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 — F6 \
            class; compile-time, no inputs involved"]
fn prog_sponge() {
    run_case("prog_sponge", include_str!("../cases/case_prog_sponge.rs"));
}

/// The largest variant of `prog_sponge` that compiles: four distinct rotation
/// offsets instead of eight, on the same eight lanes. Passes at all four
/// optimization levels.
#[test]
fn prog_sponge_guard() {
    run_case("prog_sponge_guard", include_str!("../cases/case_prog_sponge_guard.rs"));
}

/// Pinned grid for [`prog_sponge_guard`]: `1 + input2 % 12` is the number of
/// absorbed blocks, so these pairs pin the single-block and the twelve-block
/// sponge, plus zero / all-ones / equal pairs.
#[test]
fn prog_sponge_guard_edges() {
    run_case_with_inputs(
        "prog_sponge_guard_edges",
        include_str!("../cases/case_prog_sponge_guard.rs"),
        &[
            (0, 0),
            (1, 11),
            (0x9e37_79b9, 5),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
            (7, 1),
        ],
    );
}

/// COMPILE-TIME COMPILER PANIC AT EVERY OPTIMIZATION LEVEL (safe Rust,
/// campaign 21, 2026-09-09). Nested TLV record validator (cliff shape:
/// three-level diamond nest whose deepest arm is the only consumer of the
/// accumulated words; 8 u64 digest words, 4 shared rotate constants): a
/// 64-byte container is parsed as tag / length / value records behind three
/// nested checks — container magic, record header, value type — and only the
/// innermost accepted path reads the eight digest words computed before the
/// loop. Building it panics with `called Option::unwrap() on a None value` at
/// hir/src/ir/dominance/frontier.rs:123 (`DominanceFrontier::new` from
/// `spill::rewrite_cfg_spills`) — the F6 flavor `pressure::zero_trip_frontier`
/// documents, here with no zero-trip-capable loop anywhere in the program and
/// at EVERY optimization level (default, size-min, max, basic); the spills
/// trace reports nine edges to split before the unwrap. Bounding
/// sibling: `prog_tlv_guard` — the same validator with TWO digest words —
/// compiles at all four levels; four words still panic at all four.
/// Compile-time — no inputs involved. Un-ignore when the spill transform
/// recomputes dominance after splitting edges.
#[test]
#[ignore = "compiler panic at every optimization level: 'called `Option::unwrap()` on a `None` \
            value' at hir/src/ir/dominance/frontier.rs:123 (DominanceFrontier::new from \
            spill::rewrite_cfg_spills) — F6 class; compile-time, no inputs involved"]
fn prog_tlv() {
    run_case("prog_tlv", include_str!("../cases/case_prog_tlv.rs"));
}

/// The largest variant of `prog_tlv` that compiles: two digest words instead
/// of eight. Passes at all four optimization levels.
#[test]
fn prog_tlv_guard() {
    run_case("prog_tlv_guard", include_str!("../cases/case_prog_tlv_guard.rs"));
}

/// Per-path pinned grid for [`prog_tlv_guard`]: `input2`'s low bits break the
/// container magic (bit 0) and the version (bit 1) and set the record count
/// and the value type, so these pairs pin the accepted deep path, both
/// rejected middle paths and the rejected outer path, plus zero / all-ones /
/// equal pairs.
#[test]
fn prog_tlv_guard_edges() {
    run_case_with_inputs(
        "prog_tlv_guard_edges",
        include_str!("../cases/case_prog_tlv_guard.rs"),
        &[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 12),
            (0x1234_5678, 4),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
        ],
    );
}
