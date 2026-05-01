use core::ffi::c_void;

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_id")]
    pub extern "C" fn active_account_get_id_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_nonce")]
    pub extern "C" fn active_account_get_nonce_plain() -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_commitment")]
    pub extern "C" fn active_account_get_initial_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::compute_commitment")]
    pub extern "C" fn active_account_compute_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_code_commitment")]
    pub extern "C" fn active_account_get_code_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_storage_commitment")]
    pub extern "C" fn active_account_get_initial_storage_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::compute_storage_commitment")]
    pub extern "C" fn active_account_compute_storage_commitment_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_asset")]
    pub extern "C" fn active_account_get_asset_plain(
        asset_key_0: f32,
        asset_key_1: f32,
        asset_key_2: f32,
        asset_key_3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_asset")]
    pub extern "C" fn active_account_get_initial_asset_plain(
        asset_key_0: f32,
        asset_key_1: f32,
        asset_key_2: f32,
        asset_key_3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_item")]
    pub extern "C" fn active_account_get_item_plain(
        index_suffix: f32,
        index_prefix: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_item")]
    pub extern "C" fn active_account_get_initial_item_plain(
        index_suffix: f32,
        index_prefix: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_map_item")]
    pub extern "C" fn active_account_get_map_item_plain(
        index_suffix: f32,
        index_prefix: f32,
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_map_item")]
    pub extern "C" fn active_account_get_initial_map_item_plain(
        index_suffix: f32,
        index_prefix: f32,
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        out: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_balance")]
    pub extern "C" fn active_account_get_balance_plain(suffix: f32, prefix: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_balance")]
    pub extern "C" fn active_account_get_initial_balance_plain(
        suffix: f32,
        prefix: f32,
    ) -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::has_non_fungible_asset")]
    pub extern "C" fn active_account_has_non_fungible_asset_plain(
        a0: f32,
        a1: f32,
        a2: f32,
        a3: f32,
    ) -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_initial_vault_root")]
    pub extern "C" fn active_account_get_initial_vault_root_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_vault_root")]
    pub extern "C" fn active_account_get_vault_root_plain(out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_num_procedures")]
    pub extern "C" fn active_account_get_num_procedures_plain() -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::get_procedure_root")]
    pub extern "C" fn active_account_get_procedure_root_plain(index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_account::has_procedure")]
    pub extern "C" fn active_account_has_procedure_plain(
        r0: f32,
        r1: f32,
        r2: f32,
        r3: f32,
    ) -> f32;
}
