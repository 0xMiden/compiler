//! Reproducer for https://github.com/0xMiden/compiler/issues/831
//!
//! This note-script previously triggered a panic during MASM codegen with:
//! `invalid stack offset for movup: 16 is out of range`.

#![no_std]
#![feature(alloc_error_handler)]

#[macro_use]
extern crate alloc;

use miden::{miden_field::word, *};

#[note]
struct InvalidStackOffsetMovupNote;

#[note]
impl InvalidStackOffsetMovupNote {
    /// Note-script entrypoint used to reproduce issue #831.
    ///
    /// The `create_swapp_note` call uses a flattened argument size of 15 felts.
    #[note_script]
    pub fn run(self, arg: Word) {
        // NOTE: Guard the reproduction logic behind a runtime condition so that once #831 is fixed,
        // this note can execute without requiring a full `active_note`/`output_note` runtime context.
        if arg[0] == felt!(0) {
            return;
        }

        let inputs = active_note::get_inputs();

        let executing_account_id = active_account::get_id();
        let swapp_note_creator_id = AccountId::new(inputs[8], inputs[9]);

        if swapp_note_creator_id == executing_account_id {
            // active_note::add_assets_to_account();
            return;
        }

        let inflight_val = arg[0];
        let input_amount = arg[1];
        let _inflight = inflight_val != felt!(0);

        let _requested_asset_word = Asset::from([inputs[0], inputs[1], inputs[2], inputs[3]]);
        let offered_asset_word = Asset::from([inputs[4], inputs[5], inputs[6], inputs[7]]);

        let note_assets = active_note::get_assets();
        let num_assets = note_assets.len();
        assert_eq(Felt::new(num_assets as u64), felt!(1));

        let note_asset = note_assets[0];
        let assets_match = if offered_asset_word == note_asset {
            felt!(1)
        } else {
            felt!(0)
        };
        assert_eq(assets_match, felt!(1));

        let requested_asset_total = inputs[3];
        let offered_asset_total = inputs[7];

        let current_note_serial = active_note::get_serial_number();

        let is_valid = if input_amount <= requested_asset_total {
            felt!(1)
        } else {
            felt!(0)
        };
        assert_eq(is_valid, felt!(1));

        let _one = felt!(1);
        let offered_out =
            calculate_output_amount(offered_asset_total, requested_asset_total, input_amount);

        // active_note::add_assets_to_account();

        let routing_serial = add_word(
            current_note_serial,
            Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, felt!(1)]),
        );

        let aux_value = offered_out;
        let input_asset = Asset::new(Word::from([inputs[0], inputs[1], inputs[2], input_amount]));

        create_p2id_note(routing_serial, input_asset, swapp_note_creator_id, aux_value);

        if offered_out < offered_asset_total {
            let remainder_serial = hash_words(&[current_note_serial]).inner;
            let remainder_aux = offered_out;
            let remainder_requested_asset =
                Asset::from([inputs[0], inputs[1], inputs[2], inputs[3] - input_amount]);
            let remainder_offered_asset =
                Asset::from([inputs[4], inputs[5], inputs[6], inputs[7] - offered_out]);

            create_swapp_note(
                remainder_serial,
                remainder_requested_asset,
                remainder_offered_asset,
                swapp_note_creator_id,
                remainder_aux,
            );
        }
    }
}

/// Calculates the output amount for the given swap parameters.
fn calculate_output_amount(offered_total: Felt, requested_total: Felt, input_amount: Felt) -> Felt {
    let precision_factor = Felt::new(100000);

    if offered_total > requested_total {
        let ratio = (offered_total * precision_factor) / requested_total;
        (input_amount * ratio) / precision_factor
    } else {
        let ratio = (requested_total * precision_factor) / offered_total;
        (input_amount * ratio) / precision_factor
    }
}

/// Adds two words element-wise.
fn add_word(a: Word, b: Word) -> Word {
    Word::from([a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]])
}

/// Creates a P2ID output note.
fn create_p2id_note(serial_num: Word, input_asset: Asset, recipient_id: AccountId, _aux: Felt) {
    let tag = Tag::from(felt!(0));
    let note_type = get_note_type();

    let _p2id_note_root_digest = Digest::from_word(Word::new([
        Felt::new(6412241294473976817),
        Felt::new(10671567784403105513),
        Felt::new(4275774806771663409),
        Felt::new(17933276983439992403),
    ]));

    let recipient = Recipient::compute(
        serial_num,
        Digest::from_word(active_note::get_script_root()),
        vec![
            recipient_id.prefix,
            recipient_id.suffix,
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
        ],
    );

    let note_idx = output_note::create(tag, note_type, recipient);
    output_note::add_asset(input_asset, note_idx);
}

/// Creates a SWAPP output note.
fn create_swapp_note(
    serial_num: Word,
    offered_asset: Asset,
    requested_asset: Asset,
    note_creator_id: AccountId,
    _aux: Felt,
) {
    let tag = get_note_tag();
    let note_type = get_note_type();

    let recipient = Recipient::compute(
        serial_num,
        Digest::from_word(active_note::get_script_root()),
        vec![
            offered_asset.inner[0],
            offered_asset.inner[1],
            offered_asset.inner[2],
            offered_asset.inner[3],
            requested_asset.inner[0],
            requested_asset.inner[1],
            requested_asset.inner[2],
            requested_asset.inner[3],
            note_creator_id.prefix,
            note_creator_id.suffix,
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
            felt!(0),
        ],
    );

    let note_idx = output_note::create(tag, note_type, recipient);
    output_note::add_asset(offered_asset, note_idx);
}

/// Extracts the note tag from the active note metadata.
fn get_note_tag() -> Tag {
    let metadata = active_note::get_metadata().header;
    let left_shifted_32 = metadata[2] * Felt::new(2u32.pow(32));
    let tag_felt = left_shifted_32 / (Felt::new(2u32.pow(32)));
    Tag::from(tag_felt)
}

/// Extracts the note type from the active note metadata.
fn get_note_type() -> NoteType {
    let metadata = active_note::get_metadata().header;
    let second_felt = metadata[1];
    let pow_56 = Felt::new(2u64.pow(56));
    let pow_62 = Felt::new(2u64.pow(62));
    let left_shifted_56 = second_felt * pow_56;
    let note_type_felt = left_shifted_56 / pow_62;
    NoteType::from(note_type_felt)
}
