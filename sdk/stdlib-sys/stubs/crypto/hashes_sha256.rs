use core::ffi::c_void;

/// Unreachable stubs for std::crypto::hashes::sha256

#[unsafe(export_name = "miden::core::crypto::hashes::sha256::hash")]
pub extern "C" fn sha256_hash_stub(
    _e1: u32,
    _e2: u32,
    _e3: u32,
    _e4: u32,
    _e5: u32,
    _e6: u32,
    _e7: u32,
    _e8: u32,
    _result_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::core::crypto::hashes::sha256::merge")]
pub extern "C" fn sha256_merge_stub(
    _e1: u32,
    _e2: u32,
    _e3: u32,
    _e4: u32,
    _e5: u32,
    _e6: u32,
    _e7: u32,
    _e8: u32,
    _e9: u32,
    _e10: u32,
    _e11: u32,
    _e12: u32,
    _e13: u32,
    _e14: u32,
    _e15: u32,
    _e16: u32,
    _result_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
