//! Tests of the `::intrinsics::mem::load_rmw_window` intrinsic, which loads the 32-bit window a
//! sub-word store crossing a cell boundary merges into.

use super::check_mem_intrinsic;

/// The window starting at byte `offset` of the cell holding `e0`, with the bytes past that cell
/// taken from the cell holding `e1`; only the low 32 bits of each cell count.
fn window(e0: u64, e1: u64, offset: u32) -> u32 {
    ((e0 as u32) >> (offset * 8)) | ((e1 as u32) << (32 - offset * 8))
}

/// The window is assembled from the low 32 bits of cells holding felts at or above `2^32`, and
/// the intrinsic traps on an aligned or out-of-range offset, including an offset whose bit offset
/// wraps back into range, and on an element address overflow.
#[test]
fn load_rmw_window_truncates_felt_cells() {
    // 2^40 + 7 and 2^32 + 3: the low halves 7 and 3 give 768 at offset 3.
    assert_eq!(window((1 << 40) + 7, (1 << 32) + 3, 3), 768);
    for (e0, e1) in [
        ((1u64 << 40) + 7, (1u64 << 32) + 3),
        ((1 << 40) | 0x89ab_cdef, (1 << 33) | 0x0123_4567),
    ] {
        for offset in 1..=3 {
            let expected = window(e0, e1, offset);
            check_mem_intrinsic(
                &format!(
                    "push.{e1} mem_store.17 push.{e0} mem_store.16 push.{offset}.16 \
                     exec.::intrinsics::mem::load_rmw_window push.{expected} assert_eq"
                ),
                true,
            );
        }
    }
    // An aligned window never needs merging across cells, and offsets past the cell are invalid.
    for offset in [0, 4] {
        check_mem_intrinsic(
            &format!("push.{offset}.16 exec.::intrinsics::mem::load_rmw_window drop"),
            false,
        );
    }
    // 2^29 + 1 becomes bit offset 8 under `u32wrapping_mul`, which the shifts accept, so only the
    // `offset < 4` assert rejects it.
    check_mem_intrinsic(
        &format!("push.{}.16 exec.::intrinsics::mem::load_rmw_window drop", (1u32 << 29) + 1),
        false,
    );
    // The second cell of a window at the last element address would wrap around to address 0.
    check_mem_intrinsic(
        &format!("push.3.{} exec.::intrinsics::mem::load_rmw_window drop", u32::MAX),
        false,
    );
}
