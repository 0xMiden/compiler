//! The `core` library surface at program scale, and which parts of it a
//! guest can link at all (campaign 27, 2026-09-10).
//!
//! Two halves. The `core_*` cases are the LINK-REACH map: one case per
//! `core` facility whose linkability is in question, kept as a passing guard
//! when the facility links and as an `#[ignore]`d `*_nolink` case when it
//! does not, with the exact linker error and the smallest source-level
//! replacement in the doc comment. The `prog_*` cases are realistic programs
//! written the way a user writes them, each around one cluster of that
//! surface (`core::str`, `core::fmt`, derived `Debug`, iterator pipelines,
//! slice algorithms, `core::num`, `char`, `Option`/`Result`, derived `Ord`,
//! `core::ptr`), each with a pinned per-path `_edges` grid.
//!
//! Three facts shape the whole module:
//!
//! * The blocking symbol is always `memcmp`. Any comparison that `core`
//!   performs as a slice compare — `&[T] == &[T]`, `&str == &str`,
//!   `starts_with` / `ends_with`, the two-way substring searcher, the
//!   `CharSearcher` inside `str::split` / `find` — lowers to a `memcmp`
//!   libcall, and a guest has no wasi-libc and no compiler-builtins `mem`
//!   symbols, so the link fails with `rust-lld: error: <obj>: undefined
//!   symbol: memcmp`. A user sees a linker error, not a `midenc` diagnostic.
//! * Linkability is a per-PROGRAM, per-OPTIMIZATION-LEVEL property, not a
//!   per-API one. When the comparison is inlined it folds into ordinary
//!   loads and the program links; when it stays outlined the libcall
//!   survives. So a constant-size `[u8; 4] == [u8; 4]` links everywhere, a
//!   `[u32; 8] == [u32; 8]` links at every level except `--optimize=size-min`
//!   (`core_eq_reach_oz`), one `str::split(char)` links except at
//!   `--optimize=basic` (`core_str_patterns_basic`), `str::find(char)` links
//!   only at the default level and at `--optimize=max`
//!   (`core_str_find_oz`), and two nested `str::split(char)` loops link at no
//!   level at all (`prog_expr`).
//! * `core`'s unstable sorts are usable again. Campaign 17 recorded them as
//!   recursive (ipnsort) and therefore rejected by the assembler's
//!   call-graph-cycle check; the compiler builds `core` with
//!   `-Zbuild-std-features=optimize_for_size`, whose unstable sort is the
//!   non-recursive `heapsort`, so `sort_unstable` / `sort_unstable_by` /
//!   `sort_unstable_by_key` link and match native (`core_sorts`). Only
//!   `select_nth_unstable` still does not: it keeps the recursive
//!   `median_of_medians` (`core_select_nth_nolink`).
//!
//! It also holds the campaign's two VALUE divergences, told apart by
//! `wasmtime` on the harness-built wasm: `str::parse::<i64>` of a
//! runtime-length slice is miscompiled by `midenc` (`core_parse_i64`:
//! native = wasmtime != masm), while an `i64::checked_mul` by a constant in
//! a loop is miscompiled by the GUEST TOOLCHAIN (`core_chkmul_i64`:
//! wasmtime = masm != native, the F9 family). Both go through
//! `i64.mul_wide_s`; only the first one is a compiler bug.
//!
//! OPERATIONAL NOTE: a `*_nolink` case does not just fail — the guest build
//! error ABORTS the whole `cargo test` process (no `test result` line is
//! printed), so run them one at a time with
//! `-- --ignored --exact <full::path>` and never in a batch with tests whose
//! result you need.
//!
//! Every case here was value-checked natively on the 1225-pair boundary grid
//! before it was kept, and compiled at all four optimization levels; each
//! doc comment records that table. Compile-time failures are `#[ignore]`d
//! twins beside the compiling sibling, exactly as in [`super::programs`].

use super::super::harness::{run_case, run_case_with_flags, run_case_with_inputs};

/// `--optimize=size-min` (LLVM `-Oz` for the guest).
const SIZE_MIN: &[&str] = &["--optimize=size-min"];
/// `--optimize=basic` (LLVM `-O1` for the guest).
const BASIC: &[&str] = &["--optimize=basic"];
/// `--optimize=max` (LLVM `-O3` for the guest).
const MAX: &[&str] = &["--optimize=max"];

// ---------------------------------------------------------------------------
// Part A — the link-reach map
// ---------------------------------------------------------------------------

/// `core`'s unstable sorts over runtime-length sub-slices: `sort_unstable`,
/// `sort_unstable_by_key` and `sort_unstable_by`, cross-checked against a
/// hand-written insertion sort and queried with `binary_search` /
/// `binary_search_by_key` / `is_sorted`. They link and assemble at all four
/// optimization levels because the guest's `core` is built with
/// `optimize_for_size`, whose unstable sort is the non-recursive
/// `core::slice::sort::unstable::heapsort` (the symbol is in the guest
/// wasm's name section). This supersedes campaign 17's "the unstable sorts
/// do not link" fact.
#[test]
fn core_sorts() {
    run_case("core_sorts", include_str!("../cases/case_core_sorts.rs"));
}

