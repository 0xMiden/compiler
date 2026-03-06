use core::ffi::c_void;

/// Unreachable stub for std::crypto::hashes::poseidon2::hash_elements

#[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::hash_elements")]
pub extern "C" fn rpo_hash_elements_stub(_ptr: u32, _num_elements: u32, _result_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

/// Unreachable stub for std::crypto::hashes::poseidon2::hash_words
#[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::hash_words")]
pub extern "C" fn rpo_hash_words_stub(_start_addr: u32, _end_addr: u32, _result_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

/// Unreachable stub for std::crypto::hashes::poseidon2::merge.
///
/// The ABI maps this to a function which consumes 8 felts (two digests) and returns a 4-felt digest.
/// In Rust bindings, the return value is passed back via a pointer to a result area.
#[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::merge")]
pub extern "C" fn poseidon2_merge_stub(
    _b0: f32,
    _b1: f32,
    _b2: f32,
    _b3: f32,
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _result_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
