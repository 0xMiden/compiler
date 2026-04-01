use miden_stdlib_sys::{Felt, Word};

use super::types::Asset;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::native_account::add_asset"]
    fn extern_native_account_add_asset(
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
    #[link_name = "miden::protocol::native_account::remove_asset"]
    fn extern_native_account_remove_asset(
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
    #[link_name = "miden::protocol::native_account::incr_nonce"]
    fn extern_native_account_incr_nonce() -> Felt;
    #[link_name = "miden::protocol::native_account::compute_delta_commitment"]
    fn extern_native_account_compute_delta_commitment(ptr: *mut Word);
    #[link_name = "miden::protocol::native_account::was_procedure_called"]
    fn extern_native_account_was_procedure_called(
        proc_root_0: Felt,
        proc_root_1: Felt,
        proc_root_2: Felt,
        proc_root_3: Felt,
    ) -> Felt;
}

/// Adds the specified asset to the vault and returns the resulting asset value word stored under
/// that asset key.
///
/// Panics:
/// - If the asset is not valid.
/// - If the total value of two fungible assets is greater than or equal to 2^63.
/// - If the vault already contains the same non-fungible asset.
///
/// # Examples
///
/// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
///
/// ```rust,ignore
/// use miden::{component, native_account::NativeAccount, Asset};
///
/// #[component]
/// struct MyAccount;
///
/// #[component]
/// impl MyAccount {
///     pub fn receive_asset(&mut self, asset: Asset) {
///         self.add_asset(asset);
///     }
/// }
/// ```
pub fn add_asset(asset: Asset) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_native_account_add_asset(
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

/// Removes the specified asset from the vault and returns the resulting asset value word.
///
/// Panics:
/// - The fungible asset is not found in the vault.
/// - The amount of the fungible asset in the vault is less than the amount to be removed.
/// - The non-fungible asset is not found in the vault.
pub fn remove_asset(asset: Asset) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_native_account_remove_asset(
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

/// Increments the account nonce by one and returns the new nonce.
#[inline]
pub fn incr_nonce() -> Felt {
    unsafe { extern_native_account_incr_nonce() }
}

/// Computes and returns the commitment to the native account's delta for this transaction.
#[inline]
pub fn compute_delta_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_native_account_compute_delta_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}

/// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
#[inline]
pub fn was_procedure_called(proc_root: Word) -> bool {
    unsafe {
        extern_native_account_was_procedure_called(
            proc_root[0],
            proc_root[1],
            proc_root[2],
            proc_root[3],
        ) != Felt::new(0)
    }
}

/// Trait that provides native account operations for components.
///
/// This trait is automatically implemented for types marked with the `#[component]` macro.
pub trait NativeAccount {
    /// Adds the specified asset to the vault and returns the resulting asset value word stored
    /// under that asset key.
    ///
    /// # Panics
    ///
    /// - If the asset is not valid.
    /// - If the total value of two fungible assets is greater than or equal to 2^63.
    /// - If the vault already contains the same non-fungible asset.
    ///
    /// # Examples
    ///
    /// Implement a basic-wallet style `receive_asset` method by adding the asset to the vault:
    ///
    /// ```rust,ignore
    /// use miden::{component, native_account::NativeAccount, Asset};
    ///
    /// #[component]
    /// struct MyAccount;
    ///
    /// #[component]
    /// impl MyAccount {
    ///     pub fn receive_asset(&mut self, asset: Asset) {
    ///         self.add_asset(asset);
    ///     }
    /// }
    /// ```
    #[inline]
    fn add_asset(&mut self, asset: Asset) -> Word {
        add_asset(asset)
    }

    /// Removes the specified asset from the vault and returns the resulting asset value word.
    ///
    /// # Panics
    ///
    /// - The fungible asset is not found in the vault.
    /// - The amount of the fungible asset in the vault is less than the amount to be removed.
    /// - The non-fungible asset is not found in the vault.
    #[inline]
    fn remove_asset(&mut self, asset: Asset) -> Word {
        remove_asset(asset)
    }

    /// Increments the account nonce by one and returns the new nonce.
    #[inline]
    fn incr_nonce(&mut self) -> Felt {
        incr_nonce()
    }

    /// Computes and returns the commitment to the native account's delta for this transaction.
    #[inline]
    fn compute_delta_commitment(&self) -> Word {
        compute_delta_commitment()
    }

    /// Returns `true` if the procedure identified by `proc_root` was called during the transaction.
    #[inline]
    fn was_procedure_called(&self, proc_root: Word) -> bool {
        was_procedure_called(proc_root)
    }
}
