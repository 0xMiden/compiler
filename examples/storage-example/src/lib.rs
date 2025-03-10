// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
extern crate alloc;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use bindings::exports::miden::storage_example::*;

bindings::export!(MyFoo with_types_in bindings);

mod bindings;

use miden::{storage, Felt, Word};

struct MyFoo;

impl foo::Guest for MyFoo {
    fn test_storage_item(index: Felt, value: Word) -> Felt {
        let (_new_root, _old_value) = storage::set_item(index, value);
        let retrieved_value = storage::get_item(index);
        assert_eq!(value, retrieved_value);
        retrieved_value[0]
    }

    fn test_storage_map_item(index: Felt, key: Word, value: Word) -> Felt {
        let (_old_map_root, _old_map_value) = storage::set_map_item(index, key, value);
        let retrieved_map_value = storage::get_map_item(index, key);
        assert_eq!(value, retrieved_map_value);
        retrieved_map_value[0]
    }
}