/// `slice::select_nth_unstable` on a runtime-length slice: the one member of
/// the unstable-sort family that is still unusable. It reaches
/// `core::slice::sort::select::median_of_medians`, which calls itself, and
/// the assembler rejects the cycle:
/// `found a cycle in the call graph, involving these procedures:
/// ::<pkg>::<pkg>::_RINvNtNtNtCsjd5dvg1KLHY_4core5slice4sort6select17median_of_medians...`
/// — at the default level, `--optimize=size-min`, `--optimize=max` and
/// `--optimize=basic` alike. A CONSTANT length is not a reproducer (LLVM
/// specialises the selection away). Workaround: sort with `sort_unstable`
/// and index, or write the selection by hand. Un-ignore when the assembler
/// tolerates recursion, or when `core` stops recursing here.
#[test]
#[ignore = "F13: select_nth_unstable reaches the recursive median_of_medians; assembler: found a \
            cycle in the call graph"]
fn core_select_nth_nolink() {
    run_case("core_select_nth_nolink", include_str!("../cases/case_core_selectnth.rs"));
}

/// The equality forms that DO link: a constant-size `[u8; 4] == [u8; 4]`, a
/// derived `PartialEq` over a struct with a `[u8; 16]` field, a
/// `[u32; 8] == [u32; 8]`, `Iterator::eq`, `zip(..).all(..)`,
/// `iter().cmp(..)`, `str::eq_ignore_ascii_case`,
/// `[u8]::eq_ignore_ascii_case` and `[u8]::contains`. On nightly-2026-04-30
/// it passed at the default level, at `--optimize=max` and at
/// `--optimize=basic`, with the constant-size array compares becoming
/// `memcmp` only at `--optimize=size-min` (`core_eq_reach_oz`); since the
/// nightly-2026-09-01 toolchain bump the default level outlines them too, so
/// the guest no longer links. A guest build failure exits the whole test
/// process (`midenc-compile` calls `process::exit` on a failed `cargo
/// build`), so this stays ignored rather than failing.
#[test]
#[ignore = "F13: constant-size array `==` becomes a memcmp libcall at the default level on \
            nightly-2026-09-01 (it did only at -Oz on nightly-2026-04-30); rust-lld: undefined \
            symbol: memcmp"]
fn core_eq_reach() {
    run_case("core_eq_reach", include_str!("../cases/case_core_eqreach.rs"));
}

/// [`core_eq_reach`] at `--optimize=size-min`: the constant-size array
/// comparisons that inline at every other level are outlined here, and the
/// guest link fails with `rust-lld: error: <obj>: undefined symbol: memcmp`.
/// The user-visible rule is that `-Oz` makes even a fixed-size `==` a
/// libcall — replace it with `iter().eq(..)` or a byte loop. Un-ignore when
/// the guest links against a `memcmp` implementation.
#[test]
#[ignore = "F13: constant-size array `==` becomes a memcmp libcall at -Oz; rust-lld: undefined \
            symbol: memcmp"]
fn core_eq_reach_oz() {
    run_case_with_flags(
        "core_eq_reach_oz",
        include_str!("../cases/case_core_eqreach.rs"),
        SIZE_MIN,
    );
}

/// The `core::str` surface that links at every optimization level:
/// `from_utf8` validation, `char_indices` / `chars().rev()` / `bytes()`,
/// `trim` / `trim_start` / `trim_end_matches` / `trim_matches`,
/// `split_whitespace`, `is_char_boundary`, `get`, `char::encode_utf8`,
/// `parse::<u32>` / `parse::<i64>` and `u32` / `i32::from_str_radix`.
#[test]
fn core_str_scan() {
    run_case("core_str_scan", include_str!("../cases/case_core_strscan.rs"));
}

/// One `str::split(char)` loop with `parse` and `trim` inside it. Links at
/// the default level, at `--optimize=size-min` and at `--optimize=max`
/// (`SplitInternal::<CharSearcher>::next` is inlined and its slice compare
/// folds to a byte load) and NOT at `--optimize=basic`
/// (`core_str_patterns_basic`).
#[test]
fn core_str_patterns() {
    run_case("core_str_patterns", include_str!("../cases/case_core_strpat.rs"));
}

/// [`core_str_patterns`] at `--optimize=basic`: the searcher stays outlined
/// and the guest link fails with `rust-lld: error: <obj>: undefined symbol:
/// memcmp`. Workaround: scan with `char_indices` and slice by hand (see
/// `prog_expr_wa`). Un-ignore when the guest links against a `memcmp`.
#[test]
#[ignore = "F13: str::split(char) keeps an outlined memcmp at -O1; rust-lld: undefined symbol: \
            memcmp"]
