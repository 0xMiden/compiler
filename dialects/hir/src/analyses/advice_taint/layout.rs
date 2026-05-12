/// The first results of `hir.adv_pipe` are raw advice values.
pub(super) const ADVICE_PIPE_RAW_RESULT_COUNT: usize = 8;

/// `hir.adv_pipe` stores its destination memory address in stack operand 12.
pub(super) const ADVICE_PIPE_MEMORY_ADDRESS_OPERAND: usize = 12;

/// `hir.adv_pipe` writes one word, i.e. eight field elements, into memory.
pub(super) const ADVICE_PIPE_MEMORY_WRITE_WIDTH: u32 = 8;
