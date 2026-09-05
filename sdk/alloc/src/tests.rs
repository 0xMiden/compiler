use super::*;

fn reserve(top: usize, limit: usize, size: usize, align: usize) -> Option<core::ops::Range<usize>> {
    allocation_range(top, limit, Layout::from_size_align(size, align).unwrap())
}

#[test]
fn mixed_alignment_allocations_are_disjoint() {
    let first = reserve(256, 1024, 64, 64).unwrap();
    let second = reserve(first.end, 1024, 16, 16).unwrap();
    assert_eq!(first, 256..320);
    assert_eq!(second, 320..336);
    assert!(first.end <= second.start);
}

#[test]
fn aligns_the_address_even_when_alignment_exceeds_size() {
    assert_eq!(reserve(272, 1024, 1, 64), Some(320..321));
    assert_eq!(reserve(321, 1024, 3, 1), Some(336..339));
}

#[test]
fn capacity_includes_alignment_padding_and_entire_allocation() {
    assert_eq!(reserve(272, 336, 16, 64), Some(320..336));
    assert_eq!(reserve(272, 335, 16, 64), None);
    assert_eq!(reserve(336, 336, 1, 16), None);
    assert_eq!(reserve(352, 336, 1, 16), None);
}

#[test]
fn rejects_address_overflow_and_uninitialized_heap() {
    assert_eq!(reserve(usize::MAX - 7, usize::MAX, 1, 16), None);
    assert_eq!(reserve(usize::MAX - 15, usize::MAX, 16, 16), None);
    assert_eq!(reserve(0, 1024, 1, 1), None);
}
