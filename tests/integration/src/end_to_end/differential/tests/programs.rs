//! Program-scale composites (campaign 17, 2026-09-03): realistic `no_std`
//! programs — the kind a Miden user would compile — each folding its whole
//! state into the result, so that long-range interactions between passes
//! (spills x lifting x memory x arithmetic over hundreds of ops) run
//! differentially. Every program has a pinned `_edges` twin and was
//! deep-fuzzed with `FUZZA_INPUT_PAIRS=512` before being kept.

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