fn core_str_patterns_basic() {
    run_case_with_flags(
        "core_str_patterns_basic",
        include_str!("../cases/case_core_strpat.rs"),
        BASIC,
    );
}

/// `str::find` / `rfind` with a `char` pattern. Links at the default level
/// and at `--optimize=max` only; at `--optimize=size-min` and
/// `--optimize=basic` the same `CharSearcher` stays outlined and takes the
/// `memcmp` libcall with it (`core_str_find_oz` pins the `-Oz` half).
#[test]
fn core_str_find() {
    run_case("core_str_find", include_str!("../cases/case_core_strfind.rs"));
}

/// [`core_str_find`] at `--optimize=size-min`: `rust-lld: error: <obj>:
/// undefined symbol: memcmp`. `--optimize=basic` fails identically.
/// Workaround: `s.as_bytes().iter().position(|&b| b == c as u8)` for an
/// ASCII pattern. Un-ignore when the guest links against a `memcmp`.
#[test]
#[ignore = "F13: str::find(char) keeps an outlined memcmp at -Oz (and at -O1); rust-lld: undefined \
            symbol: memcmp"]
fn core_str_find_oz() {
    run_case_with_flags(
        "core_str_find_oz",
        include_str!("../cases/case_core_strfind.rs"),
        SIZE_MIN,
    );
}

/// Runtime-length slice comparison — `==`, `!=`, `<`, `>=` on `&[u8]` — at
/// every optimization level: `rust-lld: error: <obj>: undefined symbol:
/// memcmp`. The replacements are in [`core_eq_reach`]: `iter().eq(..)`,
/// `zip(..).all(..)`, `iter().cmp(..)`, or a constant-size array `==`
/// (which inlines everywhere except `-Oz`). Un-ignore when the guest links
/// against a `memcmp` implementation.
#[test]
#[ignore = "F13: slice ==/!=/< lowers to memcmp; rust-lld: undefined symbol: memcmp"]
fn core_slice_eq_nolink() {
    run_case("core_slice_eq_nolink", include_str!("../cases/case_core_sliceeq.rs"));
}

/// `&str == &str` and `Option<&str> == Option<&str>` at every optimization
/// level, even between same-length literals: `rust-lld: error: <obj>:
/// undefined symbol: memcmp`. Replacements: `eq_ignore_ascii_case`,
/// `as_bytes().iter().eq(..)`, or a byte loop. Un-ignore when the guest
/// links against a `memcmp` implementation.
#[test]
#[ignore = "F13: str equality lowers to memcmp; rust-lld: undefined symbol: memcmp"]
fn core_str_eq_nolink() {
    run_case("core_str_eq_nolink", include_str!("../cases/case_core_streq.rs"));
}

/// `[u8]::starts_with` / `ends_with` and `str::starts_with(&str)` at every
/// optimization level: `rust-lld: error: <obj>: undefined symbol: memcmp`
/// (they compare a sub-slice with `==` internally). Replacement:
/// `a.iter().zip(prefix).all(|(x, y)| x == y)`, or `starts_with(char)` for a
/// one-character prefix. Un-ignore when the guest links against a `memcmp`.
#[test]
#[ignore = "F13: starts_with/ends_with lower to memcmp; rust-lld: undefined symbol: memcmp"]
fn core_starts_with_nolink() {
    run_case("core_starts_with_nolink", include_str!("../cases/case_core_startswith.rs"));
}

/// `str::contains(&str)` / `find(&str)` / `rfind(&str)` — `core`'s two-way
/// substring searcher — at every optimization level: `rust-lld: error:
/// <obj>: undefined symbol: memcmp`. Replacement: a hand-written window scan
/// over `as_bytes()`, or a single-`char` search ([`core_str_find`]).
/// Un-ignore when the guest links against a `memcmp` implementation.
#[test]
#[ignore = "F13: the two-way substring searcher lowers to memcmp; rust-lld: undefined symbol: \
            memcmp"]
fn core_str_search_nolink() {
    run_case("core_str_search_nolink", include_str!("../cases/case_core_strsearch.rs"));
}

