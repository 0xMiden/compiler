# Negative macro expansion tests

These tests verify that incorrect macro usage leads to the expected errors during expansion.

When tests are run, the Rust source files in `./errors/` are compiled. For `./errors/foo.rs`, the expected compilation error is contained in `./errors/foo.stderr`.

## Workflow

**Adding new source files**:

- Add `./errors/your_file.rs` and run tests with the environment variable `TRYBUILD=overwrite`.
- This adds `your_file.stderr`. Inspect it and commit if it contains the expected error.

**Updating `stderr` files**:

This may be needed if an error message changed, for example.

- Run tests with the environment variable `TRYBUILD=overwrite`.
- Inspect the diff and commit if it is valid.

**Fixing regressions**

Failing tests may also indicate regressions when invalid usage is no longer caught after modifying a macro's implementation.

- Fix the macro implementation.
