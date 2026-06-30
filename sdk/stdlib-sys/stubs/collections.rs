use core::ffi::c_void;

/// Unreachable stubs for std::collections::smt procedures used via the SDK

#[unsafe(export_name = "miden::core::collections::smt::get")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn std_collections_smt_get_stub(
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
    _result_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::core::collections::smt::set")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn std_collections_smt_set_stub(
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
    _result_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
