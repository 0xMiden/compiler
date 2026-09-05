# Differential regression investigation

The two pinned differential cases now run normally, alongside their randomized
siblings. Investigation used both the repository toolchain and the older Wasm
producer, with the same Miden compiler. No Miden code-generation change was needed
for these cases.

## Signed extension and wide products

For `sext_shapes_repro`, inputs `(3022925119, 3340151117)` produced:

| Rust Wasm producer | Native Rust | Wasmi on emitted Wasm | Miden on emitted Wasm |
| --- | ---: | ---: | ---: |
| `nightly-2026-04-30`: `rustc 1.97.0-nightly (c935696dd 2026-04-29)` | 3550407903 | 3550391763 | 3550391763 |
| `nightly-2026-09-01`: `rustc 1.100.0-nightly (0dfb098f3 2026-08-31)` | 3550407903 | 3550407903 | 3550407903 |

The producer versions come from the preserved Wasm `producers` sections. Wasmi
executed those exact artifacts with wide arithmetic enabled, independently of HIR
translation and Miden code generation. Their SHA-256 hashes were:

- Older artifact: `8d9e3a4d9c1994d6e13f3a08c1095f4ecc14d2cb9c617cc3f3f04e8d5a251eee`
- Current artifact: `2715e0c43c8fc1274efbc5d644fdf911c4f6a1d5c77638e548f1fd4735670d3e`

The older artifact loads zero-initialized local 3 **before** the first signed wide
multiply stores its high result into that local. The previously loaded zero is
then XORed with the sign-extended low byte, followed by `i64.extend16_s`. It never
incorporates the source program's sign-extended high-product low 16 bits at that
point. For the pinned input, that omitted contribution is `16652` (`0x410c`),
exactly the XOR difference between the native and Wasm results.

This establishes that the discrepancy already exists in the older producer's
Wasm output; Miden agrees with that output's semantics. It does not identify the
responsible Rust/LLVM internal pass or establish an upstream issue number.
Changing Miden to match the native result for that artifact would change the
meaning of the supplied Wasm.

[The fixed-WAT regression](../../tests/integration/src/end_to_end/wasm_translation/wide_product.rs)
preserves the old computation and local ordering, removing unused memory/global
and producer metadata. It compares Miden execution with Wasmi on the pinned input
and additional sign-boundary inputs, so Rust optimization changes cannot remove
coverage of the shape. The Rust differential case separately checks the intended
source computation with the supported producer.

## Switch selectors

`switch_shapes_repro`, inputs `(1669775643, 1062584501)`, passes with both producer
toolchains. The relevant historical compiler correction is commit `8710eee5b`
(`fix(wasm): bitcast the br_table selector instead of range-checking it`), already
present in the audit baseline. That change replaces a checked signed-to-unsigned
cast with a bitcast: a Wasm `br_table` selector with its high bit set is a valid
unsigned default-arm selector, including when produced by wrapping subtraction.
The former checked cast could trap with the same `value does not fit in i32`
message recorded by the ignored case.

The commit includes [issue1243 coverage](../../tests/integration/src/end_to_end/regressions/issue1243.rs)
for wrapped and directly high-bit-set selectors. This investigation verified the
historical source change and the pinned case's current behavior; it did not revert
that commit to reproduce the old trap.

Both pinned cases also pass with the current producer when the C02 coercion-fold
change is temporarily reverted. Their success is therefore not evidence that C02
fixed either historical discrepancy.
