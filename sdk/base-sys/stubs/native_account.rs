use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::native_account::add_asset")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_add_asset_plain(
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

#[unsafe(export_name = "miden::protocol::native_account::remove_asset")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_remove_asset_plain(
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

#[unsafe(export_name = "miden::protocol::native_account::get_id")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_id_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::incr_nonce")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_incr_nonce_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::compute_delta_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_compute_delta_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::set_item")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_set_item_plain(
    _index_suffix: f32,
    _index_prefix: f32,
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::set_map_item")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_set_map_item_plain(
    _index_suffix: f32,
    _index_prefix: f32,
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

#[unsafe(export_name = "miden::protocol::native_account::was_procedure_called")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_was_procedure_called_plain(
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_storage_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_storage_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_vault_root")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_vault_root_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_asset")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_asset_plain(
    _asset_key_0: f32,
    _asset_key_1: f32,
    _asset_key_2: f32,
    _asset_key_3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_item")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_item_plain(
    _index_suffix: f32,
    _index_prefix: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::native_account::get_initial_map_item")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn native_account_get_initial_map_item_plain(
    _index_suffix: f32,
    _index_prefix: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
