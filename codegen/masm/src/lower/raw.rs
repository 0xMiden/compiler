//! Instruction construction shared by typed legacy callers and registered lowering units.
//!
//! These helpers express target algorithms in raw stack order (top lane first). Callers validate
//! semantic types, ABI counts, and addresses before using them. They neither allocate storage nor
//! establish source evidence; registered definitions pair their output with independent contracts.
//! No allocator, typed stack or storage allocation is available here.
use alloc::{sync::Arc, vec::Vec};

use midenc_hir::Felt;

use crate::{
    Event,
    masm::{self, Instruction as I},
};

/// Construct a fixed helper target from a compiler-owned, syntactically valid library path.
/// Invalid paths are a caller invariant violation and panic.
pub(crate) fn target(path: &str) -> masm::InvocationTarget {
    masm::InvocationTarget::Path(masm::Span::unknown(Arc::from(
        masm::LibraryPath::new(path).unwrap().into_boxed_path(),
    )))
}

/// Collect raw structured MASM without tracking SSA identities or a modeled operand stack.
/// The caller owns stack discipline and supplies any final local offsets used by instructions.
#[derive(Default)]
pub(crate) struct Builder {
    /// Actual instruction and control bodies, later annotated for unit verification.
    pub ops: Vec<masm::Op>,
    /// Source span attached to subsequently constructed operations.
    pub span: masm::SourceSpan,
}

impl Builder {
    pub fn new(span: masm::SourceSpan) -> Self {
        Self {
            ops: Vec::new(),
            span,
        }
    }

    pub fn emit(&mut self, instruction: I) {
        self.ops.push(masm::Op::Inst(masm::Span::new(self.span, instruction)));
    }

    pub fn emit_all(&mut self, instructions: impl IntoIterator<Item = I>) {
        for inst in instructions {
            self.emit(inst);
        }
    }

    pub fn emit_push(&mut self, value: impl Into<masm::PushValue>) {
        self.emit(I::Push(value.into().into()));
    }

    pub fn extend(&mut self, ops: Vec<masm::Op>) {
        self.ops.extend(ops);
    }

    /// Append branches which consume the current boolean condition on the VM stack.
    pub fn branch(&mut self, a: Builder, b: Builder) {
        self.ops.push(masm::Op::If {
            span: self.span,
            then_blk: masm::Block::new(self.span, a.ops),
            else_blk: masm::Block::new(self.span, b.ops),
        });
    }

    /// Wrap a same-context helper invocation in the compiler's frame tracing events.
    pub fn exec(&mut self, target: masm::InvocationTarget) {
        self.emit_all([
            I::EmitImm(Event::FrameStart.into()),
            I::Exec(target),
            I::EmitImm(Event::FrameEnd.into()),
        ]);
    }
}

/// Replace `[element_addr, byte_offset]` with a masked 1-, 8-, or 16-bit value.
/// The caller supplies an offset whose value fits in the addressed field element.
pub(crate) fn load_small(bits: usize, span: masm::SourceSpan) -> Vec<masm::Op> {
    let mut b = Builder::new(span);

    // Stack: [element_addr, byte_offset]

    // First, load the aligned word containing our value
    // We need to temporarily move the offset out of the way
    b.emit(masm::Instruction::Swap1); // [byte_offset, element_addr]
    b.emit(masm::Instruction::Dup1); // [element_addr, byte_offset, element_addr]
    b.emit(masm::Instruction::MemLoad); // [loaded_word, byte_offset, element_addr]

    // Now we need to extract the correct byte(s) based on the offset
    // Stack: [loaded_word, byte_offset, element_addr]
    b.emit(masm::Instruction::Swap1); // [byte_offset, loaded_word, element_addr]

    // Shift right by (offset * 8) bits to get our byte at the low end
    b.emit_push(8u8); // [8, byte_offset, loaded_word, element_addr]
    b.emit_all([
        masm::Instruction::U32WrappingMul, // [offset*8, loaded_word, element_addr]
        masm::Instruction::U32Shr,         // [shifted_word, element_addr]
    ]);

    // Clean up the element address
    b.emit(masm::Instruction::Swap1); // [element_addr, shifted_word]
    b.emit(masm::Instruction::Drop); // [shifted_word]

    // Mask to get only the bits we need
    let mask = match bits {
        1 => 0x1,
        8 => 0xff,
        16 => 0xffff,
        _ => unreachable!("load_small called with non-small type"),
    };

    b.emit_push(mask as u32);
    b.emit(masm::Instruction::U32And);
    b.ops
}

