// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Felt, StorageMap, StorageMapAccess, Word, component, felt, miden_field::word};

/// Main contract structure for the counter example.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap,
}

#[component]
impl CounterContract {
    /// Returns the current counter value stored in the contract's storage map.
    pub fn get_count(&self) -> Felt {
        let key = Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, felt!(1)]);
        let word: Word = self.count_map.get(&key);
        word[3]
    }

    /// Increments the counter value stored in the contract's storage map by one.
    pub fn increment_count(&mut self) -> Felt {
        let key = Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, felt!(1)]);
        let current_value_word: Word = self.count_map.get(&key);
        let new_value = current_value_word[3] + felt!(1);
        let new_value_word = Word::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, new_value]);
        self.count_map.set(key, new_value_word);
        new_value
    }
}
