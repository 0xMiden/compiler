use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_multiword_fast_path() {
    let main_fn = r#"() -> Felt {
        struct Chunk([u128; 2]);

        #[inline(never)]
        fn do_copy(dst: &mut [Chunk; 1], src: &[Chunk; 2]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 1);
            }
        }

        let src = [
            Chunk([
                0x00112233445566778899aabbccddeeff_u128,
                0x112233445566778899aabbccddeeff00_u128,
            ]),
            Chunk([
                0xaabbccddeeff00112233445566778899_u128,
                0xffeeddccbbaa99887766554433221100_u128,
            ]),
        ];
        let mut dst = [Chunk([0u128; 2])];
        do_copy(&mut dst, &src);

        let expected = &src[1].0;
        let observed = &dst[0].0;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 2 {
            if observed[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_multiword_fast_path", main_fn);
}
