// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Asset, Felt, StorageMap, StorageMapAccess, Value, ValueAccess, Word, component};

use crate::bindings::exports::miden::storage_example::*;

miden::generate!();
bindings::export!(MyAccount);

/// An example account demonstrating storage value and map usage.
#[component]
struct MyAccount {
    /// Public key authorized to update the stored asset quantities.
    #[storage(description = "owner public key")]
    owner_public_key: Value,

    /// A map from asset identifier to quantity held by the account.
    #[storage(description = "asset quantity map")]
    asset_qty_map: StorageMap,
}

impl foo::Guest for MyAccount {
    /// Sets the quantity for `asset` if `pub_key` matches the stored owner key.
    fn set_asset_qty(pub_key: Word, asset: Asset, qty: Felt) {
        let mut my_account = MyAccount::default();
        let owner_key: Word = my_account.owner_public_key.read();
        if pub_key == owner_key {
            let new_value_word = Word::new([qty, Felt::ZERO, Felt::ZERO, Felt::ZERO]);
            my_account.asset_qty_map.set(asset.into(), new_value_word);
        }
    }

    /// Returns the stored quantity for `asset`, or 0 if not present.
    fn get_asset_qty(asset: Asset) -> Felt {
        let my_account = MyAccount::default();
        let word: Word = my_account.asset_qty_map.get(&asset);
        word[3]
    }
}
