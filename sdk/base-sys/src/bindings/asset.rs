use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::asset::create_fungible_asset"]
    pub fn extern_asset_create_fungible_asset(
        faucet_id_prefix: Felt,
        faucet_id_suffix: Felt,
        amount: Felt,
        ptr: *mut Asset,
    );

    #[link_name = "miden::protocol::asset::create_non_fungible_asset"]
    pub fn extern_asset_create_non_fungible_asset(
        faucet_id_prefix: Felt,
        faucet_id_suffix: Felt,
        data_hash_0: Felt,
        data_hash_1: Felt,
        data_hash_2: Felt,
        data_hash_3: Felt,
        ptr: *mut Asset,
    );
}

/// Creates a fungible asset for the faucet identified by `faucet_id` and the provided `amount`.
pub fn create_fungible_asset(faucet_id: AccountId, amount: Felt) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_asset_create_fungible_asset(
            faucet_id.prefix,
            faucet_id.suffix,
            amount,
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Creates a non-fungible asset for the faucet identified by `faucet_id` and the provided
/// `data_hash`.
pub fn create_non_fungible_asset(faucet_id: AccountId, data_hash: Word) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_asset_create_non_fungible_asset(
            faucet_id.prefix,
            faucet_id.suffix,
            data_hash[0],
            data_hash[1],
            data_hash[2],
            data_hash[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}
