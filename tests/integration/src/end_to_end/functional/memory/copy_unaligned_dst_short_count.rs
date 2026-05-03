use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_unaligned_dst_short_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 8], src: &[u8; 16]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr().add(2);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 3);
            }
        }

        let mut src = [0u8; 16];
        let mut i = 0usize;
        while i < 16 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0xffu8; 8];
        do_copy(&mut dst, &src);

        let expected = [0xffu8, 0xff, 3, 4, 5, 0xff, 0xff, 0xff];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 8 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_unaligned_dst_short_count_u8s", main_fn);
}
