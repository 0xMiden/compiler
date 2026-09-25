//! Heap programs over a case-local bump allocator (campaign 34).
//!
//! Every case in this module owns its allocator: ~20 lines of `GlobalAlloc`
//! over a `static` arena, duplicated verbatim per case so the wasm guest and
//! the native `cdylib` run the SAME allocator code and no SDK crate is
//! involved. Two rules make that sound:
//!
//! * the allocator is RESET at entry (`NEXT = 0`), because the native
//!   `cdylib` stays loaded across all input pairs of a run while the VM
//!   starts fresh each time — without the reset the arena leaks natively
//!   only, and a late pair OOMs on one side;
//! * results are ADDRESS-INDEPENDENT — content hashes and `len() as u32`,
//!   never a pointer, never `capacity()`, and never a `usize` payload
//!   (`usize` is 64 bits natively and 32 bits on wasm).
//!
//! The arena is `#[repr(align(16))]`: a `[u8; N]` static is only
//! byte-aligned, while the loads LLVM emits for `u32`/`u64` elements carry
//! `align=4`/`align=8` memargs.
//!
//! One more rule the module learned the hard way: the two sides do not link
//! the same `core`. The guest's is built by `-Z build-std` with
//! `optimize_for_size`, the native `cdylib`'s is the host sysroot's, so any
//! API whose result is UNSPECIFIED may legitimately differ — the unstable
//! sorts are `heapsort` on one side and `ipnsort` on the other, and the
//! order of equal keys is not part of the contract. See `heap_sort`.

use super::super::harness::{
    run_case, run_case_traps, run_case_traps_with_inputs, run_case_with_flags, run_case_with_inputs,
};

/// Bring-up: a `Vec<u32>` grown by `input1 & 63` pushes over the bump
/// allocator, hashed by content and length. The realloc path is one
/// `memory.copy` (no `memcpy`/`memmove` libcall), which `midenc` sees as
/// `hir.mem_cpy`.
#[test]
fn heap_vecsum() {
    run_case("heap_vecsum", include_str!("../cases/case_heap_vecsum.rs"));
}

/// Vec growth: amortized pushes across the 4/8/16/32 capacity boundaries,
/// `extend_from_slice` (exact growth) and `reserve`.
#[test]
fn heap_vec_grow() {
    run_case("heap_vec_grow", include_str!("../cases/case_heap_vec_grow.rs"));
}

/// Pinned grid for `heap_vec_grow`: counts 0, 1, 4, 5 and 33 — every
/// capacity boundary plus the empty and single-element buffers, which random
/// draws of `input1 % 34` hit only by chance.
#[test]
fn heap_vec_grow_edges() {
    run_case_with_inputs(
        "heap_vec_grow_edges",
        include_str!("../cases/case_heap_vec_grow.rs"),
        &[
            (0, 1),
            (1, 1),
            (4, 1),
            (5, 1),
            (33, 1),
            (0, 0xffff_ffff),
            (1, 0x8000_0000),
            (4, 0x7fff_ffff),
            (5, 0),
            (33, 0xffff_ffff),
            (16, 3),
            (17, 3),
            (32, 3),
        ],
    );
}

/// Element sizes through the realloc copy: `Vec<u8>` (1), `Vec<u64>` (8),
/// `Vec<[u8; 3]>` (odd) and `Vec<(u8, u32)>` (padded).
#[test]
fn heap_vec_kinds() {
    run_case("heap_vec_kinds", include_str!("../cases/case_heap_vec_kinds.rs"));
}

/// The non-shifting edit surface: `swap_remove`, `pop`, `resize`,
/// `truncate`, `retain`, `dedup` — the passing sibling that bounds the
/// shift findings to overlapping bulk moves.
#[test]
fn heap_vec_edit() {
    run_case("heap_vec_edit", include_str!("../cases/case_heap_vec_edit.rs"));
}

/// `Vec<u32>::insert(i, v)` — an upward overlapping `memory.copy` whose
/// operands are all 4-aligned, so it takes the element fast path.
///
/// `Vec::insert` is the plainest producer of the `memory::mem_overlap`
/// defect there is: no `unsafe`, no `copy_within`, just the container method
/// every Rust program uses. Bounded by `heap_vec_edit` (same container, same
/// allocator, non-overlapping moves only — passes) and by
/// `heap_vec_remove_u8` (same overlapping copy, byte-loop arm, downward —
/// passes).
#[test]
#[ignore = "MASM-only abort, same defect as memory::mem_overlap: Vec<u32>::insert shifts the tail \
            up by one element with an overlapping memory.copy whose src/dst/count are all \
            4-aligned, so it takes the ::miden::core::mem::memcopy_elements fast path and the VM \
            stops with 'assertion failed with error message: source and destination ranges must \
            not overlap' (mem.masm:100); natively the shift is a memmove and returns a value. e.g. \
            inputs (4, 1) — see heap_vec_shift_repro. Un-ignore when hir.mem_cpy gets memmove \
            semantics (which fixes memory::mem_overlap and heap_vec_drain too)"]
