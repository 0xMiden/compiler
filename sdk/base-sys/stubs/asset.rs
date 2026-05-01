use core::ffi::c_void;

define_stub! {
    #[unsafe(export_name = "miden::protocol::asset::create_fungible_asset")]
    pub extern "C" fn asset_create_fungible_asset_plain(
        prefix: f32,
        suffix: f32,
        amount: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::asset::create_non_fungible_asset")]
    pub extern "C" fn asset_create_non_fungible_asset_plain(
        prefix: f32,
        suffix: f32,
        h0: f32,
        h1: f32,
        h2: f32,
        h3: f32,
        out: *mut c_void,
    );
}
