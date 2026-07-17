//! SWAPP (partially-fillable swap) note script.
//!
//! Port of the SWAPP note from <https://github.com/inicio-labs/miden-swapp> to the current
//! Miden SDK. The note locks an *offered* asset and asks for a *requested* asset in return.
//! A consumer may fill the swap fully or partially:
//!
//! - the consumer receives a share of the offered asset proportional to the provided fill
//!   amount,
//! - a P2ID routing note carrying the fill amount of the requested asset is created for the
//!   swap creator,
//! - on a partial fill, a remainder SWAPP note (same script, reduced amounts) is created for
//!   the unfilled portion,
//! - the creator can consume their own note to reclaim the offered asset.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec;

use miden::*;

/// Native account of the note: exposes the `basic-wallet` component methods (e.g.
/// `receive_asset`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// Adds two words element-wise.
fn add_word(a: Word, b: Word) -> Word {
    Word::new([a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]])
}

/// Computes the output amount of the offered asset proportional to the provided input amount
/// of the requested asset, preserving the offered/requested price ratio.
///
/// # Arguments
/// * `offered_total` - Total offered asset amount
/// * `requested_total` - Total requested asset amount
/// * `input_amount` - Input asset amount provided
///
/// # Limitations (inherited from the original SWAPP contract)
/// * A non-divisible full fill fails asset conservation: e.g. 10 offered for 3 requested,
///   filled with 3, pays out `floor(3 * floor(10 * 100000 / 3) / 100000) = 9`; no remainder
///   note is created, so 1 offered unit is unaccounted for and the kernel rejects the
///   transaction.
/// * The `total * 100000` products can wrap `u64` for valid protocol amounts (up to
///   2^63 - 2^31), producing wrong-price fills.
/// * The payout depends on the direct/inflight split of the fill: for 7 offered for 5
///   requested, a single fill of 3 pays 4, while fills of 1 and 2 pay 1 + 2 = 3.
/// * A positive fill can round to a zero payout (dust fill): 1 offered for 2 requested,
///   filled with 1, pays the creator and returns nothing.
fn calculate_output_amount(offered_total: Felt, requested_total: Felt, input_amount: Felt) -> Felt {
    assert!(requested_total.as_canonical_u64() > 0);

    let precision_factor = Felt::from_u32(100000);

    // For the better precision, we use the two different paths for the calculation
    if offered_total > requested_total {
        // Case 1: offered_total > requested_total
        // Calculate ratio = (offered_total * factor) / requested_total
        // Then output = (input_amount * ratio) / factor
        let ratio = (offered_total.as_canonical_u64() * precision_factor.as_canonical_u64())
            / requested_total.as_canonical_u64();
        let output =
            (input_amount.as_canonical_u64() * ratio) / precision_factor.as_canonical_u64();
        Felt::new(output).unwrap()
    } else {
        // Case 2: offered_total <= requested_total
        // Calculate ratio = (requested_total * factor) / offered_total
        // Then output = (input_amount * factor) / ratio
        let ratio = (requested_total.as_canonical_u64() * precision_factor.as_canonical_u64())
            / offered_total.as_canonical_u64();
        let output =
            (input_amount.as_canonical_u64() * precision_factor.as_canonical_u64()) / ratio;
        Felt::new(output).unwrap()
    }
}

/// Returns the tag of the note that is currently executing.
fn active_note_tag() -> Tag {
    note::metadata_into_tag(active_note::get_metadata().header)
}

/// Returns the untyped ("none") attachment scheme used for the aux word attachments.
///
/// Scheme 0 is reserved by the protocol to signal an absent attachment.
fn aux_attachment_scheme() -> Felt {
    felt!(1)
}

/// SWAPP note storage.
///
/// The note creator stores the swap terms in the note storage; the fields below are decoded
/// from the storage elements in declaration order.
#[note]
struct SwappNote {
    /// Vault key identifying the requested asset (faucet id, composition, callback flags).
    requested_asset_key: Word,
    /// Total requested asset amount for the full offer.
    requested_total: Felt,
    /// The account that created the swap offer and receives the requested asset.
    creator: AccountId,
    /// Note type used for the notes created by this script (P2ID routing note and remainder
    /// SWAPP note).
    output_note_type: Felt,
    /// Tag routing the P2ID note to the creator.
    p2id_tag: Felt,
    /// Script root of the P2ID note script used for the routing note.
    p2id_script_root: Word,
}

