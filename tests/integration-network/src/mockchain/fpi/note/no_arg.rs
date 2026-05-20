//! Foreign procedure invocation tests for methods without user arguments.

use super::super::{
    super::support::COUNTER_CONTRACT_STORAGE_KEY,
    common::{build_fpi_test_packages, execute_counter_caller_note},
};

/// Deploys a counter contract and consumes a note which reads it through no-arg FPI.
#[test]
pub fn no_arg() {
    let (counter_package, caller_note_package, counter_storage_slot) =
        build_fpi_test_packages("no_arg", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        COUNTER_CONTRACT_STORAGE_KEY,
        42,
    );
}

/// Minimal counter account component source used by the no-argument FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, felt, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the current counter value.
    pub fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }
}
"#;

/// Minimal note script source which reads the generated counter account through no-arg FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::MidenNoArgAccount as CounterContract;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that the foreign counter account stores the initialized value.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let count = count_acc.get_count();
        assert_eq(count, felt!(42));
    }
}
"#;
