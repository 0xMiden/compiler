use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::types::{AccountId, Asset};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::asset::create_fungible_asset"]
    pub fn extern_asset_create_fungible_asset(
        enable_callbacks: Felt,
        faucet_id_suffix: Felt,
        faucet_id_prefix: Felt,
        amount: Felt,
        ptr: *mut Asset,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::asset::create_non_fungible_asset"]
    pub fn extern_asset_create_non_fungible_asset(
        enable_callbacks: Felt,
        faucet_id_suffix: Felt,
        faucet_id_prefix: Felt,
        data_hash_0: Felt,
        data_hash_1: Felt,
        data_hash_2: Felt,
        data_hash_3: Felt,
        ptr: *mut Asset,
    );
}

#[inline]
fn callback_flag(enable_callbacks: bool) -> Felt {
    Felt::from_u32(enable_callbacks as u32)
}

/// Creates a fungible asset for the faucet identified by `faucet_id` and the provided `amount`.
pub fn create_fungible_asset(faucet_id: AccountId, amount: Felt, enable_callbacks: bool) -> Asset {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_asset_create_fungible_asset(
            callback_flag(enable_callbacks),
            faucet_id.suffix,
            faucet_id.prefix,
            amount,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}

/// Creates a non-fungible asset for the faucet identified by `faucet_id` and the provided
/// `data_hash`.
pub fn create_non_fungible_asset(
    faucet_id: AccountId,
    data_hash: Word,
    enable_callbacks: bool,
) -> Asset {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_asset_create_non_fungible_asset(
            callback_flag(enable_callbacks),
            faucet_id.suffix,
            faucet_id.prefix,
            data_hash[0],
            data_hash[1],
            data_hash[2],
            data_hash[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}
