use std::sync::Arc;

use midenc_hir::{diagnostics::NullEmitter, testing::TestContext};
use midenc_session::{ColorChoice, Options};

pub fn test_context() -> TestContext {
    let options = Options::default().with_verbosity(midenc_session::Verbosity::Debug);
    let emitter = Arc::new(NullEmitter::new(ColorChoice::Auto));
    TestContext::default_with_opts_and_emitter(options, Some(emitter))
}
