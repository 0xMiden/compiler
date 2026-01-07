# `miden-felt-repr-derive`

Derive macros used by `miden-felt-repr` (and its thin wrappers `miden-felt-repr-{onchain,offchain}`) to implement
serialization/deserialization to a flat sequence of `Felt` elements (“felt repr”).

## Usage

This crate is not typically used directly. Instead, depend on either:

- `miden-felt-repr` (preferred), or
- `miden-felt-repr-onchain` (for on-chain code), or
- `miden-felt-repr-offchain` (for off-chain code),

and derive the traits re-exported by those crates.

### Struct example

```rust
use miden_core::Felt;
use miden_felt_repr_offchain::{FromFeltRepr, ToFeltRepr};

#[derive(Debug, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct AccountId {
    prefix: Felt,
    suffix: Felt,
}

let value = AccountId {
    prefix: Felt::new(1),
    suffix: Felt::new(2),
};
let felts = value.to_felt_repr();
let roundtrip = AccountId::from(felts.as_slice());
assert_eq!(roundtrip, value);
```

### Enum example

```rust
use miden_core::Felt;
use miden_felt_repr_offchain::{FromFeltRepr, ToFeltRepr};

#[derive(Debug, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
enum Message {
    Ping,
    Transfer { to: Felt, amount: u32 },
}

// Encoded as: [tag, payload...], where `tag` is the variant ordinal in declaration order.
// Ping -> tag = 0
// Transfer -> tag = 1
let value = Message::Transfer {
    to: Felt::new(7),
    amount: 10,
};
let felts = value.to_felt_repr();
let roundtrip = Message::from(felts.as_slice());
assert_eq!(roundtrip, value);
```

## Felt-repr format

The felt representation of a value is a flat, ordered sequence of field elements (`Felt`). The
encoding is **not** self-describing: it does not include field names, variant names, or schema
metadata.

### Primitives

Provided by the runtime crates:

- `Felt`: 1 felt
- `u64`: 1 felt
- `u32`, `u8`: 1 felt
- `bool`: 1 felt (`0` = `false`, non-zero = `true`)

### Structs

Only structs with **named fields** are supported.

Encoding is the concatenation of field encodings in **declaration order**:

`struct S { a: A, b: B }` → `A` then `B`

Important: **field order is part of the wire format**. Reordering fields (or inserting a field in
the middle) changes the encoding and breaks compatibility.

### Enums

Enums are encoded as:

`tag: u32` (variant ordinal starting at `0` in **declaration order**) followed by the selected
variant payload encoded in declaration order.

- Unit variants have no payload.
- Tuple variants serialize their fields left-to-right.
- Struct variants serialize their named fields in declaration order.

Important: **variant order is part of the wire format**. Reordering variants (or inserting new
variants before existing ones) changes the tag values and breaks compatibility.

Current limitation: explicit discriminants are not supported; tags are always ordinals.

### Nesting

Fields may themselves be types that implement `ToFeltRepr`/`FromFeltRepr`. Nested encodings are
simply concatenated.

## Unsupported items

- Tuple structs and unit structs
- Unions

## Compatibility note

If you need evolvable formats, add an explicit version field and/or build a schema layer on top;
this crate intentionally keeps the encoding minimal and order-dependent.
