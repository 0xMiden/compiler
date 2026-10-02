use super::support::assert_memory_test_returns_zero;

/// Checks that `core::ptr::copy` within one byte buffer, with overlapping ranges at unaligned
/// offsets, produces memmove results in both directions.
///
/// Regression test for #1418.
#[test]
fn memory_copy_overlapping_unaligned() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(buf: &mut [u8; 64], src_off: usize, dst_off: usize, len: usize) {
            unsafe {
                let p = buf.as_mut_ptr();
                core::ptr::copy(p.add(src_off), p.add(dst_off), len);
            }
        }

        #[inline(never)]
        fn fill(buf: &mut [u8; 64]) {
            let mut i = 0usize;
            while i < 64 {
                buf[i] = (i as u8).wrapping_mul(7).wrapping_add(3);
                i += 1;
            }
        }

        #[inline(never)]
        fn count_mismatches(buf: &[u8; 64], src_off: usize, dst_off: usize, len: usize) -> u32 {
            let mut mismatches = 0u32;
            let mut i = 0usize;
            while i < 64 {
                let source = if i >= dst_off && i < dst_off + len { i - dst_off + src_off } else { i };
                if buf[i] != (source as u8).wrapping_mul(7).wrapping_add(3) {
                    mismatches += 1;
                }
                i += 1;
            }
            mismatches
        }

        let mut mismatches = 0u32;
        let mut buf = [0u8; 64];

        fill(&mut buf);
        do_copy(
            &mut buf,
            core::hint::black_box(3),
            core::hint::black_box(5),
            core::hint::black_box(41),
        );
        mismatches += count_mismatches(&buf, 3, 5, 41);

        fill(&mut buf);
        do_copy(
            &mut buf,
            core::hint::black_box(5),
            core::hint::black_box(3),
            core::hint::black_box(41),
        );
        mismatches += count_mismatches(&buf, 5, 3, 41);

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_overlapping_unaligned_u8s", main_fn);
}
