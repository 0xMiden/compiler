// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
extern crate alloc;
use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use bindings::exports::miden::basic_wallet::*;

bindings::export!(MyAccount with_types_in bindings);

mod bindings;

use miden::{blake3_hash_1to1, felt, Asset, Felt, NoteType, Recipient, Tag};

struct MyAccount;

impl basic_wallet::Guest for MyAccount {
    fn receive_asset(asset: Asset) {
        miden::account::add_asset(asset);
    }

    fn send_asset(asset: Asset, tag: Tag, note_type: NoteType, recipient: Recipient) {
        let asset = miden::account::remove_asset(asset);
        miden::tx::create_note(asset, tag, note_type, recipient);
    }
}

impl aux::Guest for MyAccount {
    fn test_felt_intrinsics(a: Felt, b: Felt) -> Felt {
        a + b
    }

    fn test_stdlib(input: Vec<u8>) -> Vec<u8> {
        let input: [u8; 32] = input.try_into().unwrap();
        blake3_hash_1to1(input).to_vec()
    }

    fn process_list_felt(input: Vec<Felt>) -> Vec<Felt> {
        // input.into_iter().map(|felt| felt + felt!(1)).collect()
        todo!()
    }

    fn process_core_asset(input: Asset) -> CoreAsset {
        todo!()
    }
}
