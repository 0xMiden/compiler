//! Typed SDK attachment tests against the transaction kernel and protocol attachment model.

use std::sync::{Arc, OnceLock};

use miden_client::account::{AccountComponent, component::InitStorageData};
use miden_core::{Felt, crypto::hash::Poseidon2};
use miden_mast_package::Package;
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
    assert_failure_contains, compile_rust_package, execute_tx, execute_tx_expect_failure,
    transaction_script_from_package_with_deps,
};

/// Scheme the host passes to the guest; the round-trip scenario also uses
/// `SCHEME + 1..=SCHEME + 3`, so it must stay at most 65531.
const SCHEME: u16 = 42;

/// Returns the guest's first content word.
fn first_word() -> Word {
    Word::from([Felt::ONE, Felt::from(2u32), Felt::from(3u32), Felt::from(4u32)])
}

/// Returns the felts the host provides in the advice map for the guest's advice-map attachment.
fn advice_content() -> Vec<Felt> {
    (9u32..=16).map(Felt::from).collect()
}

/// Compiled packages used by the attachment tests.
struct AttachmentPackages {
    wallet: Arc<Package>,
    note: Arc<Package>,
    tx_script: Arc<Package>,
}

/// Compiles the basic wallet, the P2ID note and the attachment transaction script once per test
/// process.
fn attachment_packages() -> &'static AttachmentPackages {
    static PACKAGES: OnceLock<AttachmentPackages> = OnceLock::new();
    PACKAGES.get_or_init(|| AttachmentPackages {
        wallet: compile_rust_package("../../examples/basic-wallet", true),
        note: compile_rust_package("../../examples/p2id-note", true),
        tx_script: compile_rust_package("../fixtures/components/note-attachment-tx-script", true),
    })
}

/// Builds a transaction running the selected attachment scenario with `scheme` on a native wallet
/// account.
fn build_attachment_tx(scenario: u32, scheme: u64) -> (MockChain, MockTransaction) {
    let AttachmentPackages {
        wallet: wallet_package,
        note: note_package,
        tx_script: tx_script_package,
    } = attachment_packages();
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

    let advice_content = advice_content();
    let advice_commitment = Poseidon2::hash_elements(&advice_content);
    let mock_tx = chain
        .build_transaction(account_id)
        .tx_script(transaction_script_from_package_with_deps(
            tx_script_package,
            &[note_package.as_ref()],
        ))
        .tx_script_args(Word::from([
            Felt::from(scenario),
            Felt::new(scheme).unwrap(),
            Felt::ZERO,
            Felt::ZERO,
        ]))
        .add_advice_map_entry(advice_commitment, advice_content)
        .build()
        .unwrap();

    (chain, mock_tx)
}

/// Typed word/slice paths and the advice-map writer preserve the protocol's commitments.
#[test]
fn typed_attachments_round_trip_after_note_constructor() {
    let (mut chain, mock_tx) = build_attachment_tx(0, SCHEME.into());
    let executed_tx = execute_tx(&mut chain, mock_tx);
    assert_eq!(executed_tx.output_notes().num_notes(), 1);

    let scheme = NoteAttachmentScheme::new(SCHEME).unwrap();
    let s1 = NoteAttachmentScheme::new(SCHEME + 1).unwrap();
    let s2 = NoteAttachmentScheme::new(SCHEME + 2).unwrap();
    let second =
        Word::from([Felt::from(5u32), Felt::from(6u32), Felt::from(7u32), Felt::from(8u32)]);
    let advice_words = advice_content()
        .chunks(4)
        .map(|chunk| Word::from(<[Felt; 4]>::try_from(chunk).unwrap()))
        .collect();
    let expected = NoteAttachments::new(vec![
        NoteAttachment::with_word(scheme, first_word()),
        NoteAttachment::with_word(s1, first_word()),
        NoteAttachment::with_words(scheme, vec![first_word(), second]).unwrap(),
        NoteAttachment::with_words(s2, advice_words).unwrap(),
    ])
    .unwrap();
    assert_eq!(executed_tx.output_notes().get_note(0).attachments(), &expected);
}

/// Per-note count and aggregate-content limits accept their exact protocol boundaries.
#[test]
fn typed_attachments_accept_four_attachments_and_512_words() {
    // Neither scenario does arithmetic on the scheme, so the upper bound passes through unchanged.
    for scheme in [SCHEME, NoteAttachmentScheme::MAX.as_u16()] {
        for (scenario, count, words) in [(1, 4, 1), (3, 2, 256)] {
            let (mut chain, mock_tx) = build_attachment_tx(scenario, scheme.into());
            let executed_tx = execute_tx(&mut chain, mock_tx);
            assert_eq!(executed_tx.output_notes().num_notes(), 1);
            let attachment = NoteAttachment::with_words(
                NoteAttachmentScheme::new(scheme).unwrap(),
                vec![first_word(); words],
            )
            .unwrap();
            let expected = NoteAttachments::new(vec![attachment; count]).unwrap();
            assert_eq!(
                executed_tx.output_notes().get_note(0).attachments(),
                &expected,
                "scheme {scheme}, scenario {scenario}"
            );
        }
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
        let (_chain, mock_tx) = build_attachment_tx(scenario, SCHEME.into());
        let err = execute_tx_expect_failure(mock_tx);
        let needle = format!("assertion failed with error message: {expected_error}");
        assert_failure_contains(&err, &needle);
    }
}

/// The compiled scheme conversion's accept and reject branches are observed through the attachment
/// the guest writes, including felts with a non-zero high limb.
#[test]
fn the_guest_reports_the_scheme_conversion_outcome() {
    for (scheme, expected) in [
        (u64::from(SCHEME), [0, 0, 0, 0]),
        (65534, [0, 0, 0, 0]),
        (0, [1, 0, 0, 0]),
        (65535, [1, 65535, 0, 0]),
        (65536 + u64::from(SCHEME), [2, 0, 0, 0]),
        ((1u64 << 32) + u64::from(SCHEME), [2, 0, 0, 0]),
        (Felt::ORDER - 1, [2, 0, 0, 0]),
    ] {
        let (mut chain, mock_tx) = build_attachment_tx(6, scheme);
        let executed_tx = execute_tx(&mut chain, mock_tx);
        let outcome = Word::from(expected.map(Felt::from_u32));
        let expected = NoteAttachments::new(vec![NoteAttachment::with_word(
            NoteAttachmentScheme::none(),
            outcome,
        )])
        .unwrap();
        assert_eq!(
            executed_tx.output_notes().get_note(0).attachments(),
            &expected,
            "scheme felt {scheme}"
        );
    }
}