/// A native-vs-MASM DIVERGENCE (campaign 27): `str::parse::<i64>` of a
/// runtime-length digit slice. At `(0, 0)` the slice is `"9"`; native and
/// `wasmtime` on the harness-built wasm (`-W wide-arithmetic=y`) both return
/// 9, MASM returns 0. The wasm is therefore correct and the miscompile is on
/// the Miden side. `core_parse_u64` (same shape, `u64`) and `core_mulwide_s`
/// (the signed widening multiply on its own) pass, and `parse::<i32>` passes
/// inside `core_str_scan`. It reproduces at every optimization level. A
/// constant-length slice is not a reproducer (LLVM folds the parse away),
/// which is why `core_str_scan` did not see it until the slice came from
/// `split_whitespace`.
/// ROOT CAUSE (director, 2026-09-10; full mechanism at `wide::parse_i64_hand`):
/// the parse has an overflow-checked sixteen-digit path whose
/// `i64.mul_wide_s acc, 10` the frontend sign-extends to `i128`, and a plain
/// `acc * 10` path over the SAME `i64.const 10`; `Sext::fold` retypes the
/// shared `arith.constant 10 : i64` to an `i128` immediate in place
/// (dialects/arith/src/ops/coercions.rs), so the plain multiply is fed four
/// felts instead of two and the digit add consumes zeros — which is why the
/// wrong answer is exactly the digits dropped. Un-ignore when the coercion
/// folders stop mutating their operand constant.
#[test]
#[ignore = "DIVERGENCE: str::parse::<i64> of a runtime-length slice; inputs (0, 0): native 9 / \
            wasmtime 9 / masm 0"]
fn core_parse_i64() {
    run_case_with_inputs(
        "core_parse_i64",
        include_str!("../cases/case_core_parse_i64.rs"),
        &[(0, 0), (0, 1), (0, 2), (0, 5), (0, 9), (0, 15), (1, 3), (1, 0)],
    );
}

/// The `u64` half of [`core_parse_i64`]: the same runtime-length-slice parse
/// with an unsigned accumulator agrees with native, which is what bounds the
/// divergence to the signed path.
#[test]
fn core_parse_u64() {
    run_case_with_inputs(
        "core_parse_u64",
        include_str!("../cases/case_core_parse_u64.rs"),
        &[(0, 0), (0, 1), (0, 5), (0, 9), (0, 15), (1, 3), (1, 9), (2, 1), (2, 0), (1, 0)],
    );
}

/// The signed widening multiply on its own — `(a as i128) * (b as i128)`
/// over two runtime `i64` operands, the plain-Rust producer of
/// `i64.mul_wide_s` with both halves used. It agrees with native, so
/// [`core_parse_i64`] is not "the wide multiply is broken".
#[test]
fn core_mulwide_s() {
    run_case_with_inputs(
        "core_mulwide_s",
        include_str!("../cases/case_core_mulwide_s.rs"),
        &[
            (3, 5),
            (0xffff_ffff, 2),
            (0x8000_0000, 0x8000_0000),
            (7, 0xffff_fffb),
            (0, 0),
            (1, 1),
        ],
    );
}

/// A GUEST-TOOLCHAIN divergence (F9), kept because it is the smallest
/// producer of that family in the corpus: an `i64` accumulator multiplied by
/// the CONSTANT 10 with `checked_mul` in a loop — `core`'s signed `from_str`
/// fast-path shape. LLVM compiles the overflow check to `i64.mul_wide_s` +
/// `hi != lo >> 63` and gets it wrong: at `(0, 1)` the accumulator is 9,
/// `9 * 10` does not overflow, native returns 90, and BOTH `wasmtime` on the
/// harness-built wasm (`-W wide-arithmetic=y`) and MASM return the overflow
/// marker 0xdead. wasmtime agreeing with MASM is what attributes this to the
/// guest toolchain rather than to `midenc` (contrast [`core_parse_i64`],
/// where wasmtime agrees with native). Reproduces at all four optimization
/// levels — at `--optimize=basic` one digit later, `(0, 2)`. Un-ignore when
/// the guest toolchain's `+wide-arithmetic` lowering is fixed.
#[test]
#[ignore = "F9 guest toolchain: i64::checked_mul by a constant in a loop; inputs (0, 1): native 90 \
            / wasmtime 0xdead / masm 0xdead"]
fn core_chkmul_i64() {
    run_case_with_inputs(
        "core_chkmul_i64",
        include_str!("../cases/case_core_chkmul_i64.rs"),
        &[(0, 0), (0, 1), (0, 2), (0, 5), (0, 9), (0, 15), (1, 3)],
    );
}

// ---------------------------------------------------------------------------
// Part B — realistic programs on that surface
// ---------------------------------------------------------------------------

/// Program 1, as a user first writes it: a tokenizer / expression evaluator
/// that renders its inputs into a byte buffer, validates it with
/// `core::str::from_utf8` and evaluates it with `text.split('+')` and, INSIDE
/// that loop, `term.trim().split('*')`, `char_indices`, `is_ascii_digit`,
/// `parse::<i32>`, `checked_*` and a derived-`Debug` error enum carried
/// through `?`. Two `Split<char>` iterators in one function are one too
/// many: one of the searchers stays outlined and the guest link fails at
/// EVERY optimization level with `rust-lld: error: <obj>: undefined symbol:
/// memcmp` (the call is in
/// `core::str::iter::SplitInternal::<char>::next`, LLVM-IR-verified).
/// [`prog_expr_wa`] is the same program with the splits hand-written; it
/// links everywhere and computes the identical answer on the whole 1225-pair
/// native grid. Un-ignore when the guest links against a `memcmp`.
#[test]
#[ignore = "F13: two nested str::split(char) loops keep an outlined memcmp; rust-lld: undefined \
            symbol: memcmp"]
