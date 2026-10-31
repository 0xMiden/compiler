//! Panic buffer overflow output requires process-isolated logging.

mod common;

use common::panic_handler::{PANIC_HANDLER_MESSAGE, panic_handler_messages_for_fixture};
use midenc_session::PanicStrategy;

#[test]
fn panic_handler_fmt_overflow_prints_fallback() {
    let messages = panic_handler_messages_for_fixture(
        "../fixtures/components/panic-handler-fmt",
        "panic_handler_fmt::overflow_entrypoint",
        PanicStrategy::Abort,
    );
    assert_eq!(messages, [PANIC_HANDLER_MESSAGE, "panic message formatting failed"]);
}
