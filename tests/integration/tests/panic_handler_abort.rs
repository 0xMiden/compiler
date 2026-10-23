//! Handler invocation under the abort strategy requires process-isolated logging.

mod common;

use common::panic_handler::{ASSERTION_MESSAGE, PANIC_HANDLER_MESSAGE, panic_handler_messages};
use midenc_session::PanicStrategy;

#[test]
fn panic_handler_invoked_with_abort() {
    let messages = panic_handler_messages(PanicStrategy::Abort);
    assert_eq!(messages, [PANIC_HANDLER_MESSAGE, ASSERTION_MESSAGE]);
}
