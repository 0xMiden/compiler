#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::{
    StorageValue, Word, component, component_storage, felt, hash_words, intrinsics::advice::adv_insert,
    tx,
};

/// Authentication component storage/layout.
///
/// Public key is expected to be in the slot 0. Matches MASM constant `PUBLIC_KEY_SLOT=0` in
/// ../base/crates/miden-lib/asm/account_components/rpo_falcon_512.masm
#[component_storage]
struct AuthComponentStorage {
    /// The account owner's public key (RPO-Falcon512 public key hash).
    #[storage(
        description = "owner public key",
        type = "miden::standards::auth::pub_key"
    )]
    owner_public_key: StorageValue<Word>,
}

/// API of the RPO-Falcon512 authentication component.
#[component]
trait AuthComponent {
    #[auth_script]
    fn check_signature(&mut self, _arg: Word);
}

#[component]
impl AuthComponent for AuthComponentStorage {
    fn check_signature(&mut self, _arg: Word) {
        let final_nonce = self.incr_nonce();

        // Gather tx summary parts
        let acct_delta_commit = self.compute_delta_commitment();
        let input_notes_commit = tx::get_input_notes_commitment();
        let output_notes_commit = tx::get_output_notes_commitment();
        let block_commit = tx::get_block_commitment();
        let expiration_delta = tx::get_expiration_block_delta();

        // The transaction summary binds the reference block commitment, the expiration delta,
        // and seven user parameters. As in the standards singlesig component, the first user
        // parameter carries the final nonce for replay protection and the rest are zero.
        let params_head =
            Word::from([expiration_delta.into(), final_nonce.into(), felt!(0), felt!(0)]);
        let params_tail = Word::from([felt!(0), felt!(0), felt!(0), felt!(0)]);

        let tx_summary = [
            acct_delta_commit,
            input_notes_commit,
            output_notes_commit,
            block_commit,
            params_head,
            params_tail,
        ];
        let msg: Word = hash_words(&tx_summary).into();
        // Insert tx summary into advice map under key `msg`
        adv_insert(msg, &tx_summary);

        let pub_key: Word = self.owner_public_key.get();

        // Emit signature request event to advice stack,
        miden::emit_falcon_sig_to_stack(msg, pub_key);

        // Verify the signature loaded on the advice stack.
        miden::rpo_falcon512_verify(pub_key, msg);
    }
}
