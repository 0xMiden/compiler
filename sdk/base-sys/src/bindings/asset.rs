use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::asset::build_fungible_asset"]
    pub fn extern_asset_build_fungible_asset(
        faucet_id_prefix: Felt,
        faucet_id_suffix: Felt,
        amount: Felt,
        ptr: *mut Asset,
    );

    #[link_name = "miden::protocol::asset::build_non_fungible_asset"]
    pub fn extern_asset_build_non_fungible_asset(
        faucet_id_prefix: Felt,
        data_hash_3: Felt,
        data_hash_2: Felt,
        data_hash_1: Felt,
        data_hash_0: Felt,
        ptr: *mut Asset,
    );
}

/// Builds a fungible asset for the faucet identified by `faucet_id` and the provided `amount`.
pub fn build_fungible_asset(faucet_id: AccountId, amount: Felt) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_asset_build_fungible_asset(
            faucet_id.prefix,
            faucet_id.suffix,
            amount,
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reversed()
    }
}

/// Builds a non-fungible asset for the faucet identified by `faucet_id` and the provided
/// `data_hash`.
pub fn build_non_fungible_asset(faucet_id: AccountId, data_hash: Word) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_asset_build_non_fungible_asset(
            faucet_id.prefix,
            data_hash[3],
            data_hash[2],
            data_hash[1],
            data_hash[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reversed()
    }
}
