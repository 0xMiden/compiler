use miden_stdlib_sys::{Felt, Word};

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::faucet::create_fungible_asset"]
    pub fn extern_faucet_create_fungible_asset(amount: Felt, ptr: *mut Asset);

    #[link_name = "miden::protocol::faucet::create_non_fungible_asset"]
    pub fn extern_faucet_create_non_fungible_asset(
        data_hash_3: Felt,
        data_hash_2: Felt,
        data_hash_1: Felt,
        data_hash_0: Felt,
        ptr: *mut Asset,
    );

    #[link_name = "miden::protocol::faucet::mint"]
    pub fn extern_faucet_mint(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
        ptr: *mut Asset,
    );

    #[link_name = "miden::protocol::faucet::burn"]
    pub fn extern_faucet_burn(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
        ptr: *mut Asset,
    );

    #[link_name = "miden::protocol::faucet::get_total_issuance"]
    pub fn extern_faucet_get_total_issuance() -> Felt;

    #[link_name = "miden::protocol::faucet::is_non_fungible_asset_issued"]
    pub fn extern_faucet_is_non_fungible_asset_issued(
        asset_3: Felt,
        asset_2: Felt,
        asset_1: Felt,
        asset_0: Felt,
    ) -> Felt;
}

/// Creates a fungible asset for the faucet bound to the current transaction.
pub fn create_fungible_asset(amount: Felt) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_create_fungible_asset(amount, ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Creates a non-fungible asset for the faucet bound to the current transaction.
pub fn create_non_fungible_asset(data_hash: Word) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_create_non_fungible_asset(
            data_hash[3],
            data_hash[2],
            data_hash[1],
            data_hash[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Mints the provided asset for the faucet bound to the current transaction.
pub fn mint(asset: Asset) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_mint(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Burns the provided asset from the faucet bound to the current transaction.
pub fn burn(asset: Asset) -> Asset {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Asset>::uninit();
        extern_faucet_burn(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Returns the total issuance of the faucet bound to the current transaction.
#[inline]
pub fn get_total_issuance() -> Felt {
    unsafe { extern_faucet_get_total_issuance() }
}

/// Returns `true` if the specified non-fungible `asset` has already been issued by the faucet.
#[inline]
pub fn is_non_fungible_asset_issued(asset: Asset) -> bool {
    unsafe {
        let result = extern_faucet_is_non_fungible_asset_issued(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
        );
        result != Felt::new(0)
    }
}
