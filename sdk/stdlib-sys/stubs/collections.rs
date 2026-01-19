use core::ffi::c_void;

/// Unreachable stubs for std::collections::smt procedures used via the SDK

#[unsafe(export_name = "std::collections::smt::get")]
pub extern "C" fn std_collections_smt_get_stub(
    k0: f32,
    k1: f32,
    k2: f32,
    k3: f32,
    r0: f32,
    r1: f32,
    r2: f32,
    r3: f32,
    result_ptr: *mut c_void,
) {
    let _ = (k0, k1, k2, k3, r0, r1, r2, r3, result_ptr);
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "std::collections::smt::set")]
pub extern "C" fn std_collections_smt_set_stub(
    v0: f32,
    v1: f32,
    v2: f32,
    v3: f32,
    k0: f32,
    k1: f32,
    k2: f32,
    k3: f32,
    r0: f32,
    r1: f32,
    r2: f32,
    r3: f32,
    result_ptr: *mut c_void,
) {
    let _ = (v0, v1, v2, v3, k0, k1, k2, k3, r0, r1, r2, r3, result_ptr);
    unsafe { core::hint::unreachable_unchecked() }
}
