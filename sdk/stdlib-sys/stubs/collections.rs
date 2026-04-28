use core::ffi::c_void;

/// Stubs for std::collections::smt procedures used via the SDK.
define_stub! {
    #[unsafe(export_name = "miden::core::collections::smt::get")]
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
    );
}

define_stub! {
    #[unsafe(export_name = "miden::core::collections::smt::set")]
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
    );
}
