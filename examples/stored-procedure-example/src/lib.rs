//! An account component that dispatches through a procedure root kept in account storage.
//!
//! A slot of type `StorageValue<StoredProcedure<F>>` holds a MAST root together with the signature
//! `F`. The slot accepts a root of that signature only, and a call through the slot takes the
//! arguments of that signature only. The compiler checks both.
//!
//! The component has two such slots, because each slot fixes one signature:
//!
//! * `handler` holds a root of a procedure that takes no argument and returns one field element.
//!   `dispatch` calls it.
//! * `weighted_handler` holds a root of a procedure that takes a word and a field element, and
//!   returns one field element. `dispatch_weighted` calls it.
//!
//! Install a root with `set_handler` or `set_weighted_handler`, or supply it in the initial
//! storage of the account.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{
    Call0, Call2, Felt, ProcedureRoot, StorageValue, StoredProcedure, Word, component,
    component_storage, felt,
};

/// Storage layout of the stored-procedure example.
#[component_storage]
struct StoredProcedureExampleStorage {
    /// Procedure that `dispatch` calls.
    #[storage(description = "root of the procedure dispatch calls")]
    handler: StorageValue<StoredProcedure<fn() -> Felt>>,

    /// Procedure that `dispatch_weighted` calls.
    #[storage(description = "root of the procedure dispatch_weighted calls")]
    weighted_handler: StorageValue<StoredProcedure<fn(Word, Felt) -> Felt>>,

    /// Value that `get_value` returns.
    #[storage(description = "value get_value returns")]
    value: StorageValue<Felt>,
}

/// API of the stored-procedure example account component.
#[component]
trait StoredProcedureExample {
    /// Returns the value in the `value` storage slot.
    #[account_procedure]
    fn get_value(&self) -> Felt;

    /// Writes `root` into the `handler` storage slot.
    #[account_procedure]
    fn set_handler(&mut self, root: ProcedureRoot);

    /// Writes `root` into the `weighted_handler` storage slot.
    #[account_procedure]
    fn set_weighted_handler(&mut self, root: ProcedureRoot);

    /// Calls the procedure whose root is in the `handler` storage slot and returns its result.
    ///
    /// The transaction fails if the slot holds the zero word.
    #[account_procedure]
    fn dispatch(&self) -> Felt;

    /// Returns the sum of the elements of `w` and of `scale`, each with a different weight.
    ///
    /// Every input has its own weight, so the result changes if the caller supplies the inputs in
    /// another order.
    #[account_procedure]
    fn weighted_sum(&self, w: Word, scale: Felt) -> Felt;

    /// Calls the procedure whose root is in the `weighted_handler` slot with `w` and `scale`.
    ///
    /// The transaction fails if the slot holds the zero word.
    #[account_procedure]
    fn dispatch_weighted(&self, w: Word, scale: Felt) -> Felt;
}

#[component]
impl StoredProcedureExample for StoredProcedureExampleStorage {
    fn get_value(&self) -> Felt {
        self.value.get()
    }

    fn set_handler(&mut self, root: ProcedureRoot) {
        // The slot type gives the signature, so the caller states no type here. The caller of this
        // procedure asserts that the root takes no argument and returns one field element.
        self.handler.set(root.assume_signature());
    }

    fn set_weighted_handler(&mut self, root: ProcedureRoot) {
        // The caller of this procedure asserts that the root takes a word and a field element, and
        // returns one field element.
        self.weighted_handler.set(root.assume_signature());
    }

    fn dispatch(&self) -> Felt {
        self.handler.get().call()
    }

    fn weighted_sum(&self, w: Word, scale: Felt) -> Felt {
        w[0] + w[1] * felt!(2) + w[2] * felt!(3) + w[3] * felt!(4) + scale * felt!(5)
    }

    fn dispatch_weighted(&self, w: Word, scale: Felt) -> Felt {
        self.weighted_handler.get().call(w, scale)
    }
}
