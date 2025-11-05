# Miden Standard Library

The `miden-stdlib-sys` crate provides a `Felt` type that represents field element in the Miden VM and a standard library for developing Miden programs.

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