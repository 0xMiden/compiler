// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
extern crate alloc;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

mod bindings;

use bindings::exports::miden::basic_wallet::*;
use miden::NoteIdx;

bindings::export!(MyAccount with_types_in bindings);

use miden::{component, Asset};

#[component]
struct MyAccount;

impl basic_wallet::Guest for MyAccount {
    fn receive_asset(asset: Asset) {
        miden::account::add_asset(asset);
    }

    fn move_asset_to_note(asset: Asset, note_idx: NoteIdx) {
        let asset = miden::account::remove_asset(asset);
        miden::tx::add_asset_to_note(asset, note_idx);
    }
}