fn prog_expr() {
    run_case("prog_expr", include_str!("../cases/case_prog_expr.rs"));
}

/// Program 1, rewritten around the missing facility: the two `split(char)`
/// loops become a hand-written `char_indices` scan that yields `&str`
/// sub-slices, everything else (UTF-8 validation, `trim`,
/// `is_ascii_digit`, `parse::<i32>`, `checked_mul` / `checked_add`, the
/// derived-`Debug` error enum and `?`) unchanged. Compiles and matches
/// native at the default level, at `--optimize=size-min`, at
/// `--optimize=max` and at `--optimize=basic`.
#[test]
fn prog_expr_wa() {
    run_case("prog_expr_wa", include_str!("../cases/case_prog_expr_wa.rs"));
}

/// Pinned grid for [`prog_expr_wa`]: the clean parse (`input2 >> 3` ≡ 0 mod
/// 5), the planted non-digit (≡ 1) and the planted invalid UTF-8 lead byte
/// (≡ 2) — i.e. the `Ok`, `BadDigit` and `NotUtf8` result tags — plus zero,
/// all-ones, mixed and equal pairs. (`Overflow` and `TooManyTerms` are
/// unreachable for four four-digit terms; they are the error arms a user
/// still has to write.)
#[test]
fn prog_expr_wa_edges() {
    run_case_with_inputs(
        "prog_expr_wa_edges",
        include_str!("../cases/case_prog_expr_wa.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0000, 0x8000_0000),
            (12345, 8),
            (999, 16),
            (7, 24),
            (1, 3),
            (65535, 40),
        ],
    );
}

/// Program 2: a report formatter. Three table rows go into a 256-byte stack
/// buffer through a `fmt::Write` implementation with runtime width, runtime
/// fill and alignment, zero padding, explicit sign, lower/upper hex, binary
/// and octal radices and a precision field; the decimal and hex columns are
/// then located by a byte scan and parsed back with `str::parse` /
/// `u32::from_str_radix`, and the buffer is hashed. Compiles and matches
/// native at all four optimization levels.
#[test]
fn prog_fmttable() {
    run_case("prog_fmttable", include_str!("../cases/case_prog_fmttable.rs"));
}

/// Pinned grid for [`prog_fmttable`]: both runtime-width extremes
/// (`input2 % 14` at 0 and 13), the widest decimal and hex columns, the
/// `i32::MIN` signed column, a buffer-filling row set, zero / all-ones /
/// equal pairs.
#[test]
fn prog_fmttable_edges() {
    run_case_with_inputs(
        "prog_fmttable_edges",
        include_str!("../cases/case_prog_fmttable.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0xdead_beef, 0xdead_beef),
            (0x8000_0000, 13),
            (12345, 1),
            (1, 7),
            (0xffff_ffff, 27),
        ],
    );
}

/// Program 3: a `{:?}` / `{:#?}` dump of a nested value graph — derived
/// `Debug` on a struct holding an enum with unit-, tuple- and struct-
/// variants, a `[u8; 4]`, `Option<char>`, a `&str` needing escapes, a tuple
/// and a `Result` — rendered both compactly and pretty into one buffer, then
/// hashed together with the newline and indentation counts. Compiles and
/// matches native at all four optimization levels.
#[test]
fn prog_debugdump() {
    run_case("prog_debugdump", include_str!("../cases/case_prog_debugdump.rs"));
}

/// Pinned grid for [`prog_debugdump`]: every slice length (`input2 % 4`),
/// every pretty-printed frame (`input1 % 4`), the `None` and `Some` arms of
/// the `Option` field (`input1 & 4`), zero / all-ones / equal pairs.
#[test]
fn prog_debugdump_edges() {
    run_case_with_inputs(
        "prog_debugdump_edges",
        include_str!("../cases/case_prog_debugdump.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
            (4, 3),
            (5, 1),
            (2, 2),
            (7, 0),
        ],
    );
}

/// Program 4: a sample-stream reducer built entirely from iterator adapters
/// — `chunks_exact`, `windows`, `step_by`, `filter_map`, `scan`,
/// `take_while`, `skip_while`, `peekable` run-length collapsing, `flat_map`,
/// `enumerate`, `rev().zip()`, `max`/`min`/`position`/`rposition`, `once`,
/// `repeat`, `successors`. Compiles and matches native at all four
/// optimization levels.
#[test]
fn prog_pipeline() {
    run_case("prog_pipeline", include_str!("../cases/case_prog_pipeline.rs"));
}

