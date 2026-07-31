use miden_stdlib_sys::Felt;

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::faucet::mint"]
    pub fn extern_faucet_mint(
        asset_key_0: Felt,
        asset_key_1: Felt,
        asset_key_2: Felt,
        asset_key_3: Felt,
        asset_value_0: Felt,
        asset_value_1: Felt,
        asset_value_2: Felt,
        asset_value_3: Felt,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::faucet::burn"]
    pub fn extern_faucet_burn(
        asset_key_0: Felt,
        asset_key_1: Felt,
        asset_key_2: Felt,
        asset_key_3: Felt,
        asset_value_0: Felt,
        asset_value_1: Felt,
        asset_value_2: Felt,
        asset_value_3: Felt,
    );
}

/// Mints the provided asset for the faucet bound to the current transaction.
pub fn mint(asset: Asset) {
    unsafe {
        extern_faucet_mint(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
        );
    }
}

/// Burns the provided asset from the faucet bound to the current transaction.
pub fn burn(asset: Asset) {
    unsafe {
        extern_faucet_burn(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
        );
    }
}
