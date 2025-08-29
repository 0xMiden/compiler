use core::ffi::c_void;

/// Unreachable stub for intrinsics::crypto::hmerge.
/// Signature in Wasm is (i32 digests_ptr, i32 result_ptr)
#[export_name = "intrinsics::crypto::hmerge"]
pub extern "C" fn hmerge_stub(_digests_ptr: *const c_void, _result_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

/// Unreachable stub for intrinsics::felt::add.
/// Signature in Wasm is (f32, f32) -> f32
#[export_name = "intrinsics::felt::add"]
pub extern "C" fn felt_add_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
