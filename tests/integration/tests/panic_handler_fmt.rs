//! Formatted panic output under the abort strategy requires process-isolated logging.

mod common;

use common::panic_handler::{PANIC_HANDLER_MESSAGE, panic_handler_messages_for_fixture};
use midenc_session::PanicStrategy;

#[test]
fn panic_handler_fmt_invoked_with_abort() {
    let messages = panic_handler_messages_for_fixture(
        "../fixtures/components/panic-handler-fmt",
        "panic_handler_fmt::entrypoint",
        PanicStrategy::Abort,
    );
    assert_eq!(
        messages,
        [
            PANIC_HANDLER_MESSAGE,
            "invalid input: Input { value: 50, limits: Limits { threshold: 100 }, \
             above_threshold: false, label: input, owned_label: owned input, optional_value: \
             Some(50), missing_value: None, samples: [50, 0, 100], slice: [50, 100] }"
        ]
    );
}
