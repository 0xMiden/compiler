//! Foreign procedure invocation tests for payload variants between two accounts.
//!
//! The caller account invokes the callee account's mixed-width variant method through FPI
//! while executing as the native account of the consumed note.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{
    build_account_to_account_fpi_test_packages, execute_account_to_account_note,
};

/// Deploys two accounts and consumes a note whose target bumps payload variants via FPI.
#[test]
pub fn payload_variant() {
    let (callee_package, caller_package, note_package, callee_storage_slot) =
        build_account_to_account_fpi_test_packages(
            "payload_variant",
            CALLEE_ACCOUNT_SOURCE,
            CALLER_ACCOUNT_SOURCE,
            NOTE_SOURCE,
        );

    execute_account_to_account_note(
        callee_package,
        caller_package,
        note_package,
        callee_storage_slot,
        callee_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used to initialize the callee account.
fn callee_storage_key() -> Word {
    Word::new([
        Felt::new(81).unwrap(),
        Felt::new(82).unwrap(),
        Felt::new(83).unwrap(),
        Felt::new(84).unwrap(),
    ])
}

/// Minimal callee account component source used by the payload variant FPI test.
const CALLEE_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, export_type, felt, Felt, StorageMap, Word};

/// Variant whose cases join a double-word integer lane with a felt lane.
#[export_type]
pub enum MixedPayload {
    /// Carries a double-word integer value.
    Wide(u64),
    /// Carries a single felt value.
    Scalar(Felt),
}

/// Account component whose FPI method transforms a payload variant.
#[component_storage]
struct CounterContractStorage {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "callee account counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose FPI method transforms a payload variant.
#[component]
trait CounterContract {
    /// Increments the payload of either mixed-width variant case.
    fn bump_mixed(&self, input: MixedPayload) -> MixedPayload;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Increments the payload of either mixed-width variant case.
    fn bump_mixed(&self, input: MixedPayload) -> MixedPayload {
        let _ = &self.count_map;
        match input {
            MixedPayload::Wide(value) => MixedPayload::Wide(value + 1),
            MixedPayload::Scalar(value) => MixedPayload::Scalar(value + felt!(1)),
        }
    }
}
"#;

/// Minimal caller account component source used by the payload variant FPI test.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{account, assert_eq, component, component_storage, felt, AccountId, Felt};

use crate::bindings::miden::payload_variant_callee_account::counter_contract::MixedPayload;

#[account(payload_variant_callee_account::CounterContract)]
struct CalleeAccount;

/// Account component which bumps payload variants on another account through FPI.
#[component_storage]
struct CallerAccountStorage;

/// Account component which bumps payload variants on another account through FPI.
#[component]
trait CallerAccount {
    /// Bumps both variant cases on the provided foreign account and returns the felt payload.
    fn relay_bump(&self, callee_account_id: AccountId) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    /// Bumps both variant cases on the provided foreign account and returns the felt payload.
    fn relay_bump(&self, callee_account_id: AccountId) -> Felt {
        let callee = CalleeAccount::new(callee_account_id);
        match callee.bump_mixed(MixedPayload::Wide(40)) {
            MixedPayload::Wide(value) => {
                assert_eq(Felt::from_u32(value as u32), Felt::from_u32(41))
            }
            MixedPayload::Scalar(_) => assert_eq(felt!(0), felt!(1)),
        }
        match callee.bump_mixed(MixedPayload::Scalar(felt!(20))) {
            MixedPayload::Scalar(value) => value,
            MixedPayload::Wide(_) => felt!(0),
        }
    }
}
"#;

/// Minimal note script source which invokes the caller account method.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller account component, whose `relay_bump`
/// method is invoked directly on the active account.
#[account(payload_variant_caller_account::CallerAccount)]
struct Account;

/// Note script input containing the foreign callee account id.
#[note]
struct PayloadVariantNote {
    /// Account id of the callee account to invoke through the caller account.
    callee_account_id: AccountId,
}

#[note]
impl PayloadVariantNote {
    /// Checks that payload variants cross the FPI boundary between two accounts.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let value = account.relay_bump(self.callee_account_id);
        assert_eq(value, felt!(21));
    }
}
"#;
