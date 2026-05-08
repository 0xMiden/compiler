//! Foreign procedure invocation tests for methods that accept a `Word` argument.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which reads it through `Word -> Felt` FPI.
#[test]
pub fn word_arg() {
    let (counter_package, caller_note_package, counter_storage_slot) =
        build_fpi_test_packages("word_arg", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        word_arg_storage_key(),
        42,
    );
}

/// Returns the non-zero storage key used by the `Word` argument FPI test.
fn word_arg_storage_key() -> Word {
    Word::new([Felt::new(11), Felt::new(22), Felt::new(33), Felt::new(44)])
}

/// Minimal counter account component source used by the `Word` argument FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, Felt, StorageMap, Word};

/// Account component whose storage map holds one counter value.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the counter value stored under `key`.
    pub fn get_count_by_key(&self, key: Word) -> Felt {
        self.count_map.get(key)
    }
}
"#;

/// Minimal note script source which reads the generated counter account through `Word -> Felt` FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::WordArgAccount as CounterContract;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that a `Word` argument is forwarded to the foreign counter account.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let key = Word::new([felt!(11), felt!(22), felt!(33), felt!(44)]);
        let count = count_acc.get_count_by_key(key);
        assert_eq(count, felt!(42));
    }
}
"#;
