// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
//
// extern crate alloc;
// use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

bindings::export!(BasicWalletTxScript with_types_in bindings);

mod bindings;

use bindings::{exports::miden::base::transaction_script::Guest, miden::basic_wallet::basic_wallet};
use miden::{intrinsics::advice::adv_push_mapvaln, *};

struct BasicWalletTxScript;

impl Guest for BasicWalletTxScript {
    fn run(arg: Word) {
        let num_felts = adv_push_mapvaln(arg.clone());
        let num_felts_u64 = num_felts.as_u64();
        assert_eq(Felt::from_u32((num_felts_u64 % 4) as u32), felt!(0));
        let num_words = Felt::from_u64_unchecked(num_felts_u64 / 4);
        let commitment = arg;
        let input = adv_load_preimage(num_words, commitment);
        let tag = input[0];
        let aux = input[1];
        let note_type = input[2];
        let execution_hint = input[3];
        let recipient: [Felt; 4] = input[4..8].try_into().unwrap();
        let note_idx = miden::tx::create_note(
            tag.into(),
            aux,
            note_type.into(),
            execution_hint,
            recipient.into(),
        );
        let asset: [Felt; 4] = input[8..12].try_into().unwrap();
        basic_wallet::move_asset_to_note(asset.into(), note_idx);
    }
}
