use crate::reexport::miden_testing::*;

/// Wrapper struct containing
pub struct Context {
    mock_chain: MockChain,
}

impl Context {
    pub fn new(builder: MockChainBuilder) -> Self {
        let mock_chain = builder.build().expect("Failed to build MockChain");

        Context { mock_chain }
    }
}
