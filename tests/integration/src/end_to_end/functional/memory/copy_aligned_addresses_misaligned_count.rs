use super::support::assert_memory_test_returns_zero;

#[test]
fn test_memory_copy_aligned_addresses_misaligned_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u32; 12], src: &[u32; 16]) {
            unsafe {
                let src_ptr = (src.as_ptr() as *const u8).add(4);
                let dst_ptr = dst.as_mut_ptr() as *mut u8;
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 47);
            }
        }

        let mut src = [0u32; 16];
        let src_bytes = src.as_mut_ptr() as *mut u8;
        let mut i = 0usize;
        while i < 64 {
            unsafe { *src_bytes.add(i) = i as u8; }
            i += 1;
        }

        let mut dst = [0xffff_ffffu32; 12];
        do_copy(&mut dst, &src);

        let dst_bytes = dst.as_ptr() as *const u8;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            let observed = unsafe { *dst_bytes.add(i) };
            let expected = if i < 47 {
                (i as u8).wrapping_add(4)
            } else {
                0xff
            };
            if observed != expected {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    assert_memory_test_returns_zero("memory_copy_aligned_addresses_misaligned_count_u8s", main_fn);
}
