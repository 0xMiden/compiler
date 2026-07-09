//! Foreign procedure invocation tests for the direct ABI 16-operand boundary.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes ten user felts through FPI.
#[test]
pub fn ten_felts() {
    let (counter_package, caller_note_package, counter_storage_slot) =
        build_fpi_test_packages("ten_felts", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        ten_felts_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the ten-felt FPI test.
fn ten_felts_storage_key() -> Word {
    Word::new([
        Felt::new(31).unwrap(),
        Felt::new(32).unwrap(),
        Felt::new(33).unwrap(),
        Felt::new(34).unwrap(),
    ])
}

/// Minimal counter account component source used by the ten-felt FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, Felt, StorageMap, Word};

/// Account component whose FPI method accepts ten felt arguments.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose FPI method accepts ten felt arguments.
#[component]
trait CounterContract {
    /// Returns the counter value plus all six extra felt arguments.
    fn get_count_by_ten_felts(
        &self,
        key0: Felt,
        key1: Felt,
        key2: Felt,
        key3: Felt,
        add0: Felt,
        add1: Felt,
        add2: Felt,
        add3: Felt,
        add4: Felt,
        add5: Felt,
    ) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Returns the counter value plus all six extra felt arguments.
    fn get_count_by_ten_felts(
        &self,
        key0: Felt,
        key1: Felt,
        key2: Felt,
        key3: Felt,
        add0: Felt,
        add1: Felt,
        add2: Felt,
        add3: Felt,
        add4: Felt,
        add5: Felt,
    ) -> Felt {
        let key = Word::new([key0, key1, key2, key3]);
        self.count_map.get(key) + add0 + add1 + add2 + add3 + add4 + add5
    }
}
"#;

/// Minimal note script source which exercises the direct FPI 16-operand boundary.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[account(ten_felts_account::CounterContract)]
struct Counter;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that ten user felts cross the direct FPI boundary in order.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = Counter::new(self.counter_account_id);
        let count = count_acc.get_count_by_ten_felts(
            felt!(31),
            felt!(32),
            felt!(33),
            felt!(34),
            felt!(1),
            felt!(2),
            felt!(3),
            felt!(4),
            felt!(5),
            felt!(6),
        );
        assert_eq(count, felt!(121));
    }
}
"#;
