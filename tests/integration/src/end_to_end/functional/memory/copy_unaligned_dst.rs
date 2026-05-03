use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_unaligned_dst() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 53], src: &[u8; 64]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr().add(5);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u8; 64];
        let mut i = 0usize;
        while i < 64 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0xffu8; 53];
        do_copy(&mut dst, &src);

        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 53 {
            let expected = if i < 5 { 0xff } else { (i as u8).wrapping_sub(2) };
            if dst[i] != expected {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_unaligned_dst_len_48_u8s", main_fn);
}