/// Pinned grid for [`prog_pipeline`]: the shortest and longest live window
/// (`8 + input2 % 41`), a threshold below and above every sample, the
/// all-equal run (longest run-length), zero / all-ones / equal pairs.
#[test]
fn prog_pipeline_edges() {
    run_case_with_inputs(
        "prog_pipeline_edges",
        include_str!("../cases/case_prog_pipeline.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x0f0f_0f0f, 0x0f0f_0f0f),
            (1, 40),
            (3, 0),
            (0x7fff_ffff, 0x8000_0000),
            (2, 2),
        ],
    );
}

/// Program 5: a reorder buffer over a `[u32; 64]` — `rotate_left` /
/// `rotate_right`, `reverse`, `swap`, `split_at_mut`, `copy_from_slice`,
/// `fill`, `copy_within` between distinct ranges, `chunks_exact_mut`, an
/// insertion sort cross-checked element-wise against `sort_unstable` /
/// `sort_unstable_by_key`, `binary_search`, `windows(2).all(..)` and
/// `contains`. Compiles and matches native at the default level, at
/// `--optimize=size-min` and at `--optimize=max`; it does NOT compile at
/// `--optimize=basic` ([`prog_slicealg_basic`]).
#[test]
fn prog_slicealg() {
    run_case("prog_slicealg", include_str!("../cases/case_prog_slicealg.rs"));
}

/// Pinned grid for [`prog_slicealg`]: both rotation extremes
/// (`input2 % 64` at 0 and 63), the `copy_within` source at its lowest and
/// highest offset (`k % 24`), a probe value present and absent from the
/// sorted half, zero / all-ones / equal pairs.
#[test]
fn prog_slicealg_edges() {
    run_case_with_inputs(
        "prog_slicealg_edges",
        include_str!("../cases/case_prog_slicealg.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x5a5a_5a5a, 0x5a5a_5a5a),
            (5, 63),
            (9, 24),
            (0x1234, 0),
            (7, 7),
        ],
    );
}

/// [`prog_slicealg`] at `--optimize=basic`: the compiler panics with
/// `called `Option::unwrap()` on a `None` value` at
/// `hir/src/ir/dominance/frontier.rs:123:55`. The spills trace
/// (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`) shows
/// `edges to split = 17` and nineteen `max usage on exit (17) exceeds K (16),
/// additional spills required` lines for `entrypoint`, i.e. the F6
/// stale-dominator-tree cluster: the panic happens while the split edges are
/// being processed, before any reload is erased. The same program compiles
/// at the other three levels, so `--optimize=basic` is not a safe fallback.
/// Un-ignore when F6 is fixed.
#[test]
#[ignore = "F6: frontier.rs:123 Option::unwrap on None at --optimize=basic (17 edges to split, 19 \
            additional-spill rounds)"]
fn prog_slicealg_basic() {
    run_case_with_flags(
        "prog_slicealg_basic",
        include_str!("../cases/case_prog_slicealg.rs"),
        BASIC,
    );
}

/// Program 6, reduced to the variant that compiles everywhere: a
/// measurement pipeline over the `core::num` families — `leading_zeros` /
/// `trailing_zeros` / `count_ones` / `checked_ilog2` / `isqrt` / `pow` /
/// `abs_diff`, `div_euclid` / `rem_euclid`, and the `checked_*` /
/// `saturating_*` / `overflowing_*` / `wrapping_*` families across i8 / u16 /
/// i32 / u32 — closed by a `from_str_radix` round trip of a number the
/// program printed itself. No 128-bit types (the F9 family) and no floats.
/// It compiles at all four optimization levels WITH guest DWARF and not
/// without it ([`prog_numeric_nodwarf`]). The full program is
/// [`prog_numeric_full`]; the doc comment of `case_prog_numeric_guard.rs`
/// carries the measured reduction ladder.
#[test]
fn prog_numeric() {
    run_case("prog_numeric", include_str!("../cases/case_prog_numeric_guard.rs"));
}

/// Pinned grid for [`prog_numeric`]: the `checked_ilog2` zero input, the
/// `overflowing_mul` and `saturating_add` overflow arms, `i32::MIN` for
/// `checked_neg` / `saturating_abs`, the `checked_shl` count at and past the
/// width, divisor boundaries for `div_euclid` / `rem_euclid`, zero /
/// all-ones / equal pairs.
#[test]
fn prog_numeric_edges() {
    run_case_with_inputs(
        "prog_numeric_edges",
        include_str!("../cases/case_prog_numeric_guard.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0000, 0x8000_0000),
            (0x8000_0000, 1),
            (97, 13),
            (255, 7),
            (1, 0x7fff_ffff),
        ],
    );
}

