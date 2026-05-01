use core::ffi::c_void;

define_stub! {
    #[unsafe(export_name = "miden::protocol::native_account::add_asset")]
    pub extern "C" fn native_account_add_asset_plain(
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
    #[unsafe(export_name = "miden::protocol::native_account::remove_asset")]
    pub extern "C" fn native_account_remove_asset_plain(
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
    #[unsafe(export_name = "miden::protocol::native_account::incr_nonce")]
    pub extern "C" fn native_account_incr_nonce_plain() -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::native_account::compute_delta_commitment")]
    pub extern "C" fn native_account_compute_delta_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::native_account::set_item")]
    pub extern "C" fn native_account_set_item_plain(
        index_suffix: f32,
        index_prefix: f32,
        v0: f32,
        v1: f32,
        v2: f32,
        v3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::native_account::set_map_item")]
    pub extern "C" fn native_account_set_map_item_plain(
        index_suffix: f32,
        index_prefix: f32,
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
    #[unsafe(export_name = "miden::protocol::native_account::was_procedure_called")]
    pub extern "C" fn native_account_was_procedure_called_plain(
        r0: f32,
        r1: f32,
        r2: f32,
        r3: f32,
    ) -> f32;
}
