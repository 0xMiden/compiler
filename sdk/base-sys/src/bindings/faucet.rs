use miden_stdlib_sys::{Felt, Word};

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::faucet::create_fungible_asset"]
    pub fn extern_faucet_create_fungible_asset(amount: Felt, ptr: *mut Asset);

    #[link_name = "miden::protocol::faucet::create_non_fungible_asset"]
    pub fn extern_faucet_create_non_fungible_asset(
        data_hash_0: Felt,
        data_hash_1: Felt,
        data_hash_2: Felt,
        data_hash_3: Felt,
        ptr: *mut Asset,
    );

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
        ptr: *mut Word,
    );

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
        ptr: *mut Word,
    );
}

/// Creates a fungible asset for the faucet bound to the current transaction.
pub fn create_fungible_asset(amount: Felt) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_create_fungible_asset(amount, ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}

/// Creates a non-fungible asset for the faucet bound to the current transaction.
pub fn create_non_fungible_asset(data_hash: Word) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_create_non_fungible_asset(
            data_hash[0],
            data_hash[1],
            data_hash[2],
            data_hash[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Mints the provided asset for the faucet bound to the current transaction and returns the new
/// asset value.
pub fn mint_value(asset: Asset) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_faucet_mint(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Burns the provided asset from the faucet bound to the current transaction and returns the
/// resulting asset value.
pub fn burn_value(asset: Asset) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_faucet_burn(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Mints the provided asset for the faucet bound to the current transaction.
#[inline]
pub fn mint(asset: Asset) -> Asset {
    Asset::new(asset.key, mint_value(asset))
}

/// Burns the provided asset from the faucet bound to the current transaction.
#[inline]
pub fn burn(asset: Asset) -> Asset {
    Asset::new(asset.key, burn_value(asset))
}