impl SwappNote {
    /// Creates the P2ID routing note carrying the requested asset to the swap creator.
    ///
    /// The `input_amount` portion transits through the consumer's vault, while the
    /// `inflight_amount` portion is attached directly from the transaction's asset pool
    /// (e.g. assets released by another note consumed in the same transaction). The total
    /// fill amount is attached as an aux word so the creator can identify the payment.
    fn create_p2id_note(
        &self,
        serial_num: Word,
        input_amount: Felt,
        inflight_amount: Felt,
        account: &mut Wallet,
    ) {
        // The protocol P2ID note storage layout is [target_id_suffix, target_id_prefix].
        let recipient = note::build_recipient(
            serial_num,
            self.p2id_script_root,
            vec![self.creator.suffix, self.creator.prefix],
        );

        let note_idx = output_note::create(
            Tag::from(self.p2id_tag),
            NoteType::from(self.output_note_type),
            recipient,
        );

        let aux = input_amount + inflight_amount;
        output_note::add_word_attachment(
            note_idx,
            aux_attachment_scheme(),
            padded_word_from_felt(aux),
        );

        if input_amount != felt!(0) {
            let input_asset =
                Asset::new(self.requested_asset_key, padded_word_from_felt(input_amount));
            account.move_asset_to_note(input_asset, note_idx);
        }

        if inflight_amount != felt!(0) {
            let inflight_asset =
                Asset::new(self.requested_asset_key, padded_word_from_felt(inflight_amount));
            output_note::add_asset(inflight_asset, note_idx);
        }
    }

    /// Creates the remainder SWAPP note for the unfilled portion of the swap.
    ///
    /// The remainder note reuses the executing note's script and tag, carries the remaining
    /// offered asset, and requests the remaining requested amount. The offered amount paid
    /// out for this fill is attached as an aux word.
    fn create_remainder_note(
        &self,
        serial_num: Word,
        aux: Felt,
        remainder_offered_asset: Asset,
        remainder_requested_total: Felt,
    ) {
        let storage = vec![
            self.requested_asset_key[0],
            self.requested_asset_key[1],
            self.requested_asset_key[2],
            self.requested_asset_key[3],
            remainder_requested_total,
            self.creator.prefix,
            self.creator.suffix,
            self.output_note_type,
            self.p2id_tag,
            self.p2id_script_root[0],
            self.p2id_script_root[1],
            self.p2id_script_root[2],
            self.p2id_script_root[3],
        ];

        let recipient = note::build_recipient(serial_num, active_note::get_script_root(), storage);

        let note_idx = output_note::create(
            active_note_tag(),
            NoteType::from(self.output_note_type),
            recipient,
        );

        output_note::add_word_attachment(
            note_idx,
            aux_attachment_scheme(),
            padded_word_from_felt(aux),
        );
        output_note::add_asset(remainder_offered_asset, note_idx);
    }
}

#[note]
impl SwappNote {
    /// SWAPP note script entrypoint.
    ///
    /// **Note arg (provided by the note consumer):**
    /// - `arg[0]`: input amount of the requested asset provided from the consumer's vault
    /// - `arg[1]`: inflight amount of the requested asset provided from the transaction's
    ///   asset pool (released by other notes consumed in the same transaction)
    /// - `arg[2..4]`: unused
    ///
    /// Unless the creator consumes their own note, the total fill amount must be positive and
    /// must not exceed the requested total.
    #[note_script]
    fn run(self, arg: Word, account: &mut Wallet) {
        // The swap note must carry exactly one offered asset.
        let note_assets = active_note::get_assets();
        assert_eq(Felt::from_u32(note_assets.len() as u32), felt!(1));
        let offered_asset = note_assets[0];

        // Note creator is consuming their own note - receive the offered asset back.
        let executing_account_id = active_account::get_id();
        if self.creator == executing_account_id {
            account.receive_asset(offered_asset);
            return;
        }

        let offered_total = offered_asset.value[0];
        let requested_total = self.requested_total;

        let input_amount = arg[0];
        let inflight_amount = arg[1];
        let total_input_amount = input_amount + inflight_amount;

        // A zero fill would recreate the order under a new note id for free (griefing), and an
        // overfill could otherwise succeed whenever the computed payout still fits the locked
        // offered asset.
        assert!(total_input_amount.as_canonical_u64() > 0);
        assert!(total_input_amount.as_canonical_u64() <= requested_total.as_canonical_u64());

        // Compute the offered output amounts proportional to the provided amounts.
        let input_offered_out =
            calculate_output_amount(offered_total, requested_total, input_amount);
        let inflight_offered_out =
            calculate_output_amount(offered_total, requested_total, inflight_amount);

        // The consumer receives the offered share corresponding to the vault-provided input.
        if input_offered_out != felt!(0) {
            let input_offered_asset =
                Asset::new(offered_asset.key, padded_word_from_felt(input_offered_out));
            account.receive_asset(input_offered_asset);
        }

        // Create the P2ID routing note paying the requested asset to the swap creator.
        let current_note_serial = active_note::get_serial_number();
        let routing_serial =
            add_word(current_note_serial, Word::new([felt!(1), felt!(1), felt!(1), felt!(1)]));
        self.create_p2id_note(routing_serial, input_amount, inflight_amount, account);

        // Create the remainder SWAPP note in case of a partial fill.
        if total_input_amount.as_canonical_u64() < requested_total.as_canonical_u64() {
            let remainder_serial = hash_words(&[current_note_serial]).inner;

            let total_offered_out = input_offered_out + inflight_offered_out;
            let remainder_offered_asset = Asset::new(
                offered_asset.key,
                padded_word_from_felt(offered_total - total_offered_out),
            );
            let remainder_requested_total = requested_total - total_input_amount;

            self.create_remainder_note(
                remainder_serial,
                total_offered_out,
                remainder_offered_asset,
                remainder_requested_total,
            );
        }
    }
}
