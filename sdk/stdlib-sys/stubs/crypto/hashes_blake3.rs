use core::ffi::c_void;

/// Stubs for std::crypto::hashes::blake3.
define_stub! {
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
    );
}

define_stub! {
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
    );
}
