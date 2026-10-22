//! Typed SDK attachment tests against the transaction kernel and protocol attachment model.

use miden_client::account::{AccountComponent, component::InitStorageData};
use miden_core::{Felt, crypto::hash::Poseidon2};
use miden_protocol::{
    Word,
    account::auth::AuthScheme,
    errors::tx_kernel::{
        ERR_OUTPUT_NOTE_IS_SEALED, ERR_OUTPUT_NOTE_TOO_MANY_ATTACHMENTS,
        ERR_OUTPUT_NOTE_TOTAL_ATTACHMENT_WORDS_EXCEEDED,
    },
    note::{NoteAttachment, NoteAttachmentScheme, NoteAttachments},
};
use miden_testing::{Auth, MockChain, MockTransaction};

use super::support::{
    compile_rust_package, execute_tx, execute_tx_expect_failure,
    transaction_script_from_package_with_deps,
};

/// Scheme shared by the guest's duplicate attachments.
const SCHEME: u16 = 42;

/// Returns the guest's first content word.
fn first_word() -> Word {
    Word::from([Felt::ONE, Felt::from(2u32), Felt::from(3u32), Felt::from(4u32)])
}

/// Builds a transaction running the selected attachment scenario on a native wallet account.
fn build_attachment_tx(scenario: u32) -> (MockChain, MockTransaction) {
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package =
        compile_rust_package("../fixtures/components/note-attachment-tx-script", true);
    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let account_id = account.id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let advice_content: Vec<_> = (9u32..=16).map(Felt::from).collect();
    let advice_commitment = Poseidon2::hash_elements(&advice_content);
    let mock_tx = chain
        .build_transaction(account_id)
        .tx_script(transaction_script_from_package_with_deps(
            &tx_script_package,
            &[note_package.as_ref()],
        ))
        .tx_script_args(Word::from([
            Felt::from(scenario),
            Felt::from(SCHEME),
            Felt::ZERO,
            Felt::ZERO,
        ]))
        .add_advice_map_entry(advice_commitment, advice_content)
        .build()
        .unwrap();

    (chain, mock_tx)
}

/// Typed word/slice paths and the legacy advice writer preserve the protocol's commitments.
#[test]
fn typed_attachments_round_trip_after_note_constructor() {
    let (mut chain, mock_tx) = build_attachment_tx(0);
    let executed_tx = execute_tx(&mut chain, mock_tx);
    assert_eq!(executed_tx.output_notes().num_notes(), 1);

    let scheme = NoteAttachmentScheme::new(SCHEME).unwrap();
    let single = NoteAttachment::with_word(scheme, first_word());
    let second =
        Word::from([Felt::from(5u32), Felt::from(6u32), Felt::from(7u32), Felt::from(8u32)]);
    let multiple = NoteAttachment::with_words(scheme, vec![first_word(), second]).unwrap();
    let legacy = NoteAttachment::with_words(
        scheme,
        vec![
            Word::from([Felt::from(9u32), Felt::from(10u32), Felt::from(11u32), Felt::from(12u32)]),
            Word::from([
                Felt::from(13u32),
                Felt::from(14u32),
                Felt::from(15u32),
                Felt::from(16u32),
            ]),
        ],
    )
    .unwrap();
    let expected = NoteAttachments::new(vec![single.clone(), single, multiple, legacy]).unwrap();
    let actual = executed_tx.output_notes().get_note(0).attachments();
    assert_eq!(actual, &expected);
    assert_eq!(actual.to_commitment(), expected.to_commitment());
}

/// Per-note count and aggregate-content limits accept their exact protocol boundaries.
#[test]
fn typed_attachments_accept_four_attachments_and_512_words() {
    for (scenario, count, words) in [(1, 4, 1), (3, 2, 256)] {
        let (mut chain, mock_tx) = build_attachment_tx(scenario);
        let executed_tx = execute_tx(&mut chain, mock_tx);
        assert_eq!(executed_tx.output_notes().num_notes(), 1);
        let attachment = NoteAttachment::with_words(
            NoteAttachmentScheme::new(SCHEME).unwrap(),
            vec![first_word(); words],
        )
        .unwrap();
        let expected = NoteAttachments::new(vec![attachment; count]).unwrap();
        assert_eq!(executed_tx.output_notes().get_note(0).attachments(), &expected);
    }
}

/// Kernel failures identify aggregate limits and sealing rather than a guest-side panic.
#[test]
fn typed_attachments_reject_excess_count_total_words_and_sealed_notes() {
    for (scenario, expected_error) in [
        (2, ERR_OUTPUT_NOTE_TOO_MANY_ATTACHMENTS.message()),
        (4, ERR_OUTPUT_NOTE_TOTAL_ATTACHMENT_WORDS_EXCEEDED.message()),
        (5, ERR_OUTPUT_NOTE_IS_SEALED.message()),
    ] {
        let (_chain, mock_tx) = build_attachment_tx(scenario);
        let err = execute_tx_expect_failure(mock_tx);
        let needle = format!("assertion failed with error message: {expected_error}");
        assert!(err.contains(&needle), "scenario {scenario}: wanted `{needle}`, got `{err}`");
    }
}