fn heap_vec_shift() {
    run_case("heap_vec_shift", include_str!("../cases/case_heap_vec_shift.rs"));
}

/// Pinned twin of [`heap_vec_shift`]: `insert` in the middle of an 8-element
/// buffer (4, 1), at the front (4, 0), and one pair from a random run.
#[test]
#[ignore = "pinned reproducer of heap_vec_shift (memcopy_elements overlap abort)"]
fn heap_vec_shift_repro() {
    run_case_with_inputs(
        "heap_vec_shift_repro",
        include_str!("../cases/case_heap_vec_shift.rs"),
        &[(4, 1), (4, 0), (0, 2)],
    );
}

/// `Vec<u32>::remove(j)` + `drain(a..b)` — the same 4-aligned overlapping
/// copy in the DOWNWARD direction, which a forward copy loop would serve
/// correctly. The element fast path refuses it anyway, which is what makes
/// this the complement of [`heap_vec_shift`].
#[test]
#[ignore = "MASM-only abort, same defect as memory::mem_overlap and heap_vec_shift, other \
            direction: Vec<u32>::remove / drain shift the tail DOWN with a 4-aligned overlapping \
            memory.copy, and ::miden::core::mem::memcopy_elements rejects overlap in both \
            directions — 'assertion failed with error message: source and destination ranges must \
            not overlap' (mem.masm:100) — even though a forward copy is correct for dst < src. \
            e.g. inputs (4, 1) — see heap_vec_drain_repro. Bounded by heap_vec_remove_u8, the same \
            downward overlap on the byte-loop arm, which passes. Un-ignore when the fast path \
            accepts dst < src overlap, or when hir.mem_cpy gets memmove semantics"]
fn heap_vec_drain() {
    run_case("heap_vec_drain", include_str!("../cases/case_heap_vec_drain.rs"));
}

/// Pinned twin of [`heap_vec_drain`].
#[test]
#[ignore = "pinned reproducer of heap_vec_drain (memcopy_elements overlap abort, dst < src)"]
fn heap_vec_drain_repro() {
    run_case_with_inputs(
        "heap_vec_drain_repro",
        include_str!("../cases/case_heap_vec_drain.rs"),
        &[(4, 1), (0, 0), (7, 3)],
    );
}

/// `Vec<u8>::insert(i, b)` — an upward overlapping `memory.copy` one byte
/// wide, which can never take the element fast path and so hits the byte
/// fallback loop instead.
///
/// The byte loop copies UPWARD, so shifting a range up re-reads bytes it has
/// already overwritten: this is the SILENT half of the overlap defect —
/// wrong values, no abort, no diagnostic. `memorder::copy_fwd` pins that the
/// same loop is correct for `dst < src`, and `heap_vec_remove_u8` pins it for
/// this exact container and allocator, so direction is the only variable.
#[test]
#[ignore = "native/MASM value divergence (SILENT, no abort): Vec<u8>::insert shifts the tail up by \
            one byte with an overlapping memory.copy that can never be 4-aligned, so codegen takes \
            the memcpy byte fallback loop, which copies UPWARD and re-reads bytes it has already \
            overwritten; wasm memory.copy has memmove semantics. Inputs (7, 0): native 3483233431, \
            masm 2893074715; wasmtime on the harness-built wasm returns -811733865 = 3483233431, \
            i.e. wasmtime == native, so the compiler is wrong, not the guest toolchain. Same root \
            as memory::mem_overlap / heap_vec_shift but the byte arm has no overlap assert at all. \
            Bounded by heap_vec_remove_u8 (same loop, dst < src, passes). Un-ignore when the \
            memcpy byte loop copies downward when dst > src (or hir.mem_cpy gets memmove semantics)"]
fn heap_vec_shift_u8() {
    run_case("heap_vec_shift_u8", include_str!("../cases/case_heap_vec_shift_u8.rs"));
}

