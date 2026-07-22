// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Felt, StorageMap, Word, component, component_storage, felt};

/// Storage layout for the counter example.
#[component_storage]
struct CounterContractStorage {
    /// Storage map holding the counter value.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// API of the counter contract account component.
#[component]
trait CounterContract {
    /// Returns the current counter value stored in the contract's storage map.
    #[account_procedure]
    fn get_count(&self) -> Felt;
    /// Increments the counter value stored in the contract's storage map by one.
    #[account_procedure]
    fn increment_count(&mut self) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }

    fn increment_count(&mut self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        let current_value: Felt = self.count_map.get(key);
        let new_value = current_value + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
