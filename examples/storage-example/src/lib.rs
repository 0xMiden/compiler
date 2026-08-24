// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Asset, AssetAmount, StorageMap, StorageValue, Word, component, component_storage};

/// An example account demonstrating storage value and map usage.
#[component_storage]
struct MyAccount {
    /// Public key authorized to update the stored asset quantities.
    #[storage(description = "owner public key")]
    owner_public_key: StorageValue<Word>,

    /// A map from asset vault key to quantity held by the account.
    #[storage(description = "asset quantity map")]
    asset_qty_map: StorageMap<Word, AssetAmount>,
}

/// API of the storage example account component.
#[component]
trait Foo {
    /// Sets the quantity for `asset` if `pub_key` matches the stored owner key.
    #[account_procedure]
    fn set_asset_qty(&mut self, pub_key: Word, asset: Asset, qty: AssetAmount);

    /// Returns the stored quantity for `asset`, or 0 if not present.
    #[account_procedure]
    fn get_asset_qty(&self, asset: Asset) -> AssetAmount;
}

#[component]
impl Foo for MyAccount {
    /// Sets the quantity for `asset` if `pub_key` matches the stored owner key.
    fn set_asset_qty(&mut self, pub_key: Word, asset: Asset, qty: AssetAmount) {
        let owner_key: Word = self.owner_public_key.get();
        if pub_key == owner_key {
            self.asset_qty_map.set(asset.key, qty);
        }
    }

    /// Returns the stored quantity for `asset`, or 0 if not present.
    fn get_asset_qty(&self, asset: Asset) -> AssetAmount {
        self.asset_qty_map.get(asset.key)
    }
}
