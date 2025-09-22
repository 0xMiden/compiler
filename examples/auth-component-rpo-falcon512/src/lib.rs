#![no_std]

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

bindings::export!(AuthComponent with_types_in bindings);

mod bindings;

use bindings::exports::miden::base::authentication_component::Guest;
use miden::{
    account, component, felt,
    intrinsics::crypto::{merge, Digest},
    tx, Felt, Value, ValueAccess, Word,
};

/// Authentication component storage/layout.
///
/// Public key is expected to be in the slot 0. Matches MASM constant `PUBLIC_KEY_SLOT=0` in
/// ../base/crates/miden-lib/asm/account_components/rpo_falcon_512.masm
#[component]
struct AuthStorage {
    /// The account owner's public key (RPO-Falcon512 public key hash).
    #[storage(
        slot(0),
        description = "owner public key",
        type = "auth::rpo_falcon512::pub_key"
    )]
    owner_public_key: Value,
}

struct AuthComponent;

impl Guest for AuthComponent {
    fn auth_procedure(_arg: Word) {
        // Translated from MASM at:
        // https://github.com/0xMiden/miden-base/blob/280a53f8e7dcfa98fb88e6872e6972ec45c8ccc2/crates/miden-lib/asm/miden/contracts/auth/basic.masm?plain=1#L18-L57

        // Get commitments and account context
        let out_notes: Word = tx::get_output_notes_commitment();
        let in_notes: Word = tx::get_input_notes_commitment();
        let nonce: Felt = account::get_nonce();
        let acct_id = account::get_id();

        // Compute MESSAGE = h(OUT, h(IN, h([0,0,acc_id_prefix,acc_id_suffix], [0,0,0,nonce])))
        let w_id = Word::from([felt!(0), felt!(0), acct_id.prefix, acct_id.suffix]);
        let w_nonce = Word::from([felt!(0), felt!(0), felt!(0), nonce]);
        let inner = merge([Digest::from(w_id), Digest::from(w_nonce)]);
        let mid = merge([Digest::from(in_notes), inner]);
        let msg: Word = merge([Digest::from(out_notes), mid]).into();

        // Load public key from storage slot 0
        let storage = AuthStorage::default();
        let pub_key: Word = storage.owner_public_key.read();

        account::incr_nonce(felt!(1));

        // Emit signature request event to advice stack, then verify.
        miden::emit_falcon_sig_to_stack();
        miden::rpo_falcon512_verify(pub_key, msg);
    }
}
