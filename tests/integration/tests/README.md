# Process-Isolated Integration Tests

Files in this directory are compiled as separate test binaries. Use this directory only when a test
needs process isolation, such as global logger initialization for `println` output capture.

Prefer normal module tests under `src` for new integration tests unless process-global state makes
that unsafe.
