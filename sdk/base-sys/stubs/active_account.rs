use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::active_account::get_id")]
pub extern "C" fn active_account_get_id_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_nonce")]
pub extern "C" fn active_account_get_nonce_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_commitment")]
pub extern "C" fn active_account_get_initial_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::compute_commitment")]
pub extern "C" fn active_account_compute_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_code_commitment")]
pub extern "C" fn active_account_get_code_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_storage_commitment")]
pub extern "C" fn active_account_get_initial_storage_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::compute_storage_commitment")]
pub extern "C" fn active_account_compute_storage_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_item")]
pub extern "C" fn active_account_get_item_plain(_index_prefix: f32, _index_suffix: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_item")]
pub extern "C" fn active_account_get_initial_item_plain(
    _index_prefix: f32,
    _index_suffix: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_map_item")]
pub extern "C" fn active_account_get_map_item_plain(
    _index_prefix: f32,
    _index_suffix: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_map_item")]
pub extern "C" fn active_account_get_initial_map_item_plain(
    _index_prefix: f32,
    _index_suffix: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_balance")]
pub extern "C" fn active_account_get_balance_plain(_prefix: f32, _suffix: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_balance")]
pub extern "C" fn active_account_get_initial_balance_plain(_prefix: f32, _suffix: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::has_non_fungible_asset")]
pub extern "C" fn active_account_has_non_fungible_asset_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_initial_vault_root")]
pub extern "C" fn active_account_get_initial_vault_root_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_vault_root")]
pub extern "C" fn active_account_get_vault_root_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_num_procedures")]
pub extern "C" fn active_account_get_num_procedures_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::get_procedure_root")]
pub extern "C" fn active_account_get_procedure_root_plain(_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_account::has_procedure")]
pub extern "C" fn active_account_has_procedure_plain(
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
