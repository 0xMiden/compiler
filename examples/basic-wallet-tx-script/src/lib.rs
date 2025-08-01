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

use bindings::{exports::miden::base::script::Guest, miden::basic_wallet::basic_wallet};
use miden::*;

struct BasicWalletTxScript;

impl Guest for BasicWalletTxScript {
    fn script(arg: Word) {
        let num_felts = adv_push_mapvaln(arg.clone());
        let num_felts_u64 = num_felts.as_u64();
        assert_eq(Felt::from_u32((num_felts_u64 % 4) as u32), felt!(0));
        let num_words = Felt::from_u64_unchecked(num_felts_u64 / 4);
        let commitment = arg;
        let vals = adv_load_preimage(num_words, commitment);

        let tag = vals[0];
        let aux = vals[1];
        let note_type = vals[2];
        let execution_hint = vals[3];
        let recipient: [Felt; 4] = vals[4..8].try_into().unwrap();
        let assets_len = vals[8].as_u64() as usize;
        let mut assets_sent = 0;
        if assets_sent != assets_len {
            // TODO: how does one invoke create_note? I see it's "call'd" in the `miden-base` codebase
            // if it's invoked from the transaction script and "exec'd" if from the account.
            let note_idx = miden::tx::create_note(
                tag.into(),
                aux,
                note_type.into(),
                execution_hint,
                recipient.into(),
            );
            while assets_sent < assets_len {
                let start = 9 + assets_sent * 4;
                let next = start + 4;
                let asset: [Felt; 4] = vals[start..next].try_into().unwrap();
                basic_wallet::move_asset_to_note(asset.into(), note_idx);
                assets_sent += 1;
            }
        }
    }
}
