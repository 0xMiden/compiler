// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

#[macro_use]
extern crate alloc;

use crate::bindings::Account;
use alloc::vec::Vec;
use miden::*;

fn get_note_tag() -> Tag {
    let metadata = active_note::get_metadata();
    Tag::from(metadata.header[2])
}

/// Add two Words element-wise
fn add_word(a: Word, b: Word) -> Word {
    Word::from([a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]])
}

///
/// # Arguments
/// * `offered_total` - Total offered asset amount (Felt)
/// * `requested_total` - Total requested asset amount (Felt)
/// * `input_amount` - Input asset amount provided (Felt)
///
/// # Returns
/// Output asset amount proportional to the ratio (Felt)
fn calculate_output_amount(offered_total: Felt, requested_total: Felt, input_amount: Felt) -> Felt {
    let precision_factor = Felt::from_u32(100000);

    // For the better precision, we use the two different paths for the calculation
    if offered_total > requested_total {
        // Case 1: offered_total > requested_total
        // Calculate ratio = (offered_total * factor) / requested_total
        // Then output = (input_amount * ratio) / factor
        let ratio = (offered_total.as_u64() * precision_factor.as_u64()) / requested_total.as_u64();
        let output = (input_amount.as_u64() * ratio) / precision_factor.as_u64();
        return Felt::from_u32(output as u32);
    } else {
        // Case 2: offered_total <= requested_total
        // Calculate ratio = (requested_total * factor) / offered_total
        // Then output = (input_amount * factor) / ratio
        let ratio = (requested_total.as_u64() * precision_factor.as_u64()) / offered_total.as_u64();
        let output = (input_amount.as_u64() * precision_factor.as_u64()) / ratio;
        return Felt::from_u32(output as u32);
    }
}

/// Create a P2ID (Pay-to-ID) note
fn create_p2id_note(
    serial_num: Word,
    input_asset: &Asset,
    inflight_asset: &Asset,
    recipient_id: AccountId,
    aux: Felt,
    account: &mut Account,
) {
    let inputs = active_note::get_inputs();
    let tag = inputs[7];
    let tag = Tag::from(tag);

    let note_type = NoteType::from(inputs[6]);

    let p2id_note_root_digest = Digest::from_word(Word::new([
        Felt::from_u64_unchecked(13362761878458161062),
        Felt::from_u64_unchecked(15090726097241769395),
        Felt::from_u64_unchecked(444910447169617901),
        Felt::from_u64_unchecked(3558201871398422326),
    ]));

    // Create recipient from serial number and account ID
    let recipient = Recipient::compute(
        serial_num,
        p2id_note_root_digest,
        vec![recipient_id.suffix, recipient_id.prefix],
    );

    // Create the note using output_note::create
    let note_idx = output_note::create(tag, note_type, recipient);

    output_note::set_word_attachment(
        note_idx,
        felt!(0),
        Word::from([aux, felt!(0), felt!(0), felt!(0)]),
    );

    if input_asset.inner[0] != felt!(0) {
        account.move_asset_to_note(input_asset.clone(), note_idx)
    }

    output_note::add_asset(inflight_asset.clone(), note_idx);
}
/// Create a Swapp note with remainder parameters
fn create_swapp_note(serial_num: Word, aux: Felt, offered_asset: &Asset, padded_inputs: Vec<Felt>) {
    let tag = get_note_tag();

    let inputs = active_note::get_inputs();

    let note_type = NoteType::from(inputs[6]);

    // Create recipient with swapp script and remainder parameters
    let recipient = Recipient::compute(
        serial_num,
        Digest::from_word(active_note::get_script_root()),
        padded_inputs,
    );

    // Create the note using output_note::create
    let note_idx = output_note::create(tag, note_type, recipient);
    output_note::set_word_attachment(
        note_idx,
        felt!(0),
        Word::from([aux, felt!(0), felt!(0), felt!(0)]),
    );

    output_note::add_asset(offered_asset.clone(), note_idx);
}

#[note]
struct SwappNote;

