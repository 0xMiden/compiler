//! Foreign procedure invocation tests for one account calling another account.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{
    build_account_to_account_fpi_test_packages, execute_account_to_account_note,
};

/// Deploys two accounts and consumes a note whose target account calls the second account via FPI.
#[test]
pub fn account_to_account() {
    let (callee_package, caller_package, note_package, callee_storage_slot) =
        build_account_to_account_fpi_test_packages(
            "account_to_account",
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
        314,
    );
}

/// Returns the non-zero storage key read by the account-to-account FPI call.
fn callee_storage_key() -> Word {
    Word::new([
        Felt::new(13).unwrap(),
        Felt::new(21).unwrap(),
        Felt::new(34).unwrap(),
        Felt::new(55).unwrap(),
    ])
}

/// Minimal callee account component source used by the account-to-account FPI test.
const CALLEE_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "callee account counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose storage map holds one counter value.
#[component]
trait CounterContract {
    /// Returns the counter value stored under the provided key.
    fn get_count(&self, key: Word) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Returns the counter value stored under the provided key.
    fn get_count(&self, key: Word) -> Felt {
        self.count_map.get(key)
    }
}
"#;

/// Minimal caller account component source used by the account-to-account FPI test.
const CALLER_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{account, component, component_storage, felt, AccountId, Felt, Word};

#[account(account_to_account_callee_account::CounterContract)]
struct CalleeAccount;

/// Account component which forwards reads to another account through FPI.
#[component_storage]
struct CallerAccountStorage;

/// Account component which forwards reads to another account through FPI.
#[component]
trait CallerAccount {
    /// Reads a counter value from the provided foreign account.
    fn read_foreign_count(&self, callee_account_id: AccountId) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    /// Reads a counter value from the provided foreign account.
    fn read_foreign_count(&self, callee_account_id: AccountId) -> Felt {
        let callee = CalleeAccount::new(callee_account_id);
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        callee.get_count(key)
    }
}
"#;

/// Minimal note script source which invokes the caller account method.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller account component, whose `read_foreign_count`
/// method is invoked directly on the active account.
///
/// Deliberately named `Account` — the name of the removed auto-generated wrapper — as regression
/// coverage that user-defined `#[account(...)]` wrappers may use it.
#[account(account_to_account_caller_account::CallerAccount)]
struct Account;

/// Note script input containing the foreign callee account id.
#[note]
struct AccountToAccountNote {
    /// Account id of the callee account to invoke through the caller account.
    callee_account_id: AccountId,
}

#[note]
impl AccountToAccountNote {
    /// Checks that the active caller account can read the foreign callee account through FPI.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let count = account.read_foreign_count(self.callee_account_id);
        assert_eq(count, felt!(314));
    }
}
"#;