/// [`prog_numeric`] in the RELEASE configuration (`--guest-debug=0`, no
/// guest DWARF — what `cargo miden build` actually emits): the compiler
/// panics with `AliasingViolationError { kind: Mutable, location: ...
/// operation.rs:877 }` at `hir/src/patterns/rewriter.rs:335:32`, and the
/// pattern driver's last attempt under
/// `MIDENC_TRACE='pattern-rewrite-driver=trace'` is
/// `remove-loop-invariant-args-from-before-block` on `scf.while` — the F12
/// cluster. With DWARF the same program compiles at all four optimization
/// levels, so this is the campaign-24 rule (the release configuration moves
/// F12's producer set) reached by ordinary `core::num` code: a `while` loop
/// over narrow integer accumulators, no `return` / `break` / `continue` in
/// it at all. Un-ignore when F12 is fixed.
#[test]
#[ignore = "F12: rewriter.rs:335 AliasingViolationError without guest DWARF (last pattern: \
            remove-loop-invariant-args-from-before-block)"]
fn prog_numeric_nodwarf() {
    run_case_with_flags(
        "prog_numeric_nodwarf",
        include_str!("../cases/case_prog_numeric_guard.rs"),
        &["--guest-debug=0"],
    );
}

/// Program 6 as first written: [`prog_numeric`] plus a 64-bit lane
/// (`swap_bytes` / `rotate_right` / `saturating_sub` / `isqrt` /
/// `to_le_bytes` on a `u64` built from two readings) and a byte-order block
/// (`to_le_bytes` / `to_be_bytes` / `from_le_bytes` / `from_be_bytes` /
/// `swap_bytes` / `reverse_bits`). It compiles ONLY at
/// `--optimize=size-min`; at the default level and at `--optimize=max` the
/// compiler panics at `codegen/masm/src/lower/lowering.rs:109:17` with
/// `failed to schedule operands: [%3324, %1243] for inst 'arith.shr' with
/// error: NoSolution constraints: [Move, Copy]` over a thirteen-operand,
/// sixteen-felt stack. It LOOKS like the arity-2 solver gap (a `Copy`
/// constraint on a stack inside the window), but the same function's spills
/// trace shows `edges to split = 4` and seven `erase unused reload` lines —
/// the stale-dominator-tree erasure — and erased split-edge reloads take
/// precedence in the classification (director, campaign 27): the values
/// those reloads should have brought back stay live on the operand stack
/// past their spills, which is what pushes the Copy-constrained operand out
/// of reach, so this is F6 wearing F2's signature. At `--optimize=basic` it
/// panics at `hir/src/ir/dominance/frontier.rs:123:55` — the same class at
/// its first site. Neither block is the lever on its own: removing either
/// one makes the default level compile. Un-ignore with the other F6
/// reproducers.
#[test]
#[ignore = "F6 (erased split-edge reloads in the spills trace): lowering.rs:109 NoSolution [Move, \
            Copy] on arith.shr over an in-window 16-felt stack at the default level and \
            --optimize=max; frontier.rs:123 at --optimize=basic"]
fn prog_numeric_full() {
    run_case("prog_numeric_full", include_str!("../cases/case_prog_numeric.rs"));
}

/// Program 7: a UTF-8 state machine. The inputs are encoded as one-, two-
/// and three-byte code points with `char::encode_utf8`, a malformation is
/// planted at an input-selected offset (stray continuation byte, truncated
/// sequence, surrogate lead), and the stream is decoded with
/// `core::str::from_utf8` + `Utf8Error::valid_up_to` / `error_len`
/// resynchronisation, running each decoded `char` through `to_digit`,
/// `is_ascii_*`, `to_ascii_uppercase` and a re-encode round trip. Compiles
/// and matches native at all four optimization levels.
#[test]
fn prog_charsm() {
    run_case("prog_charsm", include_str!("../cases/case_prog_charsm.rs"));
}

/// Pinned grid for [`prog_charsm`]: all four malformation kinds
/// (`input1 % 4`: none, continuation byte, truncated sequence, surrogate
/// lead), the malformation at the first and last eligible offset, zero /
/// all-ones / equal pairs.
#[test]
fn prog_charsm_edges() {
    run_case_with_inputs(
        "prog_charsm_edges",
        include_str!("../cases/case_prog_charsm.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0001, 0x8000_0001),
            (1, 5),
            (2, 9),
            (3, 1),
            (4, 0),
        ],
    );
}

/// Program 8: a record arena driven by `Option` / `Result` combinators —
/// `take` / `replace` / `get_or_insert_with` / `map` / `map_or` /
/// `and_then` / `filter` / `zip` / `ok_or` / `unwrap_or_else` / `?`, with
/// `mem::swap` / `mem::replace` / `mem::take` doing the in-place moves and a
/// derived-`Debug` error enum reporting empty slots, out-of-range indices
/// and underflow. Compiles and matches native at all four optimization
/// levels.
#[test]
fn prog_records() {
    run_case("prog_records", include_str!("../cases/case_prog_records.rs"));
}