/// Pinned twin of [`heap_vec_shift_u8`]: the first random failing pair plus
/// an insert at the front of the buffer.
#[test]
#[ignore = "pinned reproducer of heap_vec_shift_u8 (silent byte-loop overlap corruption): inputs \
            (7, 0) give native 3483233431, masm 2893074715"]
fn heap_vec_shift_u8_repro() {
    run_case_with_inputs(
        "heap_vec_shift_u8_repro",
        include_str!("../cases/case_heap_vec_shift_u8.rs"),
        &[(7, 0), (4, 0), (0, 1)],
    );
}

/// `Vec<u8>::remove(j)` — the same byte-wide overlapping copy DOWNWARD, the
/// control of the direction/alignment matrix: it passes, so what breaks
/// `heap_vec_shift_u8` is the direction and nothing else.
#[test]
fn heap_vec_remove_u8() {
    run_case("heap_vec_remove_u8", include_str!("../cases/case_heap_vec_remove_u8.rs"));
}

/// `Box<[u32; 8]>`, `Vec<Box<u32>>` (a realloc that moves POINTERS),
/// `Option<Box<u32>>` in both niche states and an 8-byte-aligned `Box<u64>`.
#[test]
fn heap_box() {
    run_case("heap_box", include_str!("../cases/case_heap_box.rs"));
}

/// `Rc<RefCell<u32>>` shared by two handles: strong-count arithmetic through
/// clone/drop, `RefCell` borrow state, and `Rc::try_unwrap` in both outcomes.
#[test]
fn heap_rc() {
    run_case("heap_rc", include_str!("../cases/case_heap_rc.rs"));
}

/// A `Box`-linked list built, traversed, reversed by re-linking and dropped
/// ITERATIVELY (`Option::take` loop) — the automatic drop glue would be a
/// recursive call graph, which the assembler rejects. 0 `memory.copy`, so it
/// isolates pointer traffic from the bulk-copy findings.
#[test]
fn heap_list() {
    run_case("heap_list", include_str!("../cases/case_heap_list.rs"));
}

/// The same list at `--optimize=basic`, where the iterative drop is no
/// longer enough: LLVM keeps `core::ptr::drop_glue::<Node>`, which calls
/// itself through the `Option<Box<Node>>` link.
#[test]
#[ignore = "configuration-dependent compile failure at --optimize=basic only: the guest keeps \
            core::ptr::drop_glue::<Node> for the Box-linked list, which calls itself through the \
            Option<Box<Node>> link, and the assembler rejects it with 'found a cycle in the call \
            graph' (panic at tests/support/src/compiler_test.rs:1024). The case already drops \
            every node iteratively with an Option::take loop, which IS enough at the default \
            level, --optimize=max and --optimize=size-min (heap_list passes at all three, swept \
            2026-09-17) — cargo's opt-level 1 simply does not prove the glue dead. Known \
            limitation (recursive call graphs are unsupported), recorded here because it makes the \
            opt level a correctness cliff for a plain recursive data type. Un-ignore when \
            recursion is supported"]
fn heap_list_basic() {
    run_case_with_flags(
        "heap_list_basic",
        include_str!("../cases/case_heap_list.rs"),
        &["--optimize=basic"],
    );
}

/// `Vec<Box<dyn Op>>` read through `black_box`: vtable dispatch survives as 5
/// `call_indirect`s in the guest wasm (without `black_box`, nightly-2026-09-01
/// LLVM devirtualizes the whole thing).
#[test]
fn heap_dyn() {
    run_case("heap_dyn", include_str!("../cases/case_heap_dyn.rs"));
}

/// `VecDeque<u32>` wrapped around its ring and grown while wrapped: the
/// realloc path that has to re-join two live segments, plus wrap-around
/// logical indexing.
#[test]
fn heap_deque() {
    run_case("heap_deque", include_str!("../cases/case_heap_deque.rs"));
}

/// Pinned grid for `heap_deque`: counts 0, 1, 4, 5 and 33 (the ring starts at
/// capacity 4, so 4 and 5 straddle the first grow-while-wrapped), each with a
/// front-heavy and a back-heavy seed.
#[test]
fn heap_deque_edges() {
    run_case_with_inputs(
        "heap_deque_edges",
        include_str!("../cases/case_heap_deque.rs"),
        &[
            (0, 1),
            (1, 1),
            (4, 1),
            (5, 1),
            (33, 1),
            (0, 0xffff_ffff),
            (1, 0xffff_ffff),
            (4, 0xffff_ffff),
            (5, 0xffff_ffff),
            (33, 0xffff_ffff),
            (39, 0x8000_0000),
        ],
    );
}

