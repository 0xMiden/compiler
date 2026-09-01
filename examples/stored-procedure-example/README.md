# Stored Procedure Example

An account component that keeps the MAST root of a procedure in a storage slot and calls that
procedure through the root.

A slot of type `StorageValue<StoredProcedure<F>>` holds a root together with the signature `F`, so
the compiler checks every root the slot takes and every call the slot makes.

The component shows the feature:

- `dispatch` reads the `handler` slot, which holds a procedure that takes no argument, and calls it
  with `handle.call()`.
- `dispatch_weighted` reads the `weighted_handler` slot, which holds a procedure that takes a word
  and a field element, and calls it with `handle.call(w, scale)`. `weighted_sum` is a procedure with
  that signature.

Each slot fixes one signature, so a component that dispatches to two signatures needs two slots.

Write a root into a slot with `set_handler` or `set_weighted_handler`, which add the signature of
the slot with `assume_signature`, or supply the root in the initial storage of the account.

## Build

```bash
cargo miden build --release
```