#[note]
impl SwappNote {
    /// Swapp Note Script
    ///
    /// Implements a partially-fillable swap note for DEX functionality.
    ///
    /// **Note Arg (via `arg` parameter - provided by note consumer):**
    /// - Position 0: input_amount: Felt (single Felt value for amount)
    /// - Position 1: inflight_amount: Felt (single Felt value for amount)
    /// - Position 2: 0: Felt (unused)
    /// - Position 3: 0: Felt (unused)
    /// arg structure: [input_amount, inflight_amount, 0, 0]
    ///
    /// **Note Inputs (via `active_note::get_inputs()` - stored when note is created):**
    /// - Positions 0-3: Requested Asset Word (4 Felts)
    ///   - inputs[0]: requested_asset_id_prefix (Felt)
    ///   - inputs[1]: requested_asset_id_suffix (Felt)
    ///   - inputs[2]: padding (0, Felt)
    ///   - inputs[3]: requested_asset_total (Felt)
    /// - Positions 4-7: Note Creator AccountId (4 Felts)
    ///   - inputs[4]: note_creator_account_id_prefix (Felt)
    ///   - inputs[5]: note_creator_account_id_suffix (Felt)
    ///   - inputs[6]: note_type (Felt)
    ///   - inputs[7]: tag (Felt)
    ///
    ///
    #[note_script]
    fn run(self, arg: Word, account: &mut Account) {
        // Get stored note inputs
        let inputs = active_note::get_inputs();

        // Get executing account ID (the note consumer)
        let executing_account_id = active_account::get_id();
        let swapp_note_creator_id = AccountId::new(inputs[4], inputs[5]);

        // Validate that offered_asset_word matches the asset in the active note
        let note_assets = active_note::get_assets();

        // Check that there is exactly one asset in the note
        let num_assets = note_assets.len();
        assert_eq(Felt::from_u32(num_assets as u32), felt!(1));

        // Get the asset from the note
        let offered_asset = note_assets[0];

        if swapp_note_creator_id == executing_account_id {
            // Note creator is consuming their own note - receive assets back
            // Moves all assets from the note into the executing account's vault
            account.receive_asset(offered_asset);
            return;
        }

        // Extract amounts for calculations
        let requested_asset_total = inputs[3];
        let offered_asset_total = offered_asset.inner[0];

        // Get the current note serial number
        let current_note_serial = active_note::get_serial_number();

        // Extract input_amount from note arg (provided by note consumer)
        let input_amount = arg[0];
        let inflight_amount = arg[1];
        let total_input_amount = input_amount + inflight_amount;

        // Compute offered output amount proportional to input
        let input_offered_out =
            calculate_output_amount(offered_asset_total, requested_asset_total, input_amount);

        let inflight_offered_out =
            calculate_output_amount(offered_asset_total, requested_asset_total, inflight_amount);

        let input_offered_asset = Asset::new(Word::from([
            input_offered_out,
            offered_asset.inner[1],
            offered_asset.inner[2],
            offered_asset.inner[3],
        ]));

        account.receive_asset(input_offered_asset);

        // Create routing (P2ID) note, this is the note that will be used to route the requested asset to the note creator
        let routing_serial = add_word(
            current_note_serial,
            Word::from([felt!(1), felt!(1), felt!(1), felt!(1)]),
        );

        // aux value is the input amount so that the swapp note creator can determine build the note
        let aux_value = input_amount + inflight_amount;
        let input_asset_reversed =
            Asset::new(Word::from([inputs[0], inputs[1], inputs[2], input_amount]));
        let input_asset = Asset::new(input_asset_reversed.inner.reverse());

        // Add the inflight amount to the p2id note
        let inflight_asset_reversed = Asset::new(Word::from([
            inputs[0],
            inputs[1],
            inputs[2],
            inflight_amount,
        ]));
        let inflight_asset = Asset::new(inflight_asset_reversed.inner.reverse());

        // Create P2ID note using output_note module
        create_p2id_note(
            routing_serial,
            &input_asset,
            &inflight_asset,
            swapp_note_creator_id,
            aux_value,
            account,
        );

        let total_offered_out = input_offered_out + inflight_offered_out;

        // Create remainder swap note in case of partial fill
        if total_input_amount.as_u64() < requested_asset_total.as_u64() {
            let remainder_serial = hash_words(&[current_note_serial]).inner;
            let remainder_aux = total_offered_out;

            let requested_asset_total = inputs[3] - total_input_amount;
            let remainder_requested_asset =
                Asset::from([inputs[0], inputs[1], inputs[2], requested_asset_total]);

            let remainder_offered_asset_total = offered_asset_total - total_offered_out;
            let remainder_offered_asset_reversed = Asset::new(Word::from([
                offered_asset.inner[3],
                offered_asset.inner[2],
                offered_asset.inner[1],
                remainder_offered_asset_total,
            ]));
            let remainder_offered_asset =
                Asset::new(remainder_offered_asset_reversed.inner.reverse());

            let swapp_note_creator_id = AccountId::new(inputs[4], inputs[5]);

            let tag = inputs[7];
            let note_type = inputs[6];

            let padded_inputs = vec![
                remainder_requested_asset.inner[0],
                remainder_requested_asset.inner[1],
                remainder_requested_asset.inner[2],
                remainder_requested_asset.inner[3],
                swapp_note_creator_id.prefix,
                swapp_note_creator_id.suffix,
                note_type,
                tag,
            ];

            create_swapp_note(
                remainder_serial,
                remainder_aux,
                &remainder_offered_asset,
                padded_inputs,
            );
        }
    }
}
