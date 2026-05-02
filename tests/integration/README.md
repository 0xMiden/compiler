# miden-integration-tests

This crate contains all of the integration tests for the Miden compiler, specifically integration tests that use Rust source code as the input, so that we can validate the entire compiler pipeline from Rust through to execution of the assembled Miden artifacts.

This test crate _does not_ contain tests which exercise code that requires the protocol/MockChain - see `midenc-integration-network-tests` for that.

The test suite is organized into two major areas:

* Trivial integration tests which are compiled into a single binary and executed in parallel. These are organized under `src` as normal Rust unit tests.
* Complex integration tests which require each test to be compiled into a separate binary, to avoid issues with global resources (e.g. the logger). Each of these tests is executed in parallel with the other separately-compiled tests, but are much more expensive to compile and execute, so writing these type of tests should be avoided unless absolutely necessary.

This crate, when compiled as a library, also provides a number of useful utilities for setting up the environment, performing various types of common builds, and executing packages with the VM. Where possible, tests in the compiler suite that need to perform these types of tasks should make use of these helpers, rather than doing them manually in a way that may fall out of sync with how programs are actually built in practice.

### Notable Tests

This section provides some context on a few of the complex integration tests that are notable for one reason or another.

#### Testing the `println` intrinsic

In order to test this intrinsic, we need to access the log maintained by the debug executor (the engine of the debugger which we use to execute tests), where it writes output written via `println`.

The debug executor relies on a globally-installed logger to collect `println` output, which requires us to build `println` tests as separate integration test binaries, so that each test gets an isolated logger.

There are some additional quirks to make the test output useful for troubleshooting:

- We want to use `miden_debug::logger::DebugLogger` to rely on its test helpers. The global logger can only be initialized once per process, so we must call `DebugLogger::init_for_tests()` before any code that might initialize the logger too (e.g. the compiler).
- Before compilation, we raise the minimum log level to `warn` to suppress unnecessary output during compilation, and then lower it back to `info` before execution of the assembled package, so that we capture the `println` output (which is written to the log at `info` level under the `stdout` target).
