// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{component, component_storage, felt, Felt, StorageMap, Word};

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
    fn get_count(&self) -> Felt;
    /// Increments the counter value stored in the contract's storage map by one.
    fn increment_count(&mut self) -> Felt;
}

#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self) -> Felt {
        // Define a fixed key for the counter value within the map
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        // Read the value associated with the key from the storage map
        self.count_map.get(key)
    }

    fn increment_count(&mut self) -> Felt {
        // Define the same fixed key
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        // Read the current value
        let current_value: Felt = self.count_map.get(key);
        // Increment the value by one
        let new_value = current_value + felt!(1);
        // Write the new value back to the storage map
        self.count_map.set(key, new_value);
        new_value
    }
}