/// `BinaryHeap<u32>` push/pop/peek with duplicate keys, then
/// `into_sorted_vec` — the sift-up and sift-down loops, and the pop ORDER is
/// part of the hash.
#[test]
fn heap_bheap() {
    run_case("heap_bheap", include_str!("../cases/case_heap_bheap.rs"));
}

/// `BTreeMap<u32, u32>` / `BTreeSet<u32>` across node splits (>11 keys) and
/// height-2 trees (up to 160 inserts), with removals driving underflow,
/// steals and merges. 84 `memory.copy` sites in the guest wasm — by far the
/// densest bulk-copy shape in the corpus.
///
/// `BTreeMap` keeps each node's keys and values in sorted arrays, so an
/// insert anywhere but at the end shifts the tail of those arrays UP with an
/// overlapping `ptr::copy`, and a remove shifts it back DOWN — the same
/// `heap_vec_shift` / `heap_vec_drain` defect, reached without the program
/// ever naming a bulk operation. Bounded by `heap_vec_edit` (heap container,
/// no overlapping move, passes) on one side and by `heap_bheap` (a container
/// that reorders by SWAPS rather than shifts, passes) on the other.
#[test]
#[ignore = "MASM-only abort, same defect as memory::mem_overlap / heap_vec_shift: BTreeMap (and \
            BTreeSet) shift a node's key and value arrays with an overlapping ptr::copy on every \
            insert or remove that is not at the end of the node, and for 4-byte keys/values that \
            copy is 4-aligned, so ::miden::core::mem::memcopy_elements aborts with 'source and \
            destination ranges must not overlap' (mem.masm:100). Inputs (3, 1) — three inserts \
            into one leaf, see heap_btree_repro; no split is needed. Un-ignore when hir.mem_cpy \
            gets memmove semantics"]
fn heap_btree() {
    run_case("heap_btree", include_str!("../cases/case_heap_btree.rs"));
}

/// Pinned twin of [`heap_btree`]: three inserts into a single leaf — the
/// smallest BTreeMap that shifts a key array — plus the first leaf split
/// (12 keys) and a height-2 tree (132 keys).
#[test]
#[ignore = "pinned reproducer of heap_btree (memcopy_elements overlap abort inside a B-tree node)"]
fn heap_btree_repro() {
    run_case_with_inputs(
        "heap_btree_repro",
        include_str!("../cases/case_heap_btree.rs"),
        &[(3, 1), (12, 1), (132, 1)],
    );
}

/// Sorting heap buffers: a hand-written iterative merge sort over a scratch
/// `Vec` (stability pinned by sorting packed key/position pairs) plus
/// `sort_unstable`, `sort_unstable_by_key` on `Vec<u64>` and
/// `sort_unstable_by` with a comparator.
#[test]
fn heap_sort() {
    run_case("heap_sort", include_str!("../cases/case_heap_sort.rs"));
}

/// Pinned grid for `heap_sort`: lengths 0, 1, 2, 19, 20 (the stable merge
/// sort's first multi-pass rungs) and 40, each at two seeds; plus the pair
/// (983633457, 2147483648) that caught the unstable-sort tie-order trap the
/// case doc describes, which now has to keep agreeing.
#[test]
fn heap_sort_edges() {
    run_case_with_inputs(
        "heap_sort_edges",
        include_str!("../cases/case_heap_sort.rs"),
        &[
            (0, 1),
            (1, 1),
            (2, 1),
            (19, 1),
            (20, 1),
            (40, 1),
            (0, 0xffff_ffff),
            (2, 0xffff_ffff),
            (20, 0xffff_ffff),
            (40, 0x8000_0000),
            (983633457, 2147483648),
        ],
    );
}

/// `Vec<u32>::sort()` — `core`'s stable sort. The guest links; the ASSEMBLER
/// rejects the program.
#[test]
#[ignore = "guest assembles into a recursive call graph: Vec::sort / sort_by / sort_by_key reach \
            core::slice::sort::stable::tiny::mergesort, which calls itself, and the assembler \
            fails with 'found a cycle in the call graph' (surfacing as a panic at \
            tests/support/src/compiler_test.rs) at ALL FOUR optimization levels (default, \
            size-min, max, basic — each re-measured with scratch/c34probe.sh). Same family as \
            corelib::core_select_nth_nolink, so it does NOT take the cargo test process down. \
            sort_unstable / sort_unstable_by / sort_unstable_by_key are fine (heapsort under \
            -Zbuild-std-features=optimize_for_size) — see heap_sort, which also shows a stable \
            sort with its own scratch allocation works when written iteratively. Un-ignore when \
            recursion is supported, or when core's stable sort stops recursing"]
