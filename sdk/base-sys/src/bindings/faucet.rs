use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::faucet::create_fungible_asset"]
    pub fn extern_faucet_create_fungible_asset(amount: Felt, ptr: *mut Asset);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::faucet::create_non_fungible_asset"]
    pub fn extern_faucet_create_non_fungible_asset(
        data_hash_0: Felt,
        data_hash_1: Felt,
        data_hash_2: Felt,
        data_hash_3: Felt,
        ptr: *mut Asset,
    );
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
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::faucet::has_callbacks"]
    pub fn extern_faucet_has_callbacks() -> Felt;
}

/// Creates a fungible asset for the faucet bound to the current transaction.
pub fn create_fungible_asset(amount: Felt) -> Asset {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_faucet_create_fungible_asset(amount, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Creates a non-fungible asset for the faucet bound to the current transaction.
pub fn create_non_fungible_asset(data_hash: Word) -> Asset {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_faucet_create_non_fungible_asset(
            data_hash[0],
            data_hash[1],
            data_hash[2],
            data_hash[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
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

/// Returns whether the current faucet has custom mint or burn callbacks.
#[inline]
pub fn has_callbacks() -> bool {
    unsafe { extern_faucet_has_callbacks() != Felt::new(0).unwrap() }
}
