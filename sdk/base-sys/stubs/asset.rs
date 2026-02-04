use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::asset::build_fungible_asset")]
pub extern "C" fn asset_build_fungible_asset_plain(
    _prefix: f32,
    _suffix: f32,
    _amount: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::asset::build_non_fungible_asset")]
pub extern "C" fn asset_build_non_fungible_asset_plain(
    _prefix: f32,
    _h0: f32,
    _h1: f32,
    _h2: f32,
    _h3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
