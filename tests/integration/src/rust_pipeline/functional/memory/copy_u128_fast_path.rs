use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_u128_fast_path() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u128; 2], src: &[u128; 3]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 2);
            }
        }

        let src = [
            0x00112233445566778899aabbccddeeff_u128,
            0x102132435465768798a9bacbdcedfe0f_u128,
            0xfedcba98765432100123456789abcdef_u128,
        ];
        let mut dst = [0u128; 2];
        do_copy(&mut dst, &src);

        let expected = [src[1], src[2]];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 2 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_u128_fast_path", main_fn);
}
