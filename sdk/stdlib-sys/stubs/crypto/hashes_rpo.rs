use core::ffi::c_void;

/// Stub for std::crypto::hashes::poseidon2::hash_elements.
define_stub! {
    #[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::hash_elements")]
    pub extern "C" fn rpo_hash_elements_stub(
        ptr: u32,
        num_elements: u32,
        result_ptr: *mut c_void,
    );
}

/// Stub for std::crypto::hashes::poseidon2::hash_words.
define_stub! {
    #[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::hash_words")]
    pub extern "C" fn rpo_hash_words_stub(
        start_addr: u32,
        end_addr: u32,
        result_ptr: *mut c_void,
    );
}

/// Stub for std::crypto::hashes::poseidon2::merge.
///
/// The ABI maps this to a function which consumes 8 felts (two digests) and returns a 4-felt
/// digest. In Rust bindings, the return value is passed back via a pointer to a result area.
define_stub! {
    #[unsafe(export_name = "miden::core::crypto::hashes::poseidon2::merge")]
    pub extern "C" fn poseidon2_merge_stub(
        b0: f32,
        b1: f32,
        b2: f32,
        b3: f32,
        a0: f32,
        a1: f32,
        a2: f32,
        a3: f32,
        result_ptr: *mut c_void,
    );
}
