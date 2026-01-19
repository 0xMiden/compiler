# Miden Standard Library

The `miden-stdlib-sys` crate provides low-level bindings for the Miden standard library, and re-exports the unified `Felt` type from the `miden-field` crate.

## Miden VM instructions

See the full instruction list in the [Miden VM book](https://0xMiden.github.io/miden-vm/user_docs/assembly/field_operations.html)

### Not yet implemented Miden VM instructions:

### Field Operations

Missing in IR:
- `ilog2`
- `assert_eqw`
- `eqw`
- `ext2*`

### I/O

Missing in IR:
- `adv*` (advice provider)

### Cryptographic operations

Missing in IR:
- `hash`;
- `hperm`;
- `mtree*`;

### Events, Tracing

Missing in IR:
- `emit`;
- `trace`;
