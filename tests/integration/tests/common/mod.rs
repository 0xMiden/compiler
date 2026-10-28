//! Shared helpers for process-isolated integration tests.

use miden_debug::logger::DebugLogger;

// Some test binaries include `common` for log capture only, compiling this submodule without using
// it.
#[allow(dead_code)]
pub mod panic_handler;

/// Returns info messages captured after the given log snapshot.
pub fn info_messages_since(before: usize) -> Vec<String> {
    DebugLogger::get()
        .clone_captured()
        .into_iter()
        .skip(before)
        .filter(|entry| entry.level == log::Level::Info)
        .map(|entry| entry.message)
        .collect()
}
