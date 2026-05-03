use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

#[test]
fn test_memory_copy_aligned_bytes() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u32; 12], src: &[u32; 16]) {
            unsafe {
                let src_ptr = (src.as_ptr() as *const u8).add(4);
                let dst_ptr = dst.as_mut_ptr() as *mut u8;
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u32; 16];
        let src_bytes = src.as_mut_ptr() as *mut u8;
        let mut i = 0usize;
        while i < 64 {
            unsafe { *src_bytes.add(i) = i as u8; }
            i += 1;
        }

        let mut dst = [0u32; 12];
        do_copy(&mut dst, &src);

        let dst_bytes = dst.as_ptr() as *const u8;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            let observed = unsafe { *dst_bytes.add(i) };
            if observed != (i as u8).wrapping_add(4) {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_aligned_bytes_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_u128_fast_path",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_multiword_fast_path",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_aligned_addresses_misaligned_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_src_len_48_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_dst_len_48_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_dst_short_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_unaligned_zero_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 8], src: &[u8; 16]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr().add(2);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 0);
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

        let expected = [0xffu8; 8];
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

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_zero_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}
