use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::asset::id_into_faucet_id")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn asset_id_into_faucet_id_plain(
    _asset_id_0: f32,
    _asset_id_1: f32,
    _asset_id_2: f32,
    _asset_id_3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::asset::id_into_asset_class")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn asset_id_into_asset_class_plain(
    _asset_id_0: f32,
    _asset_id_1: f32,
    _asset_id_2: f32,
    _asset_id_3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::asset::id_into_composition")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn asset_id_into_composition_plain(
    _asset_id_0: f32,
    _asset_id_1: f32,
    _asset_id_2: f32,
    _asset_id_3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}
