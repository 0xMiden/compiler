use core::ffi::c_void;

/// Unreachable stubs for std::crypto::hashes::blake3

#[unsafe(export_name = "miden::core::crypto::hashes::blake3::hash")]
pub extern "C" fn blake3_hash_stub(
    e1: u32,
    e2: u32,
    e3: u32,
    e4: u32,
    e5: u32,
    e6: u32,
    e7: u32,
    e8: u32,
    result_ptr: *mut c_void,
) {
    let _ = (e1, e2, e3, e4, e5, e6, e7, e8, result_ptr);
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::core::crypto::hashes::blake3::merge")]
pub extern "C" fn blake3_merge_stub(
    e1: u32,
    e2: u32,
    e3: u32,
    e4: u32,
    e5: u32,
    e6: u32,
    e7: u32,
    e8: u32,
    e9: u32,
    e10: u32,
    e11: u32,
    e12: u32,
    e13: u32,
    e14: u32,
    e15: u32,
    e16: u32,
    result_ptr: *mut c_void,
) {
    let _ = (
        e1, e2, e3, e4, e5, e6, e7, e8, e9, e10, e11, e12, e13, e14, e15, e16, result_ptr,
    );
    unsafe { core::hint::unreachable_unchecked() }
}
