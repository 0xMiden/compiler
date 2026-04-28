use core::ffi::c_void;

/// Stub for intrinsics::crypto::hmerge.
///
/// Signature in Wasm is (i32 digests_ptr, i32 result_ptr).
define_stub! {
    #[unsafe(export_name = "intrinsics::crypto::hmerge")]
    pub extern "C" fn hmerge_stub(digests_ptr: *const c_void, result_ptr: *mut c_void);
}
