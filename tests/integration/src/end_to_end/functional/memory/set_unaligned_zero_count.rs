use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_set_unaligned_zero_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_set(dst: &mut [u8; 11]) {
            unsafe {
                let dst_ptr = dst.as_mut_ptr().add(3);
                core::ptr::write_bytes(dst_ptr, 0x5a, 0);
            }
        }

        let mut dst = [0xffu8; 11];
        do_set(&mut dst);

        let expected = [0xffu8; 11];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 11 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_set_unaligned_zero_count_u8s", main_fn);
}
