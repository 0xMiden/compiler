#![no_std]

extern crate alloc;

use miden::{
    account, component, felt, hash_words, intrinsics::advice::adv_insert, tx, Felt, Value,
    ValueAccess, Word,
};

use crate::bindings::exports::miden::base::authentication_component::Guest;

miden::generate!();
bindings::export!(AuthComponent);

/// Authentication component storage/layout.
///
/// Public key is expected to be in the slot 0. Matches MASM constant `PUBLIC_KEY_SLOT=0` in
/// ../base/crates/miden-lib/asm/account_components/rpo_falcon_512.masm
#[component]
struct AuthComponent {
    /// The account owner's public key (RPO-Falcon512 public key hash).
    #[storage(
        slot(0),
        description = "owner public key",
        type = "auth::rpo_falcon512::pub_key"
    )]
    owner_public_key: Value,
}

impl Guest for AuthComponent {
    fn auth_procedure(_arg: Word) {
        let ref_block_num = tx::get_block_number();
        let final_nonce = account::incr_nonce();

        // Gather tx summary parts
        let acct_delta_commit = account::compute_delta_commitment();
        let input_notes_commit = tx::get_input_notes_commitment();
        let output_notes_commit = tx::get_output_notes_commitment();

        let salt = Word::from([felt!(0), felt!(0), ref_block_num, final_nonce]);

        let mut tx_summary = [acct_delta_commit, input_notes_commit, output_notes_commit, salt];
        let msg: Word = hash_words(&tx_summary).into();
        // On the advice stack the words are expected to be in the reverse order
        tx_summary.reverse();
        // Insert tx summary into advice map under key `msg`
        adv_insert(msg.clone(), &tx_summary);

        // Load public key from storage slot 0
        let storage = Self::default();
        let pub_key: Word = storage.owner_public_key.read();

        // Emit signature request event to advice stack,
        miden::emit_falcon_sig_to_stack(msg.clone(), pub_key.clone());

        // Verify the signature loaded on the advice stack.
        miden::rpo_falcon512_verify(pub_key, msg);
    }
}
