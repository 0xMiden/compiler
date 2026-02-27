// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden_test_harness::miden_test_suite;

#[cfg(target_family = "wasm")]
mod component {
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
}

#[miden_test_suite]
mod tests {
    use miden::Felt;
    use miden_protocol::account::{
        AccountBuilder, AccountComponent, auth::AuthSecretKey, component::InitStorageData,
    };
    use miden_standards::account::auth::AuthFalcon512Rpo;

    // This tests loads the generated package in the `foo` variable and is then
    // printed.
    #[miden_test]
    #[should_panic]
    fn bar(_bar: Package) {
        // To see what the generated Package looks like, uncomment this line:
        std::dbg!(&_bar);
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
    // Uncomment to see the failure!
    // #[miden_test]
    // fn bing(foo: Package, bar: Package) {
    //     std::dbg!(&foo);
    //     assert_eq!(1, 1 + 1);
    // }

    // This tests will work as a traditional test, since neither `Package` nor
    // `MockChainBuilder` are declared, the test harness does not produce any
    // type of code generation.
    #[miden_test]
    fn biz() {
        assert_eq!(2, 1 + 1)
    }

    #[miden_test]
    fn foo(chain: MockChainBuilder) {
        assert_eq!(2, 1 + 1)
    }

    // This function instantiates a `MockChain` with an `Account` with the
    // `AccountComponent` generated from the rust code from this file..
    #[miden_test]
    fn load_generated_account(account: Package, mock: MockChainBuilder) {
        let init_storage_data = InitStorageData::default();
        let account_component =
            AccountComponent::from_package(&account, &init_storage_data).unwrap();

        let (_key_pair, auth_component) = {
            let key_pair = AuthSecretKey::new_falcon512_rpo();
            let auth_component: AccountComponent =
                AuthFalcon512Rpo::new(key_pair.public_key().to_commitment()).into();
            (key_pair, auth_component)
        };

        let account = AccountBuilder::new(Default::default())
            .nonce(Felt::new(1).unwrap().into())
            .with_component(account_component)
            .with_auth_component(auth_component)
            .build()
            .unwrap();

        let _chain = mock.clone().build().unwrap();

        let _ = mock.add_account(account).unwrap().clone();
    }
}