/// Pinned grid for [`prog_records`]: the out-of-range slot index (both
/// `input1 % 9` and `input2 % 9` at 8), an all-empty arena (`v % 5 == 0`),
/// the underflow arm (large amounts), zero / all-ones / equal pairs.
#[test]
fn prog_records_edges() {
    run_case_with_inputs(
        "prog_records_edges",
        include_str!("../cases/case_prog_records.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x1234_5678, 0x1234_5678),
            (0, 8),
            (8, 0),
            (5, 5),
            (0xf0f0_f0f0, 3),
        ],
    );
}

/// Program 9: a priority table ordered by derived `Ord` — twelve composite
/// keys built with `array::from_fn` and projected with `array::map`, then
/// `max` / `min` / `max_by_key` / `min_by` with `Ordering::then_with`,
/// `clamp`, `partial_cmp`, `is_sorted` / `is_sorted_by`, an insertion sort
/// by the derived `Ord` and `sort_unstable_by` on a projection, the two
/// orders cross-checked element-wise. With nightly-2026-04-30 guests it
/// compiles and matches native at the default level, at
/// `--optimize=size-min` and at `--optimize=basic`, and fails only at
/// `--optimize=max` ([`prog_ordkeys_max`]); the nightly-2026-09-01 guest
/// shape fails the same way at the default level (the spills pass splits 8
/// edges, then `frontier.rs:123` unwraps `None` in the dominance-frontier
/// query — the F6 stale-dominator-tree mechanism; verified 2026-09-17 to
/// still pass with `RUSTUP_TOOLCHAIN=nightly-2026-04-30` guests).
#[test]
#[ignore = "F6: frontier.rs:123 Option::unwrap on None at the default level with \
            nightly-2026-09-01 guests (8 split edges in the spills trace); compiled on \
            nightly-2026-04-30"]
fn prog_ordkeys() {
    run_case("prog_ordkeys", include_str!("../cases/case_prog_ordkeys.rs"));
}

/// Pinned grid for [`prog_ordkeys`]: the first and last key index
/// (`input1 % 12`, `input2 % 12`), a clamp range whose ends are equal, all
/// three `Class` variants present, zero / all-ones / equal pairs.
#[test]
#[ignore = "F6: frontier.rs:123 Option::unwrap on None at the default level with \
            nightly-2026-09-01 guests; see prog_ordkeys"]
fn prog_ordkeys_edges() {
    run_case_with_inputs(
        "prog_ordkeys_edges",
        include_str!("../cases/case_prog_ordkeys.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x8000_0000, 0x8000_0000),
            (11, 0),
            (0, 11),
            (5, 7),
            (3, 3),
        ],
    );
}

/// [`prog_ordkeys`] at `--optimize=max`: the compiler panics with
/// `called `Option::unwrap()` on a `None` value` at
/// `hir/src/ir/dominance/frontier.rs:123:55` — the F6 stale-dominator-tree
/// cluster, reached here by ordinary derived-`Ord` comparison code that LLVM
/// unrolls at `-O3`. With nightly-2026-04-30 guests the same program
/// compiled at the other three levels; since nightly-2026-09-01 the default
/// level fails too ([`prog_ordkeys`]). Un-ignore when F6 is fixed.
#[test]
#[ignore = "F6: frontier.rs:123 Option::unwrap on None at --optimize=max"]
fn prog_ordkeys_max() {
    run_case_with_flags("prog_ordkeys_max", include_str!("../cases/case_prog_ordkeys.rs"), MAX);
}

/// Program 10: a wire-record patcher over a 128-byte buffer — unaligned
/// reads at u16 / u32 / u64 widths with `core::ptr::read_unaligned`,
/// `write_unaligned` patches, an overlapping `copy` between distinct
/// positions, a `copy_nonoverlapping` clone, `write_bytes` clearing, a
/// `read_volatile`, `offset_from` / `align_offset` pointer arithmetic and
/// `position` / `rposition` scans, all at input-derived offsets. Compiles
/// and matches native at all four optimization levels.
#[test]
fn prog_rawptr() {
    run_case("prog_rawptr", include_str!("../cases/case_prog_rawptr.rs"));
}

/// Pinned grid for [`prog_rawptr`]: the highest in-range read offset
/// (`input1 % 64` at 63), the highest patch offset (`input2 % 48` at 47),
/// the shortest and longest `copy_nonoverlapping` / `write_bytes` length
/// (`1 + input2 % 16`), zero / all-ones / equal pairs.
#[test]
fn prog_rawptr_edges() {
    run_case_with_inputs(
        "prog_rawptr_edges",
        include_str!("../cases/case_prog_rawptr.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (0, 0xffff_ffff),
            (0xffff_ffff, 0),
            (0x0102_0304, 0x0102_0304),
            (63, 47),
            (0, 15),
            (32, 16),
            (1, 1),
        ],
    );
}
