use core::ffi::c_void;

define_stub! {
    #[unsafe(export_name = "miden::protocol::faucet::create_fungible_asset")]
    pub extern "C" fn faucet_create_fungible_asset_plain(amount: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::faucet::create_non_fungible_asset")]
    pub extern "C" fn faucet_create_non_fungible_asset_plain(
        h0: f32,
        h1: f32,
        h2: f32,
        h3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::faucet::mint")]
    pub extern "C" fn faucet_mint_plain(
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        v0: f32,
        v1: f32,
        v2: f32,
        v3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::faucet::burn")]
    pub extern "C" fn faucet_burn_plain(
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        v0: f32,
        v1: f32,
        v2: f32,
        v3: f32,
        out: *mut c_void,
    );
}
