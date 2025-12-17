// Do not link against libstd (i.e. anything defined in `std::`)
#![cfg_attr(not(test), no_std)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden_test_harness_macros::miden_test_block;

#[cfg(not(test))]
mod component {
    use miden::{component, felt, Felt, StorageMap, StorageMapAccess, Word};

    /// Main contract structure for the counter example.
    #[component]
    struct CounterContract {
        /// Storage map holding the counter value.
        #[storage(slot(0), description = "counter contract storage map")]
        count_map: StorageMap,
    }

    #[component]
    impl CounterContract {
        /// Returns the current counter value stored in the contract's storage map.
        pub fn get_count(&self) -> Felt {
            let key = Word::from([felt!(0), felt!(0), felt!(0), felt!(1)]);
            self.count_map.get(&key)
        }

        /// Increments the counter value stored in the contract's storage map by one.
        pub fn increment_count(&self) -> Felt {
            let key = Word::from([felt!(0), felt!(0), felt!(0), felt!(1)]);
            let current_value: Felt = self.count_map.get(&key);
            let new_value = current_value + felt!(1);
            self.count_map.set(key, new_value);
            new_value
        }
    }
}

#[miden_test_block]
mod tests {
    // This tests loads the generated package in the `foo` variable and is then
    // printed in line 50.
    #[miden_test]
    fn bar(bar: Package) {
        std::dbg!(&bar);
        assert_eq!(1, 1 + 1);
    }

    // This test will fail at compile time because it is only legal to have a
    // single package as an argument. The following error message is displayed:
    //
    // error: custom attribute panicked
    //   --> src/lib.rs:55:5
    //    |
    // 55 |     #[miden_test]
    //    |     ^^^^^^^^^^^^^
    //    |
    //    = help: message:
    //            Detected that all of the following variables are `Package`s: foo, bar
    //
    //            #[miden_test] only supports having a single `Package` in its argument list.
    // #[miden_test]
    // fn bing(foo: Package, bar: Package) {
    //     std::dbg!(&foo);
    //     assert_eq!(1, 1 + 1);
    // }

    // This tests will not load the package since there is no argument declared

    // with the Package type. This test will simply behave as normal cargo test.
    #[miden_test]
    fn foo(chain: MockChainBuilder) {
        assert_eq!(2, 1 + 1)
    }
}