/// Consume `[element_addr, byte_offset, value]` and replace just the selected bits in memory.
/// `type_size` is a subword bit width; the value must fit that width without crossing the element.
pub(crate) fn store_small(type_size: u32, span: masm::SourceSpan) -> Vec<masm::Op> {
    let mut b = Builder::new(span);

    // Stack: [addr, offset, value]
    // Load the current aligned value
    b.emit_all([
        masm::Instruction::Dup0,    // [addr, addr, offset, value]
        masm::Instruction::MemLoad, // [prev, addr, offset, value]
    ]);

    // Calculate bit offset and create mask
    let type_size_mask = (1u32 << type_size) - 1;
    b.emit(masm::Instruction::Dup2); // [offset, prev, addr, offset, value]
    b.emit_push(8u8); // [8, offset, prev, addr, offset, value]
    b.emit(masm::Instruction::U32WrappingMul); // [bit_offset, prev, addr, offset, value]
    b.emit_push(type_size_mask); // [type_mask, bit_offset, prev, addr, offset, value]
    b.emit_all([
        masm::Instruction::Swap1, // [bit_offset, type_mask, prev, addr, offset, value]
        masm::Instruction::U32Shl, // [shifted_mask, prev, addr, offset, value]
        masm::Instruction::U32Not, // [mask, prev, addr, offset, value]
        masm::Instruction::Swap1, // [prev, mask, addr, offset, value]
        masm::Instruction::U32And, // [masked_prev, addr, offset, value]
    ]);

    // Shift value to correct position and combine
    b.emit_all([
        masm::Instruction::MovUp3, // [value, masked_prev, addr, offset]
        masm::Instruction::MovUp3, // [offset, value, masked_prev, addr]
    ]);
    b.emit_push(8u8); // [8, offset, value, masked_prev, addr]
    b.emit_all([
        masm::Instruction::U32WrappingMul, // [bit_offset, value, masked_prev, addr]
        masm::Instruction::U32Shl,         // [shifted_value, masked_prev, addr]
        masm::Instruction::U32Or,          // [new_value, addr]
        masm::Instruction::Swap1,          // [addr, new_value]
        masm::Instruction::MemStore,       // []
    ]);
    b.ops
}

fn assertz(message: impl Into<Arc<str>>) -> I {
    I::AssertzWithError(message.into().into())
}

