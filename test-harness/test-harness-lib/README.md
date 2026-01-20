# miden-test-harness

A custom test harness for Rust code targeting the Miden contracts.

It re-exports the `miden-test-harness-macros` which comes with the `#[miden_test]` and `#[miden_test_suite]`.

## `#[miden_test_suite]`
This macro wraps the `mod tests` module which contains all the tests. It's used by the test harness internally and is thusly required in order for the test harness to work correctly.
For example:
```rust
#[miden_test_suite]
mod tests {
      (...)
}
```

## `#[miden_test]`

This macro serves as a `#[test]` equivalent but for Miden's test harness.
Notably, functions marked with `#[miden_test]` can recognize some **special** arguments, currently:

- `var: Package`: The `Package` variable will have the resulting `.masp` file loaded into it. Only 1 `Package` variable is allowed.
- `var: MockChainBuilder`: This simply instantiates a `MockChainBuilder`. Only 1 `MockChainBuilder` variable is allowed.

To see examples see: `tests/examples/counter`.
