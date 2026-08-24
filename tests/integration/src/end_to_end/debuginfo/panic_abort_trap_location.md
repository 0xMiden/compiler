**TODO** Add a test which reproduces this and create an issue

# GitHub issue: trap location lost from executor payload under `panic=abort`

**Title:** With `panic=abort`, trap source location is missing from the executor's error
payload (only present in the stack trace)

**Body:**

## Summary

With `panic=immediate-abort`, a Rust `assert!` failure traps directly at the panic site and
the Miden executor's error message (the panic payload) includes the source location, e.g.
`.../src/lib.rs:26:5`.

With `panic=abort` (the default since the panic strategy became configurable), the panic is
routed through the crate's `#[panic_handler]`, and the same failure produces a payload with
**no source location at all**:

```text
program execution failed at step 242 (cycle 242): assertion failed with error message: entered unreachable code
```

The location is still available, but only in the stack trace printed to the logs, where it
is also attributed differently — `lib.rs:26:13` (the assert's condition expression) instead
of `lib.rs:26:5` (the macro invocation) — and the trapping frames show `<unavailable>`:

```text
 |-> $exec::$main in .../assert-debug-test/src/lib.rs:26:13
 |-> "root_ns:root@1.0.0"::assert_debug_test::core::result::unwrap_failed in <unavailable>
```

## Reproduce

Run the integration test without its `immediate-abort` pin (it compiles
`tests/fixtures/components/assert-debug-test` and executes its entrypoint with an input
that fails `assert!(x > 100)`):

```bash
cargo test -p midenc-integration-tests --lib \
    end_to_end::debuginfo::source_locations::rust_assert_macro_source_location_with_debug_executor
```

or compile any no_std crate with a `#[panic_handler]` and a failing `assert!` with the
default (`abort`) panic strategy and execute it.

## Expected

The trap payload should carry the source location of the failure regardless of the panic
strategy, as it does under `immediate-abort`.

## Notes

- Under `abort`, the trap is raised inside core functions on the panic-handler path (e.g.
  `core::result::unwrap_failed`), whose frames have no source location decorators
  (`<unavailable>`). The payload appears to be built from the trapping instruction's
  decorator, which is why the location is lost even though the user-code frame above it has
  one.
- The integration test
  `end_to_end::debuginfo::source_locations::rust_assert_macro_source_location_with_debug_executor`
  is pinned to `-C panic=immediate-abort` until this is resolved.