/// Consume `[src, dst, count]` in byte coordinates, preserving overlap semantics.
/// Checked range arithmetic traps on overflow. Aligned ranges call the element-copy helper;
/// other ranges copy bytes in the direction needed to preserve unread source data.
pub(crate) fn byte_memmove(span: masm::SourceSpan) -> Vec<masm::Op> {
    let mut b = Builder::new(span);
    // Bounds checks preserve [src,dst,count].
    b.emit_all([
        I::Dup2,
        I::Dup1,
        I::Dup1,
        I::U32OverflowingAdd,
        assertz("memmove source range is out of bounds"),
        I::Drop,
        I::Dup2,
        I::Dup1,
        I::U32OverflowingAdd,
        assertz("memmove destination range is out of bounds"),
        I::Drop,
        I::Drop,
    ]);
    b.emit_all([
        I::Dup0,
        I::U32DivModImm(4.into()),
        I::Swap1,
        I::Drop,
        I::EqImm(Felt::ZERO.into()),
        I::Dup2,
        I::U32DivModImm(4.into()),
        I::Swap1,
        I::Drop,
        I::EqImm(Felt::ZERO.into()),
        I::And,
        I::Dup3,
        I::U32DivModImm(4.into()),
        I::Swap1,
        I::Drop,
        I::EqImm(Felt::ZERO.into()),
        I::And,
    ]);
    let mut aligned = Builder::new(span);
    aligned.emit_all([
        I::U32DivModImm(4.into()),
        assertz("memmove byte-copy fast path expected the source pointer to be 4-byte aligned"),
        I::Swap1,
        I::U32DivModImm(4.into()),
        assertz(
            "memmove byte-copy fast path expected the destination pointer to be 4-byte aligned",
        ),
        I::Swap2,
        I::U32DivModImm(4.into()),
        assertz("memmove byte-copy fast path expected the byte count to be divisible by 4"),
    ]);
    aligned.exec(target("::intrinsics::mem::memmove_elements"));
    let mut scalar = Builder::new(span);
    scalar.emit_all([I::Dup1, I::Dup1, I::U32Gt]);
    let mut descending = Builder::new(span);
    descending.emit_all([I::MovUp2, I::U32WrappingSubImm(1.into())]);
    descending.emit_push(u32::MAX);
    descending.emit(I::MovDn3);
    descending.emit_push(u32::MAX);
    descending.emit(I::MovDn4);
    let mut ascending = Builder::new(span);
    ascending.emit_push(1u32);
    ascending.emit(I::MovDn3);
    ascending.emit_push(0u32);
    scalar.branch(descending, ascending);
    scalar.emit_all([I::Dup0, I::Dup4, I::Neq]);
    let mut body = Builder::new(span);
    body.emit_all([I::Dup2, I::Dup1, I::U32WrappingAdd, I::Dup2, I::Dup2, I::U32WrappingAdd]);
    body.emit_all([I::U32DivModImm(4.into()), I::Swap1]);
    body.extend(load_small(8, span));
    body.emit(I::Swap1);
    body.emit_all([I::U32DivModImm(4.into()), I::Swap1]);
    body.extend(store_small(8, span));
    body.emit_all([I::Dup4, I::U32WrappingAdd, I::Dup0, I::Dup4, I::Neq]);
    scalar.ops.push(masm::Op::While {
        span,
        body: masm::Block::new(span, body.ops),
    });
    scalar.emit_all([I::DropW, I::Drop]);
    b.branch(aligned, scalar);
    b.ops
}

/// Append zeros below `inputs` argument lanes to form the VM's sixteen-lane input window.
/// The meaningful input order is preserved. Counts above sixteen violate the caller contract.
pub(crate) fn pad_inputs(inputs: usize, span: masm::SourceSpan) -> Vec<masm::Op> {
    assert!(inputs <= 16);
    let mut b = Builder::new(span);
    for _ in inputs..16 {
        b.emit_push(0u32);
        if inputs == 1 {
            b.emit(I::Swap1);
        } else if inputs > 1 {
            b.emit(crate::plan::Primitive::MoveDown(inputs as u8).instruction().unwrap());
        }
    }
    b.ops
}

/// Pad the payload window, then load six protocol-prefix lanes from assigned scalar locals.
/// `offsets` are final element offsets in prefix order, never symbolic slot identities.
pub(crate) fn fpi_prefix(
    inputs: usize,
    offsets: [u16; 6],
    span: masm::SourceSpan,
) -> Vec<masm::Op> {
    let mut b = Builder::new(span);
    b.extend(pad_inputs(inputs, span));
    for offset in offsets.into_iter().rev() {
        b.emit_all([I::Locaddr(offset.into()), I::MemLoad]);
    }
    b.ops
}

/// Reject an all-zero root, run the supplied store sequence, then consume its four lanes.
/// The caller's store instructions must preserve those lanes and address an aligned word.
pub(crate) fn stage_root(
    store: impl IntoIterator<Item = I>,
    message: &str,
    span: masm::SourceSpan,
) -> Vec<masm::Op> {
    let mut b = Builder::new(span);
    b.emit_all([I::Dup0, I::EqImm(Felt::ZERO.into())]);
    for dup in [I::Dup2, I::Dup3, I::Dup4] {
        b.emit_all([dup, I::EqImm(Felt::ZERO.into()), I::And]);
    }
    b.emit(assertz(message));
    b.emit_all(store);
    b.emit(I::DropW);
    b.ops
}
