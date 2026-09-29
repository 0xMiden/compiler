use super::support::assert_memory_test_returns_zero;

/// Checks that `core::ptr::copy` within one `u32` buffer, with overlapping ranges whose addresses
/// and byte count are multiples of 4, produces memmove results in both directions and when the
/// range is copied onto itself.
///
/// Regression test for #1418.
#[test]
fn memory_copy_overlapping_aligned() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(buf: &mut [u32; 16], src_off: usize, dst_off: usize, len: usize) {
            unsafe {
                let p = buf.as_mut_ptr();
                core::ptr::copy(p.add(src_off), p.add(dst_off), len);
            }
        }

        #[inline(never)]
        fn fill(buf: &mut [u32; 16]) {
            let mut i = 0usize;
            while i < 16 {
                buf[i] = (i as u32).wrapping_mul(0x0101_0101).wrapping_add(0x0302_0100);
                i += 1;
            }
        }

        #[inline(never)]
        fn count_mismatches(buf: &[u32; 16], src_off: usize, dst_off: usize, len: usize) -> u32 {
            let mut mismatches = 0u32;
            let mut i = 0usize;
            while i < 16 {
                let source = if i >= dst_off && i < dst_off + len { i - dst_off + src_off } else { i };
                if buf[i] != (source as u32).wrapping_mul(0x0101_0101).wrapping_add(0x0302_0100) {
                    mismatches += 1;
                }
                i += 1;
            }
            mismatches
        }

        let mut mismatches = 0u32;
        let mut buf = [0u32; 16];

        fill(&mut buf);
        do_copy(
            &mut buf,
            core::hint::black_box(1),
            core::hint::black_box(2),
            core::hint::black_box(12),
        );
        mismatches += count_mismatches(&buf, 1, 2, 12);

        fill(&mut buf);
        do_copy(
            &mut buf,
            core::hint::black_box(2),
            core::hint::black_box(1),
            core::hint::black_box(12),
        );
        mismatches += count_mismatches(&buf, 2, 1, 12);

        fill(&mut buf);
        do_copy(
            &mut buf,
            core::hint::black_box(3),
            core::hint::black_box(3),
            core::hint::black_box(9),
        );
        mismatches += count_mismatches(&buf, 3, 3, 9);

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_overlapping_aligned_u32s", main_fn);
}
