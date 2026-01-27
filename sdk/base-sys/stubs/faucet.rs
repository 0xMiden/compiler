use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::faucet::create_fungible_asset")]
pub extern "C" fn faucet_create_fungible_asset_plain(_amount: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::faucet::create_non_fungible_asset")]
pub extern "C" fn faucet_create_non_fungible_asset_plain(
    _h0: f32,
    _h1: f32,
    _h2: f32,
    _h3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::faucet::mint")]
pub extern "C" fn faucet_mint_plain(_a0: f32, _a1: f32, _a2: f32, _a3: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::faucet::burn")]
pub extern "C" fn faucet_burn_plain(_a0: f32, _a1: f32, _a2: f32, _a3: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::faucet::get_total_issuance")]
pub extern "C" fn faucet_get_total_issuance_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::faucet::is_non_fungible_asset_issued")]
pub extern "C" fn faucet_is_non_fungible_asset_issued_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
