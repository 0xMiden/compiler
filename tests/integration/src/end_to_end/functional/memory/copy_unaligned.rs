use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_unaligned() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 48], src: &[u8; 64]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u8; 64];
        let mut i = 0usize;
        while i < 64 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0u8; 48];
        do_copy(&mut dst, &src);

        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            if dst[i] != (i as u8).wrapping_add(3) {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_unaligned_src_len_48_u8s", main_fn);
}
