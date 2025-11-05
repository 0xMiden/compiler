use core::ffi::c_void;

/// Account interface stubs
///
/// Unreachable stub for `add-asset` import (extern_account_add_asset).
/// Signature matches the Wasm lowering used by the SDK: (f32, f32, f32, f32, i32)
#[export_name = "miden::account::add_asset"]
pub extern "C" fn add_asset_plain(_a0: f32, _a1: f32, _a2: f32, _a3: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::remove_asset"]
pub extern "C" fn remove_asset_plain(_a0: f32, _a1: f32, _a2: f32, _a3: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_id"]
pub extern "C" fn account_get_id_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_nonce"]
pub extern "C" fn account_get_nonce_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_initial_commitment"]
pub extern "C" fn account_get_initial_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::compute_current_commitment"]
pub extern "C" fn account_compute_current_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::compute_delta_commitment"]
pub extern "C" fn account_compute_delta_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_item"]
pub extern "C" fn account_get_item_plain(_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::set_item"]
pub extern "C" fn account_set_item_plain(
    _index: f32,
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_map_item"]
pub extern "C" fn account_get_map_item_plain(
    _index: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::set_map_item"]
pub extern "C" fn account_set_map_item_plain(
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

#[export_name = "miden::account::incr_nonce"]
pub extern "C" fn account_incr_nonce_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::account::get_balance"]
pub extern "C" fn account_get_balance_plain(_prefix: f32, _suffix: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
