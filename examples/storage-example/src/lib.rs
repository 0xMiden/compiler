// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Asset, AssetAmount, StorageMap, StorageValue, Word, component_storage, export_type};
use miden_field_repr::{FromFeltRepr, ToFeltRepr};

use crate::bindings::exports::miden::storage_example::*;

// The hand-written WIT declares the `limits` and `holding` records; map them to the Rust types
// below so the generated bindings use them.
miden::generate!(with = {
    "miden:storage-example/foo@1.0.0/limits": crate::Limits,
    "miden:storage-example/foo@1.0.0/holding": crate::Holding,
});
bindings::export!(MyAccount);

/// Limits that apply to the account's holdings.
#[export_type]
#[derive(Clone, Copy, FromFeltRepr, ToFeltRepr)]
pub struct Limits {
    /// The largest quantity of one holding.
    pub max_qty: AssetAmount,
    /// Whether the holdings are frozen.
    pub paused: bool,
}

/// The account's holding of one asset.
#[export_type]
#[derive(Clone, Copy, FromFeltRepr, ToFeltRepr)]
pub struct Holding {
    /// The quantity held.
    pub qty: AssetAmount,
    /// Whether the holding is locked.
    pub locked: bool,
}

/// An example account demonstrating storage value and map usage.
#[component_storage]
struct MyAccount {
    /// Public key authorized to update the stored asset quantities.
    #[storage(description = "owner public key")]
    owner_public_key: StorageValue<Word>,

    /// A map from asset id word to quantity held by the account.
    #[storage(description = "asset quantity map")]
    asset_qty_map: StorageMap<Word, AssetAmount>,

    /// Limits that apply to the account's holdings.
    #[storage(description = "holding limits")]
    limits: StorageValue<Limits>,

    /// A map from asset id word to the account's holding of that asset.
    #[storage(description = "holdings by asset")]
    holdings: StorageMap<Word, Holding>,
}

impl foo::Guest for MyAccount {
    /// Sets the quantity for `asset` if `pub_key` matches the stored owner key.
    fn set_asset_qty(pub_key: Word, asset: Asset, qty: AssetAmount) {
        let mut my_account = MyAccount::default();
        let owner_key: Word = my_account.owner_public_key.get();
        if pub_key == owner_key {
            my_account.asset_qty_map.set(asset.id.inner, qty);
        }
    }

    /// Returns the stored quantity for `asset`, or 0 if not present.
    fn get_asset_qty(asset: Asset) -> AssetAmount {
        let my_account = MyAccount::default();
        my_account.asset_qty_map.get(asset.id.inner)
    }

    /// Returns the stored limits.
    fn get_limits() -> Limits {
        let my_account = MyAccount::default();
        my_account.limits.get()
    }

    /// Sets the limits if `pub_key` matches the stored owner key.
    fn set_limits(pub_key: Word, limits: Limits) {
        let mut my_account = MyAccount::default();
        let owner_key: Word = my_account.owner_public_key.get();
        if pub_key == owner_key {
            my_account.limits.set(limits);
        }
    }

    /// Sets the holding of `asset` if `pub_key` matches the stored owner key.
    fn set_holding(pub_key: Word, asset: Asset, holding: Holding) {
        let mut my_account = MyAccount::default();
        let owner_key: Word = my_account.owner_public_key.get();
        if pub_key == owner_key {
            my_account.holdings.set(asset.id.inner, holding);
        }
    }

    /// Returns the holding of `asset`: zero and unlocked if not present.
    fn get_holding(asset: Asset) -> Holding {
        let my_account = MyAccount::default();
        my_account.holdings.get(asset.id.inner)
    }
}
