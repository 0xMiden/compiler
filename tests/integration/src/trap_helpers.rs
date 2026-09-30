//! Helpers for asserting on the failure message of a program that is expected to trap.

use std::any::Any;

/// Returns the message of a panic payload caught with [std::panic::catch_unwind], or
/// `"opaque panic"` if the payload is not a string.
pub(crate) fn panic_message(payload: Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "opaque panic".to_string())
}

/// Returns true if the failure `err` reports the assertion `message`, either as its text or as
/// the hash-based error code the VM derives from it.
///
/// The VM reports only the code when the executed forest carries no code-to-message table; the
/// code still ties the failure to the exact message text.
pub(crate) fn trap_matches(err: &str, message: &str) -> bool {
    err.contains(message)
        || err.contains(&miden_core::mast::error_code_from_msg(message).to_string())
}
