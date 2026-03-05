// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Felt, StorageMap, Word, component, felt};

/// Main contract structure for the counter example.
#[component]
struct CounterContract {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the current counter value stored in the contract's storage map.
    pub fn get_count(&self) -> Felt {
        let key = Word::from_u64_unchecked(0, 0, 0, 1);
        self.count_map.get(key)
    }

    /// Increments the counter value stored in the contract's storage map by one.
    pub fn increment_count(&mut self) -> Felt {
        let key = Word::from_u64_unchecked(0, 0, 0, 1);
        let current_value: Felt = self.count_map.get(key);
        let new_value = current_value + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