fn heap_sort_stable_nolink() {
    run_case(
        "heap_sort_stable_nolink",
        include_str!("../cases/case_heap_sort_stable_nolink.rs"),
    );
}

/// `String` as a growing UTF-8 buffer: `push` of 1-, 2- and 3-byte chars,
/// `push_str`, `truncate`, `pop`, `String::from_utf8`, and the `chars` /
/// `chars().rev()` / `char_indices` / `bytes` walkers.
#[test]
fn heap_string() {
    run_case("heap_string", include_str!("../cases/case_heap_string.rs"));
}

/// `String::insert(idx, ch)` — the third container that shifts a buffer up
/// with an overlapping copy, on the byte-loop arm like `heap_vec_shift_u8`.
#[test]
#[ignore = "native/MASM value divergence (SILENT, no abort), same defect as heap_vec_shift_u8: \
            String::insert shifts the tail up by one byte with an overlapping memory.copy that can \
            never be 4-aligned, so codegen takes the memcpy byte fallback loop, which copies \
            UPWARD. Inputs (4, 1): native 2160629743, masm 3266568672; wasmtime on the \
            harness-built wasm returns -2134337553 = 2160629743, i.e. wasmtime == native, so the \
            compiler is wrong, not the guest toolchain. First caught inside heap_string (inputs \
            (32767, 128): native 2890095206, masm 3656259449), which now excludes insert; pinned \
            here — see heap_string_insert_repro. Bounded by heap_string (the same String surface \
            with appends only, passes). Un-ignore when the memcpy byte loop copies downward when \
            dst > src"]
fn heap_string_insert() {
    run_case("heap_string_insert", include_str!("../cases/case_heap_string_insert.rs"));
}

/// Pinned twin of [`heap_string_insert`].
#[test]
#[ignore = "pinned reproducer of heap_string_insert (silent byte-loop overlap corruption)"]
fn heap_string_insert_repro() {
    run_case_with_inputs(
        "heap_string_insert_repro",
        include_str!("../cases/case_heap_string_insert.rs"),
        &[(4, 1), (0, 2), (7, 3)],
    );
}

/// `core::fmt` over a heap buffer: `format!`, `to_string`, `write!` into a
/// `String`, radix/width/padding specs, derived `Debug`, and
/// `str::parse::<u32>()` back out of a runtime-length slice.
#[test]
fn heap_fmt() {
    run_case("heap_fmt", include_str!("../cases/case_heap_fmt.rs"));
}

/// `collect::<Vec<_>>()` out of `map` (exactly sized), `filter` (unknown
/// length — the vector grows), `chain`, `zip`, `rev`, plus `extend`,
/// `take`/`skip`/`step_by` and the `sum`/`max`/`min`/`position` consumers.
#[test]
fn heap_iters() {
    run_case("heap_iters", include_str!("../cases/case_heap_iters.rs"));
}

/// Allocator exhaustion as trap parity: odd `input1` rows ask for far more
/// than the 1 KiB arena, so `alloc` returns null and `handle_alloc_error`
/// panics — a `unreachable` on wasm, `_exit(101)` on the forked host child —
/// while the even rows fit and are value-checked in the same run. No
/// `alloc_error_handler` is needed: the stable default handler panics.
#[test]
fn heap_oom() {
    run_case_traps("heap_oom", include_str!("../cases/case_heap_oom.rs"));
}

/// Pinned grid for `heap_oom`: the two smallest OOM rows, the empty and
/// one-element fitting rows, and the largest fitting row (32 u32s).
#[test]
fn heap_oom_edges() {
    run_case_traps_with_inputs(
        "heap_oom_edges",
        include_str!("../cases/case_heap_oom.rs"),
        &[
            (1, 1),
            (3, 0xffff_ffff),
            (0, 1),
            (0x0000_0100, 1),
            (0x0000_2000, 7),
            (0xffff_fffe, 0),
        ],
    );
}

/// `memory.grow` / `memory.size` under the fixed heap model: the observable
/// is what every model agrees on — success flags and size DELTAS (a small
/// growth raises the size by exactly the pages requested; a 2^20-page
/// request returns `usize::MAX` and leaves the size untouched; a zero-page
/// growth changes nothing). 10 `memory.grow`/`memory.size` ops survive into
/// the guest wasm.
#[test]
fn heap_grow() {
    run_case("heap_grow", include_str!("../cases/case_heap_grow.rs"));
}
