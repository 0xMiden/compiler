//! Immediate abort must trap without running the custom panic handler.

mod common;

use common::panic_handler::{ASSERTION_MESSAGE, PANIC_HANDLER_MESSAGE, panic_handler_messages};
use midenc_session::PanicStrategy;

#[test]
fn panic_handler_not_invoked_with_immediate_abort() {
    let messages = panic_handler_messages(PanicStrategy::ImmediateAbort);
    assert!(
        messages.iter().all(|message| {
            !message.contains(PANIC_HANDLER_MESSAGE) && !message.contains(ASSERTION_MESSAGE)
        }),
        "the panic handler must not run: {messages:?}"
    );
}
