// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
extern crate alloc;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Define a panic handler as required by the `no_std` environment
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // For now, just loop indefinitely
    loop {}
}

mod bindings;

use bindings::exports::miden::counter_contract::counter::Guest;
use miden::{component, felt, Felt, StorageMap, StorageMapAccess, Word};

// Define the main contract struct
#[component]
struct CounterContract {
    // Define the storage field for the counter value
    #[storage(slot(0), description = "counter contract storage map")]
    count_map: StorageMap,
}

bindings::export!(CounterContract with_types_in bindings);

impl Guest for CounterContract {
    // Function to retrieve the current counter value
    fn get_count() -> Felt {
        // Get the instance of the contract
        let contract = CounterContract::default();
        // Define a fixed key for the counter value within the map
        let key = Word::from([felt!(0); 4]);
        // Read the value associated with the key from the storage map
        contract.count_map.get(&key)
    }

    // Function to increment the counter value
    fn increment_count() -> Felt {
        // Get the instance of the contract
        let contract = CounterContract::default();
        // Define the same fixed key
        let key = Word::from([felt!(0); 4]);
        // Read the current value
        let current_value: Felt = contract.count_map.get(&key);
        // Increment the value by one
        let new_value = current_value + felt!(1);
        // Write the new value back to the storage map
        contract.count_map.set(key, new_value);
        new_value
    }
}
