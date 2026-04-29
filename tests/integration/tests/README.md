# Testing `println`

The debug executor outputs `println` messages to the global logger. Therefore, there are a few quirks related to verifying prints in tests:

- We want to use `miden_debug::logger::DebugLogger` to rely on its test helpers. The global logger can only be initialized once per process, so call `DebugLogger::init_for_tests()` before code that might initialize the logger too (e.g. the compiler).
- To prevent multiple tests from sharing an `env_logger`, put each test in a separate file, so it gets executed in its own processes.
- Before compilation, lower the log level to `Warn` to avoid hitting error paths in the compiler.
- Set the max log level back to `Info` before executing the test package to ensure `println` logs are captured.

