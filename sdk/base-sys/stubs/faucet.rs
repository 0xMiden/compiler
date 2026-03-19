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
pub extern "C" fn faucet_mint_plain(
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

#[unsafe(export_name = "miden::protocol::faucet::burn")]
pub extern "C" fn faucet_burn_plain(
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
