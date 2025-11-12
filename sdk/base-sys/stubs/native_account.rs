use core::ffi::c_void;

#[export_name = "miden::native_account::add_asset"]
pub extern "C" fn native_account_add_asset_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::remove_asset"]
pub extern "C" fn native_account_remove_asset_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::incr_nonce"]
pub extern "C" fn native_account_incr_nonce_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::compute_delta_commitment"]
pub extern "C" fn native_account_compute_delta_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::set_item"]
pub extern "C" fn native_account_set_item_plain(
    _index: f32,
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::set_map_item"]
pub extern "C" fn native_account_set_map_item_plain(
    _index: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::native_account::was_procedure_called"]
pub extern "C" fn native_account_was_procedure_called_plain(
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
