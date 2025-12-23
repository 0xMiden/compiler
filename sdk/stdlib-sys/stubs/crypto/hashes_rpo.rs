use core::ffi::c_void;

/// Unreachable stub for std::crypto::hashes::rpo::hash_memory

#[unsafe(export_name = "std::crypto::hashes::rpo::hash_memory")]
pub extern "C" fn rpo_hash_memory_stub(ptr: u32, num_elements: u32, result_ptr: *mut c_void) {
    let _ = (ptr, num_elements, result_ptr);
    unsafe { core::hint::unreachable_unchecked() }
}

/// Unreachable stub for std::crypto::hashes::rpo::hash_memory_words
#[unsafe(export_name = "std::crypto::hashes::rpo::hash_memory_words")]
pub extern "C" fn rpo_hash_memory_words_stub(
    start_addr: u32,
    end_addr: u32,
    result_ptr: *mut c_void,
) {
    let _ = (start_addr, end_addr, result_ptr);
    unsafe { core::hint::unreachable_unchecked() }
}
