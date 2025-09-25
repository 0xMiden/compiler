#![no_std]

extern crate alloc;

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

bindings::export!(AuthComponent with_types_in bindings);

mod bindings;

use alloc::vec::Vec;

use bindings::exports::miden::base::authentication_component::Guest;
use miden::{
    account, component, felt, hash_elements, intrinsics::advice::adv_insert, tx, Felt, Value,
    ValueAccess, Word,
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
        let ref_block_num = tx::get_block_number();
        let final_nonce = account::incr_nonce();

        // Gather tx summary parts
        let acct_delta_commit = account::compute_delta_commitment();
        let input_notes_commit = tx::get_input_notes_commitment();
        let output_notes_commit = tx::get_output_notes_commitment();

        let salt = Word::from([felt!(0), felt!(0), ref_block_num, final_nonce]);

        // Build MESSAGE = hash([delta, input, output, salt])
        let mut elems: Vec<Felt> = Vec::with_capacity(16);
        let acct_delta_arr: [Felt; 4] = (&acct_delta_commit).into();
        let input_arr: [Felt; 4] = (&input_notes_commit).into();
        let output_arr: [Felt; 4] = (&output_notes_commit).into();
        let salt_arr: [Felt; 4] = (&salt).into();
        elems.extend_from_slice(&acct_delta_arr);
        elems.extend_from_slice(&input_arr);
        elems.extend_from_slice(&output_arr);
        elems.extend_from_slice(&salt_arr);
        // TODO: use `hash_memory_words` after https://github.com/0xMiden/compiler/issues/644 is
        // implemented
        let msg: Word = hash_elements(elems).into();

        adv_insert(
            msg.clone(),
            &[salt, output_notes_commit, input_notes_commit, acct_delta_commit],
        );

        // Load public key from storage slot 0
        let storage = AuthStorage::default();
        let pub_key: Word = storage.owner_public_key.read();

        // Emit signature request event to advice stack,
        miden::emit_falcon_sig_to_stack(msg.clone(), pub_key.clone());

        // Verify the signature loaded on the advice stack.
        miden::rpo_falcon512_verify(pub_key, msg);
    }
}
