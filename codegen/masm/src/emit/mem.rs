use miden_core::Felt;
use midenc_hir::{AddressSpace, ArrayType, PointerType, SourceSpan, StructType, Type};

use super::{OpEmitter, addressing::ByteAddressing, masm};
use crate::{OperandStack, lower::NativePtr};

/// Whether a scalar of type `ty` is represented by a single stack element.
/// Sub-word integers still require masking when accessing a shared memory element.
fn is_single_element_scalar(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Felt
            | Type::I32
            | Type::U32
            | Type::Ptr(_)
            | Type::I16
            | Type::U16
            | Type::I8
            | Type::U8
            | Type::I1
    )
}

/// How a copy between two memory ranges treats ranges that overlap.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum CopyOverlap {
    /// The ranges may overlap, the destination receives the values the source range held before
    /// the copy.
    Allowed,
    /// The ranges must be disjoint, a copy of a non-zero length between overlapping ranges traps.
    Forbidden,
}

impl CopyOverlap {
    /// The message of the trap raised when the ranges of a [CopyOverlap::Forbidden] copy overlap.
    ///
    /// It is the message of the assertion in the `memcopy_*` procedures of the core library, so
    /// that the trap reads the same whichever routine performs the copy.
    const OVERLAP_MESSAGE: &'static str = "source and destination ranges must not overlap";

    /// The name of the copy in the trap messages.
    const fn name(self) -> &'static str {
        match self {
            Self::Allowed => "memmove",
            Self::Forbidden => "memcpy",
        }
    }

    /// The procedure which copies `n` field elements between element addresses.
    ///
    /// Narrow memcpy uses the core helper's overlap assertion. Wide memcpy checks byte ranges
    /// before conversion, then uses the move helper, whose endpoints can reach native 2^32.
    const fn elements_procedure(self, mode: ByteAddressing) -> &'static str {
        match (self, mode) {
            (Self::Allowed, _) | (_, ByteAddressing::Wide) => "::intrinsics::mem::memmove_elements",
            (Self::Forbidden, ByteAddressing::Narrow) => "::miden::core::mem::memcopy_elements",
        }
    }

    /// The procedure which copies `n` words between word-aligned element addresses.
    ///
    /// Narrow memcpy uses the core helper's overlap assertion. Wide memcpy checks byte ranges
    /// before conversion, then uses the move helper, whose endpoints can reach native 2^32.
    const fn words_procedure(self, mode: ByteAddressing) -> &'static str {
        match (self, mode) {
            (Self::Allowed, _) | (_, ByteAddressing::Wide) => "::intrinsics::mem::memmove_words",
            (Self::Forbidden, ByteAddressing::Narrow) => "::miden::core::mem::memcopy_words",
        }
    }
}

/// Allocation
impl OpEmitter<'_> {
    /// Emit the loop header for a counted `while.true` loop.
    ///
    /// The caller provides the concrete `dup` instruction needed to bring `count` to the top of
    /// the stack after the loop index has been seeded with zero.
    ///
    /// Stack transition:
    ///
    /// - Before: `[loop_state..]`
    /// - After: `[count > 0, i = 0, loop_state..]`
    ///
    /// For example:
    ///
    /// - `memset`: `[dst, count, value..] -> [count > 0, i = 0, dst, count, value..]`
    fn emit_counted_loop_header(&mut self, count_dup: masm::Instruction, span: SourceSpan) {
        self.emit_push(0u32, span);
        self.emit(count_dup, span);
        self.emit_push(0u32, span);
        self.emit(masm::Instruction::U32Gt, span);
    }

    /// Emit the loop back-edge condition for a counted `while.true` loop.
    ///
    /// The caller provides the concrete `dup` instruction needed to bring `count` to the top of
    /// the stack after incrementing the loop index.
    ///
    /// Stack transition:
    ///
    /// - Before: `[i, loop_state..]`
    /// - After: `[i + 1 < count, i + 1, loop_state..]`
    ///
    /// For example:
    ///
    /// - `memset`: `[i, dst, count, value..] -> [i + 1 < count, i + 1, dst, count, value..]`
    fn emit_counted_loop_next_condition(&mut self, count_dup: masm::Instruction, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::U32WrappingAddImm(1.into()),
                masm::Instruction::Dup0,
                count_dup,
                masm::Instruction::U32Lt,
            ],
            span,
        );
    }

    /// Convert the byte pointer on top of the stack to a word-aligned element address.
    ///
    /// This traps unless the input byte address is aligned to a 16-byte Miden word boundary.
    fn emit_word_aligned_element_addr_from_byte_ptr(&mut self, span: SourceSpan) {
        self.emit_all(
            self.byte_addressing.aligned_element_addr(
                16,
                Self::assertz_with_message_inst(
                    "expected a 16-byte-aligned byte pointer for the word-copy fast path",
                    span,
                ),
            ),
            span,
        );
    }

    /// Build a MASM block whose stack protocol is managed by the caller.
    ///
    /// This is used for branch bodies which operate on a known stack shape from the enclosing
    /// emitter, but which do not need to synchronize typed operand-stack state back to it.
    fn build_masm_block(
        &mut self,
        span: SourceSpan,
        emit: impl FnOnce(&mut OpEmitter<'_>),
    ) -> masm::Block {
        let mut ops = Vec::default();
        let mut stack = OperandStack::new(self.context_rc());
        let mut emitter = OpEmitter::new(self.invoked, &mut ops, &mut stack)
            .with_byte_addressing(self.byte_addressing);
        emit(&mut emitter);
        masm::Block::new(span, ops)
    }

    /// Append an already constructed algorithm and register every invocation nested within it.
    ///
    /// This bridges shared raw instruction builders into typed emission. It deliberately does not
    /// update the typed operand stack: the caller must account for the algorithm's complete input
    /// and output shape. Registering nested invocations keeps dependency discovery correct even
    /// when an algorithm contains branches or loops.
    pub(crate) fn emit_raw_body(&mut self, ops: Vec<masm::Op>) {
        fn register(emitter: &mut OpEmitter<'_>, op: &masm::Op) {
            match op {
                masm::Op::Inst(inst) => emitter.maybe_register_invoke(inst.inner()),
                masm::Op::If {
                    then_blk, else_blk, ..
                } => {
                    for op in then_blk.iter().chain(else_blk.iter()) {
                        register(emitter, op);
                    }
                }
                masm::Op::While { body, .. } | masm::Op::Repeat { body, .. } => {
                    for op in body.iter() {
                        register(emitter, op);
                    }
                }
                masm::Op::DoWhile {
                    body, condition, ..
                } => {
                    for op in body.iter().chain(condition.iter()) {
                        register(emitter, op);
                    }
                }
            }
        }
        for op in &ops {
            register(self, op);
        }
        self.current_block.extend(ops);
    }

    /// Grow the heap (from the perspective of Wasm programs) by N pages, returning the previous
    /// size of the heap (in pages) if successful, or -1 if the heap could not be grown.
    pub fn mem_grow(&mut self, span: SourceSpan) {
        let _num_pages = self.stack.pop().expect("operand stack is empty");
        self.raw_exec("::intrinsics::mem::memory_grow", span);
        self.push(Type::U32);
    }

    /// Returns the size (in pages) of the heap (from the perspective of Wasm programs)
    pub fn mem_size(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::mem::memory_size", span);
        self.push(Type::U32);
    }

    /// Load two VM words from memory and update the top-13 stack window.
    pub fn mem_stream(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::MemStream, 13, 13, "mem_stream", span);
    }
}

/// Loads
impl OpEmitter<'_> {
    /// Push an element-space pointer to a source local at its finalized element offset.
    ///
    /// Selection supplies the checked pointee type and frame binding, so this helper needs no HIR
    /// local object and makes no storage-allocation decision. `offset` is relative to the procedure
    /// frame and must already have passed layout and capacity checks.
    pub(crate) fn local_address_typed(&mut self, ty: Type, offset: u16, span: SourceSpan) {
        self.emit(masm::Instruction::Locaddr(offset.into()), span);
        self.push(Type::from(PointerType::new_with_address_space(ty, AddressSpace::Element)));
    }

    /// Push a byte-space pointer to a local at its finalized element offset.
    pub(crate) fn local_byte_address_typed(&mut self, ty: Type, offset: u16, span: SourceSpan) {
        self.emit(masm::Instruction::Locaddr(offset.into()), span);
        self.emit(masm::Instruction::MulImm(Felt::from(4u32).into()), span);
        self.push(Type::from(PointerType::new(ty)));
    }

    /// Load a value corresponding to the pointee type of a pointer operand on the stack.
    ///
    /// The type of the pointer determines what address space the pointer value represents;
    /// either the Miden-native address space (word-addressable), or the IR's byte-addressable
    /// address space.
    pub fn load(&mut self, ty: Type, span: SourceSpan) {
        let ptr = self.stack.pop().expect("operand stack is empty");
        match ptr.ty() {
            Type::Ptr(ref ptr_ty) => {
                assert_eq!(
                    ty.size_in_bits(),
                    ptr_ty.pointee().size_in_bits(),
                    "The size of the type of the value being loaded ({ty}) is different from the \
                     type of the pointee ({})",
                    ptr_ty.pointee()
                );
                // Element pointers have no byte offset, but a sub-word pointee can still share
                // its element with other data. Extract only its bits without dynamic-offset
                // intrinsics, including when loading procedure locals under stack pressure.
                if !ptr_ty.is_byte_pointer() && is_single_element_scalar(&ty) {
                    self.emit(masm::Instruction::MemLoad, span);
                    if ty.size_in_bits() < 32 {
                        self.emit(masm::Instruction::U32Cast, span);
                        self.const_mask_u32((1u32 << ty.size_in_bits()) - 1, span);
                    }
                    self.push(ty);
                    return;
                }
                // Convert the pointer to a native pointer representation
                self.convert_to_native_ptr(ptr_ty, span);
                self.load_native(&ty, span);
                self.push(ty);
            }
            ty if !ty.is_pointer() => {
                panic!("invalid operand to load: expected pointer, got {ty}")
            }
            ty => unimplemented!("load support for pointers of type {ty} is not implemented"),
        }
    }

    /// Load a value of type `ty` from `addr`.
    ///
    /// NOTE: The address represented by `addr` is in the IR's byte-addressable address space.
    #[allow(unused)]
    pub fn load_imm(&mut self, addr: u32, ty: Type, span: SourceSpan) {
        let ptr = NativePtr::from_ptr(addr);
        match &ty {
            Type::I128 | Type::U128 => self.load_quad_word(Some(ptr), span),
            Type::I64 | Type::U64 => self.load_double_word_int(Some(ptr), span),
            Type::Felt => self.load_felt(Some(ptr), span),
            Type::I32 | Type::U32 | Type::Ptr(_) => self.load_word(Some(ptr), span),
            Type::I16 | Type::U16 | Type::U8 | Type::I8 | Type::I1 => {
                self.load_small(&ty, Some(ptr), span);
            }
            ty => todo!("support for loading {ty} is not yet implemented"),
        }
        self.push(ty);
    }

    /// Emit a sequence of instructions to translate a raw pointer value to a native pointer value,
    /// as a tuple of `(element_addr, byte_offset)`, in that order on the stack.
    ///
    /// Instructions which must act on a pointer will expect the stack to have these values in that
    /// order so that they can perform any necessary re-alignment.
    fn convert_to_native_ptr(&mut self, ty: &PointerType, span: SourceSpan) {
        if ty.is_byte_pointer() {
            self.emit_all(self.byte_addressing.native_ptr(), span);
        } else {
            self.emit_push(0u8, span);
            self.emit(masm::Instruction::Swap1, span);
        }
    }

    /// Push a [NativePtr] value to the operand stack in the expected stack representation.
    fn push_native_ptr(&mut self, ptr: NativePtr, span: SourceSpan) {
        self.emit_push(ptr.offset, span);
        self.emit_push(ptr.addr, span);
    }

    /// Load through an already normalized `[element_addr, byte_offset]` pair.
    /// Shared by scalar accesses and validated aggregate lanes; does not update typed operands.
    pub(crate) fn load_native(&mut self, ty: &Type, span: SourceSpan) {
        match ty {
            Type::I128 | Type::U128 => self.load_quad_word(None, span),
            Type::I64 | Type::U64 => self.load_double_word_int(None, span),
            Type::Felt => self.load_felt(None, span),
            _ if ty.size_in_bits() == 32 => self.load_word(None, span),
            _ if (1..32).contains(&ty.size_in_bits()) => self.load_small(ty, None, span),
            ty => todo!("support for loading {ty} is not yet implemented"),
        }
    }

    /// Load a field element from a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer pair `(element_addr, byte_offset)` is expected on the stack if an
    /// immediate is not given.
    fn load_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_felt_imm(imm, span);
        }

        self.raw_exec("::intrinsics::mem::load_felt", span);
    }

    fn load_felt_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.is_element_aligned(), "felt values must be naturally aligned");
        self.emit(masm::Instruction::MemLoadImm(ptr.addr.into()), span);
    }

    /// Loads a single 32-bit machine word, i.e. a single field element, not the Miden notion of a
    /// word
    ///
    /// Expects a native pointer pair `(element_addr, byte_offset)` on the stack if an immediate
    /// address is not given.
    fn load_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_word_imm(imm, span);
        }

        self.raw_exec("::intrinsics::mem::load_sw", span);
    }

    /// Loads a single 32-bit machine word from the given immediate address.
    fn load_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            // The element is loaded as it is, without a range check, as `load_sw` does
            self.emit(masm::Instruction::MemLoadImm(ptr.addr.into()), span);
        } else {
            // Delegate to load_sw intrinsic to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("::intrinsics::mem::load_sw", span);
        }
    }

    /// Load a 64-bit word from the given address.
    ///
    /// After execution, the stack contains `[lo, hi]` (lo on top, LE limb order).
    fn load_double_word_int(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            self.load_double_word_imm(imm, span);
        } else {
            self.raw_exec("::intrinsics::mem::load_dw", span);
        }
    }

    /// Load a sub-word value (u8, u16, etc.) from memory
    ///
    /// For sub-word loads, we need to load from the element-aligned address
    /// and then extract the correct bits based on the byte offset.
    ///
    /// If `ptr` is None, this function expects the stack to contain: [element_addr, byte_offset]
    /// If `ptr` is Some, it uses the immediate pointer value.
    ///
    /// The approach:
    /// 1. Load the 32-bit word containing the target byte(s)
    /// 2. Shift right by (byte_offset * 8) bits to move the target byte(s) to the low end
    /// 3. Mask to extract only the bits we need based on the type size
    ///
    /// After execution, the stack will contain: `[loaded_value]`
    fn load_small(&mut self, ty: &Type, ptr: Option<NativePtr>, span: SourceSpan) {
        let bits = ty.size_in_bits();
        assert!((1..32).contains(&bits), "load_small requires a sub-word width");
        // If we have an immediate pointer, handle it based on alignment
        if let Some(imm) = ptr {
            if imm.is_element_aligned() {
                // For aligned loads, we can use MemLoadImm
                self.emit(masm::Instruction::MemLoadImm(imm.addr.into()), span);
                // The value is already loaded, just need to mask it
                self.emit(masm::Instruction::U32Cast, span);
                self.emit_push((1u32 << bits) - 1, span);
                self.emit(masm::Instruction::U32And, span);
                return;
            } else {
                // [addr, offset], as the dynamic paths below expect
                self.push_native_ptr(imm, span);
            }
        }

        if ty.size_in_bits() == 16 {
            self.load_16bit_dynamic(span);
            return;
        }
        if bits <= 8 {
            self.load_small_from_current_element(ty, span);
            return;
        }
        // Read a second cell only when the represented bits actually cross its boundary.
        use masm::Instruction as I;
        self.emit(I::Dup1, span);
        self.emit_push(((32 - bits) / 8) as u32, span);
        self.emit(I::U32Gt, span);
        let crossed = self.build_masm_block(span, |emitter| {
            emitter.load_word(None, span);
            emitter.const_mask_u32((1u32 << bits) - 1, span);
        });
        self.current_block.push(masm::Op::If {
            span,
            then_blk: crossed,
            else_blk: masm::Block::new(span, crate::lower::raw::load_small(bits, span)),
        });
    }

    /// Load a sub-word value which is fully contained in the current 32-bit element.
    ///
    /// Stack transition: `[addr, offset] -> [value]`.
    fn load_small_from_current_element(&mut self, ty: &Type, span: SourceSpan) {
        self.emit_raw_body(crate::lower::raw::load_small(ty.size_in_bits(), span));
    }

    /// Load a 16-bit value from a dynamic native pointer tuple.
    ///
    /// This delegates to a dedicated intrinsic which owns the complete stack protocol for both the
    /// within-element and cross-element cases.
    ///
    /// Stack transition: `[addr, offset] -> [value]`.
    fn load_16bit_dynamic(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::mem::load_u16", span);
    }

    fn load_double_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            // The elements are loaded as they are, without a range check, as `load_dw` does:
            // [lo, hi]
            self.emit_all(
                [
                    masm::Instruction::MemLoadImm((ptr.addr + 1).into()),
                    masm::Instruction::MemLoadImm(ptr.addr.into()),
                ],
                span,
            )
        } else {
            // Delegate to load_dw to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("::intrinsics::mem::load_dw", span);
        }
    }

    /// Load a quartet of machine words (32-bit elements) to the operand stack
    fn load_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_quad_word_imm(imm, span);
        }
        self.raw_exec("::intrinsics::mem::load_qw", span);
    }

    fn load_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // When aligned, the elements are loaded as they are, without a range check, as `load_qw`
        // does
        if ptr.is_word_aligned() {
            self.emit_all(
                [
                    // [w0, w1, w2, w3]
                    masm::Instruction::PadW,
                    masm::Instruction::MemLoadWLeImm(ptr.addr.into()),
                ],
                span,
            );
        } else if ptr.is_element_aligned() {
            self.emit_all(
                [
                    masm::Instruction::MemLoadImm((ptr.addr + 3).into()),
                    masm::Instruction::MemLoadImm((ptr.addr + 2).into()),
                    masm::Instruction::MemLoadImm((ptr.addr + 1).into()),
                    masm::Instruction::MemLoadImm(ptr.addr.into()),
                ],
                span,
            );
        } else {
            // Delegate to load_qw to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("::intrinsics::mem::load_qw", span);
        }
    }

    /// This handles emitting code that handles aligning an unaligned double machine-word value
    /// which is split across three machine words (field elements).
    ///
    /// To recap:
    ///
    /// * A machine word is a 32-bit chunk stored in a single field element
    /// * A double word is a pair of 32-bit chunks
    /// * A quad word is a quartet of 32-bit chunks (i.e. a Miden "word")
    /// * An unaligned double-word requires three 32-bit chunks to represent, since the first chunk
    ///   does not contain a full 32-bits, so an extra is needed to hold those bits.
    ///
    /// As an example, assume the pointer we are dereferencing is a u64 value,
    /// which has 8-byte alignment, and the value is stored 40 bytes from the
    /// nearest quad-word-aligned boundary. To load the value, we must fetch
    /// the full quad-word from the aligned address, drop the first word, as
    /// it is unused, and then recombine the 64 bits we need spread across
    /// the remaining three words to obtain the double-word value we actually want.
    ///
    /// The data, on the stack, is shown below:
    ///
    /// ```text,ignore
    /// # If we visualize which bytes are contained in each 32-bit chunk on the stack, we get:
    /// [0..=4, 5..=8, 9..=12]
    ///
    /// # These byte indices are relative to the nearest word-aligned address, in the same order
    /// # as they would occur in a byte-addressable address space. The significance of each byte
    /// # depends on the value being dereferenced, but Miden is a little-endian machine, so typically
    /// # the most significant bytes come first (i.e. also commonly referred to as "high" vs "low" bits).
    /// #
    /// # If we visualize the layout of the bits of our u64 value spread across the three chunks, we get:
    /// [00000000111111111111111111111111, 111111111111111111111111111111, 11111111111111111111111100000000]
    /// ```
    ///
    /// As illustrated above, what should be a double-word value is occupying three words. To
    /// "realign" the value, i.e. ensure that it is naturally aligned and fits in two words, we
    /// have to perform a sequence of shifts and masks to get the bits where they belong. This
    /// function performs those steps, with the assumption that the caller has three values on
    /// the operand stack representing any unaligned double-word value
    #[allow(unused)]
    fn realign_double_word(&mut self, _ptr: NativePtr, span: SourceSpan) {
        self.raw_exec("::intrinsics::mem::realign_dw", span);
    }

    /// This handles emitting code that handles aligning an unaligned quad machine-word value
    /// which is split across five machine words (field elements).
    ///
    /// To recap:
    ///
    /// * A machine word is a 32-bit chunk stored in a single field element
    /// * A double word is a pair of 32-bit chunks
    /// * A quad word is a quartet of 32-bit chunks (i.e. a Miden "word")
    /// * An unaligned quad-word requires five 32-bit chunks to represent, since the first chunk
    ///   does not contain a full 32-bits, so an extra is needed to hold those bits.
    ///
    /// See the example in [OpEmitter::realign_quad_word] for more details on how bits are
    /// laid out in each word, and what is required to realign unaligned words.
    #[allow(unused)]
    fn realign_quad_word(&mut self, ptr: NativePtr, span: SourceSpan) {
        // The stack starts as: [chunk_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
        //
        // We will refer to the parts of our desired quad-word value
        // as four parts, `x_hi2`, `x_hi1`, `x_lo2`, and `x_lo1`, where
        // the integer suffix should appear in decreasing order on the
        // stack when we're done.
        self.emit_all(
            [
                // Re-align the high bits by shifting out the offset
                //
                // This gives us the first half of `x_hi2`.
                //
                // [x_hi2_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk__lo]
                masm::Instruction::U32ShlImm(ptr.offset.into()),
                // Move the value below the other chunks temporarily
                //
                // [chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk__lo, x_hi2_hi]
                masm::Instruction::MovDn5,
                // We must split the `chunk_mid_hi` chunk into two parts,
                // one containing the bits to be combined with `x_hi2_hi`;
                // the other to be combined with `x_hi1_hi`.
                //
                // First, we duplicate the chunk, since we need two
                // copies of it:
                //
                // [chunk_mid_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2_hi]
                masm::Instruction::Dup0,
                // Then, we shift the chunk right by 32 - offset bits,
                // re-aligning the low bits of `x_hi2`, and isolating them.
                //
                // [x_hi2_lo, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2_hi]
                masm::Instruction::U32ShrImm((32 - ptr.offset).into()),
                // Move the high bits of `x_hi2` back to the top
                //
                // [x_hi2_hi, x_hi2_lo, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
                masm::Instruction::MovUp3,
                // OR the two parts of the `x_hi2` chunk together
                //
                // [x_hi2, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
                masm::Instruction::U32Or,
                // Move `x_hi2` to the bottom for later
                //
                // [chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                masm::Instruction::MovDn5,
                // Now, we need to re-align the high bits of `x_hi1` by shifting
                // the remaining copy of `chunk_mid_hi`, similar to what we did for `x_hi2`
                //
                // This gives us the first half of `x_hi1`
                //
                // [x_hi1_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                masm::Instruction::U32ShlImm(ptr.offset.into()),
                // Next, move the chunk containing the low bits of `x_hi1` to the top temporarily
                //
                // [chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                masm::Instruction::MovDn5,
                // Duplicate it, as we need two copies
                //
                // [chunk_mid_mid, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                masm::Instruction::Dup0,
                // Shift the value right, as done previously for the low bits of `x_hi2`
                //
                // [x_hi1_lo, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                masm::Instruction::U32ShrImm((32 - ptr.offset).into()),
                // Move the high bits of `x_hi1` to the top
                masm::Instruction::MovUp5,
                // OR the two halves together, giving us our second word, `x_hi1`
                //
                // [x_hi1, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                masm::Instruction::U32Or,
                // Move the word to the bottom of the stack
                //
                // [chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                masm::Instruction::MovDn5,
                // Now, we need to re-align the high bits of `x_lo2` by shifting
                // the remaining copy of `chunk_mid_mid`, as done previously.
                //
                // [x_lo2_hi, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                masm::Instruction::U32ShlImm(ptr.offset.into()),
                // Next, move the chunk containing the low bits of `x_lo2` to the top temporarily
                //
                // [chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                masm::Instruction::MovDn5,
                // Duplicate it, as done previously
                //
                // [chunk_mid_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                masm::Instruction::Dup0,
                // Shift the value right to get the low bits of `x_lo2`
                //
                // [x_lo2_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                masm::Instruction::U32ShrImm((32 - ptr.offset).into()),
                // Move the high bits of `x_lo2` to the top
                //
                // [x_lo2_hi, x_lo2_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                masm::Instruction::MovUp6,
                // OR the two halves together, giving us our third word, `x_lo2`
                //
                // [x_lo2, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                masm::Instruction::U32Or,
                // Move to the bottom of the stack
                //
                // [chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2]
                masm::Instruction::MovDn5,
                // Re-align the high bits of `x_lo1`
                //
                // [x_lo1_hi, chunk_lo, x_hi2, x_hi1, x_lo2]
                masm::Instruction::U32ShlImm(ptr.offset.into()),
                // Move the chunk containing the low bits to the top
                //
                // [chunk_lo, x_hi2, x_hi1, x_lo2, x_lo1_hi]
                masm::Instruction::MovDn5,
                // Shift the value right to get the low bits of `x_lo1`
                masm::Instruction::U32ShrImm((32 - ptr.offset).into()),
                // Move the high bits of `x_lo1` to the top
                //
                // [x_lo1_hi, x_lo1_lo, x_hi2, x_hi1, x_lo2]
                masm::Instruction::MovUp5,
                // OR the two halves together, giving us our fourth word, `x_lo1`
                //
                // [x_lo1, x_hi2, x_hi1, x_lo2]
                masm::Instruction::U32Or,
                // Move to the bottom
                //
                // [x_hi2, x_hi1, x_lo2, x_lo1]
                masm::Instruction::MovDn5,
            ],
            span,
        );
    }
}

/// Stores
impl OpEmitter<'_> {
    /// Store a value of type `value` to the address in the Miden address space
    /// which corresponds to a pointer in the IR's byte-addressable address space.
    ///
    /// The type of the pointer is given as `ptr`, and can be used for both validation and
    /// determining alignment.
    pub fn store(&mut self, span: SourceSpan) {
        let ptr = self.stack.pop().expect("operand stack is empty");
        let value = self.stack.pop().expect("operand stack is empty");
        let ptr_ty = ptr.ty();
        assert!(ptr_ty.is_pointer(), "expected store operand to be a pointer, got {ptr_ty}");
        let value_ty = value.ty();
        assert!(!value_ty.is_zst(), "cannot store a zero-sized type in memory");
        match ptr_ty {
            Type::Ptr(ref ptr_ty) => {
                assert_eq!(
                    value_ty.size_in_bits(),
                    ptr_ty.pointee().size_in_bits(),
                    "The size of the type of the value being stored ({value_ty}) is different \
                     from the type of the pointee ({})",
                    ptr_ty.pointee()
                );
                // As with loads, alignment removes the byte offset, not the need to preserve
                // other bits in a shared element. Merge sub-word stores at the known zero offset.
                if !ptr_ty.is_byte_pointer() && is_single_element_scalar(&value_ty) {
                    if value_ty.size_in_bits() < 32 {
                        // [addr, value] -> [prev, addr, value]
                        self.emit_all(
                            [
                                masm::Instruction::Dup0,
                                masm::Instruction::MemLoad,
                                masm::Instruction::U32Cast,
                            ],
                            span,
                        );
                        self.const_mask_u32(!((1u32 << value_ty.size_in_bits()) - 1), span);
                        self.emit_all(
                            [
                                masm::Instruction::MovUp2, // [value, masked_prev, addr]
                                masm::Instruction::U32Or,  // [new_value, addr]
                                masm::Instruction::Swap1,  // [addr, new_value]
                            ],
                            span,
                        );
                    }
                    self.emit(masm::Instruction::MemStore, span);
                    return;
                }
                // Convert the pointer to a native pointer representation
                self.convert_to_native_ptr(ptr_ty, span);
                self.store_native(&value_ty, span);
            }
            ty if !ty.is_pointer() => {
                panic!("invalid operand to store: expected pointer, got {ty}")
            }
            ty => unimplemented!("store support for pointers of type {ty} is not implemented"),
        }
    }

    /// Store through an already normalized `[element_addr, byte_offset, value..]` pair.
    /// Shared by ordinary stores and contiguous typed fills; does not update typed operands.
    pub(crate) fn store_native(&mut self, value_ty: &Type, span: SourceSpan) {
        match value_ty {
            Type::I128 | Type::U128 => self.store_quad_word(None, span),
            Type::I64 | Type::U64 => self.store_double_word_int(None, span),
            Type::Felt => self.store_felt(None, span),
            Type::I32 | Type::U32 | Type::Ptr(_) => self.store_word(None, span),
            ty if ty.size_in_bytes() <= 4 => self.store_small(ty, None, span),
            Type::Array(array_ty) => self.store_array(array_ty, None, span),
            Type::Struct(struct_ty) => self.store_struct(&struct_ty.get(), None, span),
            ty => {
                unimplemented!("invalid store: support for storing {ty} has not been implemented")
            }
        }
    }

    /// Store a value of type `ty` to `addr`.
    ///
    /// NOTE: The address represented by `addr` is in the IR's byte-addressable address space.
    pub fn store_imm(&mut self, addr: u32, span: SourceSpan) {
        let value = self.stack.pop().expect("operand stack is empty");
        let value_ty = value.ty();
        assert!(!value_ty.is_zst(), "cannot store a zero-sized type in memory");
        let ptr = NativePtr::from_ptr(addr);
        match value_ty {
            Type::I128 | Type::U128 => self.store_quad_word(Some(ptr), span),
            Type::I64 | Type::U64 => self.store_double_word_int(Some(ptr), span),
            Type::Felt => self.store_felt(Some(ptr), span),
            Type::I32 | Type::U32 | Type::Ptr(_) => self.store_word(Some(ptr), span),
            ref ty if ty.size_in_bytes() <= 4 => self.store_small(ty, Some(ptr), span),
            Type::Array(ref array_ty) => self.store_array(array_ty, Some(ptr), span),
            Type::Struct(ref struct_ty) => self.store_struct(&struct_ty.get(), Some(ptr), span),
            ty => {
                unimplemented!("invalid store: support for storing {ty} has not been implemented")
            }
        }
    }

    /// Write `count` copies of `value` to `dst`
    pub fn memset(&mut self, span: SourceSpan) {
        let dst = self.stack.pop().expect("operand stack is empty");
        let count = self.stack.pop().expect("operand stack is empty");
        let value = self.stack.pop().expect("operand stack is empty");
        assert_eq!(count.ty(), Type::U32, "expected count operand to be a u32");
        let ty = value.ty();
        assert!(dst.ty().is_pointer());
        assert_eq!(&ty, dst.ty().pointee().unwrap(), "expected value and pointee type to match");
        assert!(
            matches!(ty.size_in_bytes(), 1..=4)
                || matches!(ty, Type::I64 | Type::U64 | Type::I128 | Type::U128),
            "memset requires a supported value"
        );
        let value_size = u32::try_from(ty.size_in_bytes()).expect("invalid value size");
        let lanes = value.size();
        use masm::Instruction as I;

        // Keep a wide byte position, never a narrowed u32 byte pointer. With supported size <=16,
        // start + count*size < 2^37, so these field operations are exact integer arithmetic.
        let limit = if matches!(dst.ty(), Type::Ptr(ref ptr) if ptr.is_byte_pointer()) {
            if self.byte_addressing == ByteAddressing::Wide {
                self.emit_all(
                    self.byte_addressing.validate_start(Self::assertz_with_message_inst(
                        "memset destination range is out of bounds",
                        span,
                    )),
                    span,
                );
                ByteAddressing::LIMIT
            } else {
                0xffff_fffcu64
            }
        } else {
            self.emit(I::MulImm(Felt::from(4u32).into()), span);
            1u64 << 34
        };
        self.emit_all([I::Dup1, I::MulImm(Felt::from(value_size).into()), I::Dup1, I::Add], span);
        self.emit_push(Felt::new_unchecked(limit), span);
        self.emit_all(
            [
                I::Lte,
                Self::assert_with_message_inst("memset destination range is out of bounds", span),
            ],
            span,
        );

        // Raw invariant: [i, start_bytes, count, value lanes | S]. Only actual iteration starts
        // become addresses: the valid exclusive native endpoint 2^34 is never materialized.
        let body = self.build_masm_block(span, |emitter| {
            for _ in 0..lanes {
                emitter.emit(super::dup_from_offset(lanes + 2), span);
            }
            emitter.emit_all(
                [
                    super::dup_from_offset(lanes + 1),
                    super::dup_from_offset(lanes + 1),
                    I::MulImm(Felt::from(value_size).into()),
                    I::Add,
                ],
                span,
            );
            // MemSet has always kept a wide byte position, including in Narrow units.
            emitter.emit_all(ByteAddressing::Wide.native_ptr(), span);
            emitter.store_native(&ty, span);
            emitter.emit_counted_loop_next_condition(I::Dup3, span);
        });
        self.emit_counted_loop_header(I::Dup2, span);
        self.current_block.push(masm::Op::While { span, body });
        // The logical operands were consumed above. Cleanup all raw state/value lanes explicitly.
        for _ in 0..3 + lanes {
            self.emit(I::Drop, span);
        }
    }

    /// Copy `count` values of the pointee type from a source address to a destination address.
    ///
    /// The order of operands on the stack is `src`, `dst`, then `count`.
    ///
    /// The semantics of this instruction are as follows:
    ///
    /// * `count` is expressed in units of the pointee type, not bytes
    /// * the effective byte length is `count * size_of(*src)`
    /// * the byte length must fit in a `u32`; Narrow endpoints must also fit in a `u32`, while
    ///   Wide ranges may reach the exclusive native byte endpoint 2^34
    /// * `count == 0` leaves memory unchanged and performs no copy
    /// * the source and destination ranges must not overlap, the copy traps if they do and the
    ///   byte length is not zero
    /// * only pointers in the IR's byte-addressable address space are supported
    /// * values of 16 bytes, or a multiple of 16, are copied a word at a time, which traps unless
    ///   the source and destination addresses are 16-byte aligned, also when `count` is zero
    /// * other aggregate values are not supported, since they cannot be loaded or stored
    pub fn memcpy(&mut self, span: SourceSpan) {
        self.emit_copy(CopyOverlap::Forbidden, span);
    }

    /// Copy `count` values of the pointee type from a source address to a destination address,
    /// where the two ranges may overlap.
    ///
    /// The order of operands on the stack is `src`, `dst`, then `count`.
    ///
    /// The semantics of this instruction are those of [Self::memcpy], except that the source and
    /// destination ranges may overlap: the destination receives the values the source range held
    /// before the copy.
    pub fn memmove(&mut self, span: SourceSpan) {
        self.emit_copy(CopyOverlap::Allowed, span);
    }

    /// Emit the copy of `count` values of the pointee type from a source address to a destination
    /// address, treating overlapping ranges as specified by `overlap`.
    ///
    /// The order of operands on the stack is `src`, `dst`, then `count`.
    fn emit_copy(&mut self, overlap: CopyOverlap, span: SourceSpan) {
        let name = overlap.name();
        let src = self.stack.pop().expect("operand stack is empty");
        let dst = self.stack.pop().expect("operand stack is empty");
        let count = self.stack.pop().expect("operand stack is empty");
        assert_eq!(count.ty(), Type::U32, "expected count operand to be a u32");
        let ty = src.ty();
        assert_eq!(ty, dst.ty(), "expected src and dst operands to have the same type");
        let value_size = match &ty {
            Type::Ptr(ptr_ty) if ptr_ty.is_byte_pointer() => {
                u32::try_from(ptr_ty.pointee().size_in_bytes()).expect("invalid value size")
            }
            Type::Ptr(_) => {
                unimplemented!("{name} support for pointers of type {ty} is not implemented")
            }
            ty => panic!("invalid operand to {name}: expected pointer, got {ty}"),
        };

        if value_size == 1 && matches!(overlap, CopyOverlap::Allowed) {
            self.emit_raw_body(crate::lower::raw::byte_memmove_with_addressing(
                span,
                self.byte_addressing,
            ));
            return;
        }

        // Every arm below relies on the ranges lying within the address space
        self.emit_copy_range_check(overlap, value_size, span);

        // Use optimized intrinsics when available
        match value_size {
            // Byte copies (e.g. Wasm `memory.copy`) can often be performed more efficiently by
            // copying whole felt elements when the source, destination, and length are all
            // element-aligned.
            1 => {
                // Compute: use_elements = (src % 4 == 0) && (dst % 4 == 0) && (count % 4 == 0)
                //
                // Stack: [src, dst, count]
                self.emit_all(self.byte_addressing.element_copy_alignment(), span);

                // then: convert byte addresses/count to element units and copy whole elements
                let then_blk = self.build_masm_block(span, |then_emitter| {
                    // The core memcpy helpers compare exclusive element endpoints as u32.
                    // A Wide endpoint may be 2^32, so validate disjoint byte ranges here and
                    // use the existing move helper for the already disjoint ranges.
                    if then_emitter.byte_addressing == ByteAddressing::Wide
                        && overlap == CopyOverlap::Forbidden
                    {
                        then_emitter.emit_copy_overlap_check(value_size, span);
                    }
                    then_emitter.emit_all(
                        then_emitter.byte_addressing.element_copy_operands([
                            Self::assertz_with_message_inst(
                                format!(
                                    "{name} byte-copy fast path expected the source pointer to be \
                                     4-byte aligned"
                                ),
                                span,
                            ),
                            Self::assertz_with_message_inst(
                                format!(
                                    "{name} byte-copy fast path expected the destination pointer \
                                     to be 4-byte aligned"
                                ),
                                span,
                            ),
                            Self::assertz_with_message_inst(
                                format!(
                                    "{name} byte-copy fast path expected the byte count to be \
                                     divisible by 4"
                                ),
                                span,
                            ),
                        ]),
                        span,
                    );
                    then_emitter
                        .raw_exec(overlap.elements_procedure(then_emitter.byte_addressing), span);
                });

                let else_blk = self.build_masm_block(span, |else_emitter| {
                    // The arm treats the values as raw bytes, whatever the 1-byte pointee is
                    let byte_ptr_ty = Type::from(PointerType::new(Type::U8));
                    else_emitter.emit_copy_loop(overlap, byte_ptr_ty, span);
                });

                self.current_block.push(masm::Op::If {
                    span,
                    then_blk,
                    else_blk,
                });
            }
            // Word-sized values have an optimized intrinsic we can lean on
            16 => {
                if self.byte_addressing == ByteAddressing::Wide && overlap == CopyOverlap::Forbidden
                {
                    self.emit_copy_overlap_check(value_size, span);
                }
                // Convert `src` to a word-aligned element address.
                self.emit_word_aligned_element_addr_from_byte_ptr(span);
                // Convert `dst` to an element address the same way.
                self.emit(masm::Instruction::Swap1, span);
                self.emit_word_aligned_element_addr_from_byte_ptr(span);
                // Swap with `count` to get us into the correct ordering: [count, src, dst].
                self.emit(masm::Instruction::Swap2, span);
                self.raw_exec(overlap.words_procedure(self.byte_addressing), span);
            }
            // Values which can be broken up into word-sized chunks can piggy-back on the
            // intrinsic for word-sized values, but we have to compute a new `count` by
            // multiplying `count` by the number of words in each value
            size if size > 16 && size.is_multiple_of(16) => {
                let factor = size / 16;
                if self.byte_addressing == ByteAddressing::Wide && overlap == CopyOverlap::Forbidden
                {
                    self.emit_copy_overlap_check(value_size, span);
                }
                // Convert `src` to a word-aligned element address.
                self.emit_word_aligned_element_addr_from_byte_ptr(span);
                // Convert `dst` to an element address the same way.
                self.emit(masm::Instruction::Swap1, span);
                self.emit_word_aligned_element_addr_from_byte_ptr(span);
                self.emit_all(
                    [
                        // Swap with `count` to get us into the correct ordering: [count, src, dst]
                        masm::Instruction::Swap2,
                        // Compute the corrected count; the range check bounds `count * size`, so
                        // the product cannot overflow
                        masm::Instruction::U32WrappingMulImm(factor.into()),
                        // [count * (size / 16), src, dst]
                    ],
                    span,
                );
                self.raw_exec(overlap.words_procedure(self.byte_addressing), span);
            }
            // All other values are copied one at a time
            _ => self.emit_copy_loop(overlap, ty, span),
        }
    }

    /// Check the u32 byte length and the source/destination ranges in the unit's byte domain.
    /// Wide starts are checked before addition, so an invalid field value cannot wrap into range.
    ///
    /// Stack transition: `[src, dst, count] -> [src, dst, count]`
    fn emit_copy_range_check(&mut self, overlap: CopyOverlap, value_size: u32, span: SourceSpan) {
        let name = overlap.name();
        // Compute the byte length of the copy
        self.emit(masm::Instruction::Dup2, span); // [count, src, dst, count]
        if value_size != 1 {
            self.emit_all(
                [
                    // `u32widening_mul` leaves `[lo, hi]` on the stack; assert on `hi` and keep `lo`.
                    masm::Instruction::U32WideningMulImm(value_size.into()),
                    masm::Instruction::Swap1,
                    Self::assertz_with_message_inst(format!("{name} byte length overflowed"), span),
                ],
                span,
            ); // [len, src, dst, count]
        }
        for (pointer_dup, message) in [
            (masm::Instruction::Dup1, format!("{name} source range is out of bounds")),
            (masm::Instruction::Dup2, format!("{name} destination range is out of bounds")),
        ] {
            self.emit_all([pointer_dup, masm::Instruction::Dup1], span);
            self.emit_all(
                self.byte_addressing.range_end(Self::assertz_with_message_inst(message, span)),
                span,
            );
            self.emit(masm::Instruction::Drop, span);
        }
        self.emit(masm::Instruction::Drop, span); // [src, dst, count]
    }

    /// Emit a check that the source and destination ranges of a copy of `count` values of
    /// `value_size` bytes do not overlap, trapping otherwise.
    ///
    /// The ranges do not overlap if the byte length of the copy is zero. They must lie within the
    /// address space, see [Self::emit_copy_range_check].
    ///
    /// Stack transition: `[src, dst, count] -> [src, dst, count]`
    fn emit_copy_overlap_check(&mut self, value_size: u32, span: SourceSpan) {
        // Compute the byte length of the copy; the range check bounds it, so the product cannot
        // overflow, and neither can the end addresses below
        self.emit(masm::Instruction::Dup2, span); // [count, src, dst, count]
        if value_size != 1 {
            self.emit(masm::Instruction::U32WrappingMulImm(value_size.into()), span);
        }
        self.emit_all(
            [
                // [len, src, dst, count]
                // src + len <= dst
                masm::Instruction::Dup1,
                masm::Instruction::Dup1,
                self.byte_addressing.add(), // [src + len, len, src, dst, count]
                masm::Instruction::Dup3,
                self.byte_addressing.lte(), // [src + len <= dst, len, src, dst, count]
                // dst + len <= src
                masm::Instruction::Swap1,
                masm::Instruction::Dup3,
                self.byte_addressing.add(), // [dst + len, src + len <= dst, src, dst, count]
                masm::Instruction::Dup2,
                self.byte_addressing.lte(), // [dst + len <= src, src + len <= dst, src, dst, count]
                masm::Instruction::Or,
                Self::assert_with_message_inst(CopyOverlap::OVERLAP_MESSAGE, span),
                // [src, dst, count]
            ],
            span,
        );
    }

    /// Emit a loop which copies `count` values from `src` to `dst` one value at a time, treating
    /// overlapping ranges as specified by `overlap`.
    ///
    /// The ranges must lie within the address space, see [Self::emit_copy_range_check].
    ///
    /// Expects `[src, dst, count]` on the MASM operand stack, and consumes all three.
    fn emit_copy_loop(&mut self, overlap: CopyOverlap, ptr_ty: Type, span: SourceSpan) {
        let value_ty = ptr_ty.pointee().expect("expected a pointer type").clone();
        let value_size = u32::try_from(value_ty.size_in_bytes()).expect("invalid value size");

        // Create new block for loop body and switch to it temporarily
        let mut body = Vec::default();
        let mut body_emitter = OpEmitter::new(self.invoked, &mut body, self.stack)
            .with_byte_addressing(self.byte_addressing);

        // Offset the pointer below the index on top of the stack by `i` values; the ranges were
        // checked, so the addition cannot overflow
        let offset_by_index = |emitter: &mut OpEmitter<'_>| {
            emitter.emit_all(emitter.byte_addressing.offset_by_index(value_size, span), span);
        };

        // Loop body - compute the addresses of the next value to be read and written
        body_emitter.emit_all(
            [
                // [i, src, dst, end, step]
                masm::Instruction::Dup2, // [dst, i, src, dst, end, step]
                masm::Instruction::Dup1, // [i, dst, i, src, dst, end, step]
            ],
            span,
        );
        offset_by_index(&mut body_emitter); // [new_dst := dst + i * value_size, i, src, dst, end, step]
        body_emitter.emit_all(
            [
                masm::Instruction::Dup2, // [src, new_dst, i, src, dst, end, step]
                masm::Instruction::Dup2, // [i, src, new_dst, i, src, dst, end, step]
            ],
            span,
        );
        offset_by_index(&mut body_emitter); // [new_src := src + i * value_size, new_dst, i, ..]

        // Mirror the MASM loop state on the typed operand stack, so that the typed `load` and
        // `store` below can run; the five operands they leave are the ones dropped after the loop
        body_emitter.push(Type::U32);
        body_emitter.push(Type::U32);
        body_emitter.push(ptr_ty.clone());
        body_emitter.push(ptr_ty.clone());
        body_emitter.push(Type::U32);
        body_emitter.push(ptr_ty.clone());
        body_emitter.push(ptr_ty);
        body_emitter.load(value_ty, span); // [value, new_dst, i, src, dst, end, step]

        // Write to the destination
        body_emitter.swap(1, span); // [new_dst, value, i, src, dst, end, step]
        body_emitter.store(span); // [i, src, dst, end, step]

        // Move on to the next index, determine whether to continue loop
        body_emitter.emit_all(
            [
                masm::Instruction::Dup4,           // [step, i, src, dst, end, step]
                masm::Instruction::U32WrappingAdd, // [i + step, src, dst, end, step]
                masm::Instruction::Dup0,
                masm::Instruction::Dup4,
                masm::Instruction::Neq, // [i + step != end, i + step, src, dst, end, step]
            ],
            span,
        );

        // Switch back to original block and emit loop header and 'while.true' instruction
        //
        // Loop header - the direction of the copy is carried by the loop state, so that both
        // directions share the loop body: the index starts at `i`, is advanced by the wrapping
        // addition of `step`, and the loop is over once the index reaches `end`.
        //
        // In both directions `i == end` when `count` is zero.

        // Ascending: `0` up to `count - 1`
        let emit_ascending = |emitter: &mut OpEmitter<'_>| {
            // [src, dst, count]
            emitter.emit_push(1u32, span);
            emitter.emit(masm::Instruction::MovDn3, span); // [src, dst, end, step]
            emitter.emit_push(0u32, span); // [i, src, dst, end, step]
        };
        match overlap {
            // Disjoint ranges can be copied in any order
            CopyOverlap::Forbidden => {
                self.emit_copy_overlap_check(value_size, span);
                emit_ascending(self);
            }
            CopyOverlap::Allowed => {
                // [src, dst, count]
                self.emit_all(
                    [
                        masm::Instruction::Dup1,
                        masm::Instruction::Dup1,
                        self.byte_addressing.gt(), // [dst > src, src, dst, count]
                    ],
                    span,
                );
                // Descending, so that a value of an overlapping source range is not overwritten
                // before it is read: `count - 1` down to `0`, the index wraps around to
                // `u32::MAX` past `0`
                let then_blk = self.build_masm_block(span, |then_emitter| {
                    then_emitter.emit_all(
                        [
                            masm::Instruction::MovUp2,
                            masm::Instruction::U32WrappingSubImm(1.into()), // [count - 1, src, dst]
                        ],
                        span,
                    );
                    then_emitter.emit_push(u32::MAX, span);
                    then_emitter.emit(masm::Instruction::MovDn3, span); // [i, src, dst, end]
                    then_emitter.emit_push(u32::MAX, span);
                    then_emitter.emit(masm::Instruction::MovDn4, span); // [i, src, dst, end, step]
                });
                let else_blk = self.build_masm_block(span, emit_ascending);
                self.current_block.push(masm::Op::If {
                    span,
                    then_blk,
                    else_blk,
                });
            }
        }
        self.emit_all(
            [
                masm::Instruction::Dup0,
                masm::Instruction::Dup4,
                masm::Instruction::Neq, // [i != end, i, src, dst, end, step]
            ],
            span,
        );
        self.current_block.push(masm::Op::While {
            span,
            body: masm::Block::new(span, body),
        });

        // Cleanup - at end of 'while' loop, drop the 5 operands remaining on the stack
        self.dropn(5, span);
    }

    /// Store a quartet of machine words (32-bit elements) to memory
    fn store_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_quad_word_imm(imm, span);
        }
        self.raw_exec("::intrinsics::mem::store_qw", span);
    }

    fn store_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // When aligned, the elements are stored as they are, without a range check, as `store_qw`
        // does
        if ptr.is_word_aligned() {
            self.emit_all(
                [
                    // Stack: [a, b, c, d]
                    // Write to heap
                    masm::Instruction::MemStoreWLeImm(ptr.addr.into()),
                    masm::Instruction::DropW,
                ],
                span,
            );
        } else if ptr.is_element_aligned() {
            self.emit_all(
                [
                    masm::Instruction::MemStoreImm(ptr.addr.into()),
                    masm::Instruction::MemStoreImm((ptr.addr + 1).into()),
                    masm::Instruction::MemStoreImm((ptr.addr + 2).into()),
                    masm::Instruction::MemStoreImm((ptr.addr + 3).into()),
                ],
                span,
            );
        } else {
            // Delegate to `store_qw` to handle unaligned stores
            self.push_native_ptr(ptr, span);
            self.raw_exec("::intrinsics::mem::store_qw", span);
        }
    }

    /// Store a 64-bit integer in linear memory.
    ///
    /// Values are represented as two 32-bit limbs on the operand stack in LE limb order
    /// (`[lo, hi]`, lo on top).
    fn store_double_word_int(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        match ptr {
            // When storing to an immediate address, the operand stack only contains the value
            // limbs. In LE order, lo is on top, so MemStoreImm stores lo at lower addr first.
            //
            // The elements are stored as they are, without a range check, as `store_dw` does.
            Some(ptr) if ptr.is_element_aligned() => {
                // Stack: [value_lo, value_hi]
                self.emit_all(
                    [
                        masm::Instruction::MemStoreImm(ptr.addr.into()),
                        masm::Instruction::MemStoreImm((ptr.addr + 1).into()),
                    ],
                    span,
                );
            }
            // When storing to a dynamic address, or an unaligned immediate address, the operand
            // stack contains (or must contain) the native pointer pair `(element_addr, byte_offset)`
            // above the value limbs. This is derived from the 32-bit byte pointer via `divmod 4`.
            Some(ptr) => {
                // Stack: [value_lo, value_hi]
                self.push_native_ptr(ptr, span);
                // Stack: [addr, offset, value_lo, value_hi]
                self.raw_exec("::intrinsics::mem::store_dw", span);
            }
            None => {
                // Stack: [addr, offset, value_lo, value_hi]
                self.raw_exec("::intrinsics::mem::store_dw", span);
            }
        }
    }

    /// Stores a single 32-bit machine word, i.e. a single field element, not the Miden notion of a
    /// word
    ///
    /// Expects a native pointer pair `(element_addr, byte_offset)` on the stack if an immediate
    /// address is not given.
    fn store_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_word_imm(imm, span);
        }

        self.raw_exec("::intrinsics::mem::store_sw", span);
    }

    /// Stores a single 32-bit machine word to the given immediate address.
    fn store_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            // The element is stored as it is, without a range check, as `store_sw` does
            self.emit(masm::Instruction::MemStoreImm(ptr.addr.into()), span);
        } else {
            // Delegate to `store_sw` to handle unaligned stores
            self.push_native_ptr(ptr, span);
            self.raw_exec("::intrinsics::mem::store_sw", span);
        }
    }

    /// Store a field element to a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer pair `(element_addr, byte_offset)` is expected on the stack if an
    /// immediate is not given.
    fn store_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_felt_imm(imm, span);
        }

        self.raw_exec("::intrinsics::mem::store_felt", span);
    }

    fn store_felt_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.is_element_aligned(), "felt values must be naturally aligned");
        self.emit(masm::Instruction::MemStoreImm(ptr.addr.into()), span);
    }

    /// Store a sub-word value (u8, u16, etc.) to memory
    ///
    /// For sub-word stores, we need to:
    /// 1. Load the current 32-bit word at the target address
    /// 2. Mask out the bits where we'll place the new value
    /// 3. Shift the new value to the correct bit position
    /// 4. Combine with OR and store back
    ///
    /// This function expects the stack to contain: [addr, offset, value]
    /// where:
    /// - addr: The element-aligned address
    /// - offset: The byte offset within the element (0-3)
    /// - value: The value to store (already truncated to the correct size)
    ///
    /// The approach preserves other bytes in the same word while updating only
    /// the target byte(s).
    fn store_small(&mut self, ty: &Type, ptr: Option<NativePtr>, span: SourceSpan) {
        let type_size = ty.size_in_bits();
        // Whole-cell values (including small wrappers) retain the word owner's existing
        // validation and full-field behavior, for both immediate and dynamic pointers.
        if type_size == 32 {
            self.store_word(ptr, span);
            return;
        }
        if let Some(imm) = ptr {
            self.store_small_imm(ty, imm, span);
            return;
        }
        if type_size == 16 {
            self.store_16bit_dynamic(span);
            return;
        }
        let bits = u32::try_from(type_size).expect("invalid sub-word type size");
        if bits <= 8 {
            self.store_small_within_element(bits, span);
            return;
        }

        // Normalized offsets are 0..3. A window is needed only when the actual declared
        // bits cross the cell boundary; otherwise an unrelated neighbor must stay untouched.
        use masm::Instruction as I;
        self.emit(I::Dup1, span);
        self.emit_push((32 - bits) / 8, span);
        self.emit(I::U32Gt, span);
        let crossed = self.build_masm_block(span, |emitter| {
            // [addr, offset, value | S] -> [window, addr, offset, value | S]
            emitter.emit_all([I::Dup1, I::Dup1], span);
            emitter.raw_exec("::intrinsics::mem::load_sw", span);
            emitter.const_mask_u32(!((1u32 << bits) - 1), span);
            // The existing sub32 representation requires value to be already truncated.
            emitter.emit_all([I::MovUp3, I::U32Or, I::Swap2, I::Swap1], span);
            emitter.raw_exec("::intrinsics::mem::store_sw", span);
        });
        self.current_block.push(masm::Op::If {
            span,
            then_blk: crossed,
            else_blk: masm::Block::new(span, crate::lower::raw::store_small(bits, span)),
        });
    }

    /// Store a sub-word value which is fully contained in the current 32-bit element.
    ///
    /// Stack transition: `[addr, offset, value] -> []`.
    fn store_small_within_element(&mut self, type_size: u32, span: SourceSpan) {
        self.emit_raw_body(crate::lower::raw::store_small(type_size, span));
    }

    /// Store a 16-bit value to a dynamic native pointer tuple.
    ///
    /// This delegates to a dedicated intrinsic which owns the complete stack protocol for both the
    /// within-element and cross-element cases.
    ///
    /// Stack transition: `[addr, offset, value] -> []`.
    fn store_16bit_dynamic(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::mem::store_u16", span);
    }

    /// Store a sub-word value using an immediate pointer
    ///
    /// This function stores sub-word values (u8, u16, etc.) to memory at a specific immediate address.
    /// It handles both aligned and unaligned addresses by:
    /// 1. Loading the current 32-bit word at the element-aligned address
    /// 2. Masking out the bits where the new value will be placed
    /// 3. Shifting the new value to the correct bit position based on the byte offset
    /// 4. Combining with OR and storing back
    ///
    /// # Stack Effects
    /// - Before: `[value]` (where value is already truncated to the correct size)
    /// - After: []
    fn store_small_imm(&mut self, ty: &Type, imm: NativePtr, span: SourceSpan) {
        let type_size = ty.size_in_bits();
        if (type_size == 16 && !imm.is_element_aligned())
            || type_size + usize::from(imm.offset) * 8 > 32
        {
            // Share the dynamic owner for every actual crossing and the existing U16 path.
            self.push_native_ptr(imm, span);
            self.store_small(ty, None, span);
            return;
        }

        assert!(imm.alignment() as usize >= ty.min_alignment());

        // For immediate pointers, we always load from the element-aligned address
        // The offset determines which byte(s) within that element we're modifying
        self.emit(masm::Instruction::MemLoadImm(imm.addr.into()), span);
        self.emit(masm::Instruction::U32Cast, span);

        // Calculate bit offset from byte offset
        let bit_offset = imm.offset * 8;

        // Create a mask that clears the bits where we'll place the new value
        let type_size = ty.size_in_bits();
        debug_assert!(type_size < 32, "type_size must be less than 32 bits");
        let type_size_mask = (1u32 << type_size) - 1;
        let mask = !(type_size_mask << bit_offset);

        // Apply mask to the loaded value: [masked_prev, value]
        self.const_mask_u32(mask, span);

        // Get the value and shift it to the correct position
        self.emit(masm::Instruction::Swap1, span); // Move value to top: [value, masked_prev]
        if bit_offset > 0 {
            self.emit(masm::Instruction::U32ShlImm(bit_offset.into()), span);
        }

        // Combine the masked previous value with the shifted new value
        self.bor_u32(span);

        // Store the combined bits back to the element-aligned address
        self.emit(masm::Instruction::MemStoreImm(imm.addr.into()), span);
    }

    fn store_array(&mut self, _ty: &ArrayType, _ptr: Option<NativePtr>, _span: SourceSpan) {
        todo!()
    }

    fn store_struct(&mut self, _ty: &StructType, _ptr: Option<NativePtr>, _span: SourceSpan) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::Context;

    use super::*;
    use crate::masm::Op;

    #[test]
    fn mem_stream_emits_vm_instruction_and_preserves_window_shape() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..13 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.mem_stream(span);

        assert_eq!(emitter.stack_len(), 13);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::MemStream)));
    }

    use miden_processor::{ContextId, FastProcessor, StackInputs};
    use midenc_hir::{Immediate, formatter::PrettyPrint};

    fn execute_memory_program(source: &str, succeeds: bool) -> FastProcessor {
        let context = Context::default();
        let core = midenc_session::LinkLibrary::core().load(&context.session().options).unwrap();
        let intrinsics = crate::intrinsics::load(context.session()).unwrap();
        let mut assembler = miden_assembly::Assembler::default();
        assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic).unwrap();
        assembler
            .link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)
            .unwrap();
        let package = assembler.assemble_program("typed_fill", source).unwrap();
        let program = package.try_into_program().unwrap();
        let mut host = crate::plan::tests::lowering_units::host();
        host.load_library(miden_processor::HostLibrary::from(core)).unwrap();
        host.load_library(miden_processor::HostLibrary::from(intrinsics)).unwrap();
        let mut processor = FastProcessor::new(StackInputs::default());
        let mut resume = processor.get_initial_resume_context(&program).unwrap();
        let result = loop {
            match processor.step_sync(&mut host, resume) {
                Ok(Some(next)) => resume = next,
                Ok(None) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        if succeeds {
            result.unwrap_or_else(|error| panic!("{error:?}\nProgram:\n{source}"));
        } else {
            assert!(result.is_err(), "expected fill to trap");
        }
        processor
    }

    /// Execute exactly the production emitter algorithm, every raw value lane, and a suffix wider
    /// than the indexed window. On traps, public VM stepping retains the actual memory state.
    fn check_fill(
        ty: Type,
        value: Immediate,
        space: AddressSpace,
        base: u32,
        count: u32,
        initial: &[(u32, u64)],
        expected: Option<&[(u32, u64)]>,
    ) {
        let span = SourceSpan::UNKNOWN;
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::new();
        let mut instructions = Vec::new();
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
            for lane in (0..24).rev() {
                emitter.literal(1000 + lane as u32, span);
            }
            emitter.literal(value, span);
            emitter.pop();
            emitter.push(ty.clone());
            emitter.literal(count, span);
            emitter.emit_push(base, span);
            emitter.push(Type::from(PointerType::new_with_address_space(ty, space)));
            emitter.memset(span);
            assert_eq!(emitter.stack_len(), 24);
        }
        let body = masm::Block::new(span, instructions).to_pretty_string();
        let initialize = initial
            .iter()
            .map(|(addr, value)| format!("push.{value} mem_store.{addr}\n"))
            .collect::<String>();
        let suffix = (0..24)
            .map(|lane| format!("push.{} assert_eq\n", 1000 + lane))
            .collect::<String>();
        let check = expected
            .unwrap_or(&[])
            .iter()
            .map(|(addr, value)| format!("mem_load.{addr} push.{value} assert_eq\n"))
            .collect::<String>();
        let processor = execute_memory_program(
            &format!(
                "begin {initialize} {body} {suffix} {check} \
                 exec.::miden::core::sys::truncate_stack end"
            ),
            expected.is_some(),
        );
        for &(addr, value) in expected.unwrap_or(initial) {
            assert_eq!(
                processor.memory().read_element(ContextId::root(), Felt::from(addr)).unwrap(),
                Felt::new_unchecked(value),
                "unexpected cell {addr} after execution"
            );
        }
    }

    fn check_wide_memory(
        initial: &[(u32, u64)],
        expected: Option<&[(u32, u64)]>,
        emit: impl FnOnce(&mut OpEmitter<'_>, SourceSpan),
    ) {
        let span = SourceSpan::UNKNOWN;
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::new();
        let mut ops = Vec::new();
        let mut emitter = OpEmitter::new(&mut invoked, &mut ops, &mut stack)
            .with_byte_addressing(ByteAddressing::Wide);
        for lane in (0..24u32).rev() {
            emitter.literal(3000 + lane, span);
        }
        emit(&mut emitter, span);
        assert_eq!(emitter.stack_len(), 24);
        let body = masm::Block::new(span, ops).to_pretty_string();
        let initialize = initial
            .iter()
            .map(|(addr, value)| format!("push.{value} mem_store.{addr} "))
            .collect::<String>();
        let suffix = (0..24)
            .map(|lane| format!("push.{} assert_eq ", 3000 + lane))
            .collect::<String>();
        let processor = execute_memory_program(
            &format!("begin {initialize} {body} {suffix} end"),
            expected.is_some(),
        );
        for &(addr, value) in expected.unwrap_or(initial) {
            assert_eq!(
                processor.memory().read_element(ContextId::root(), Felt::from(addr)).unwrap(),
                Felt::new_unchecked(value),
                "cell {addr}"
            );
        }
    }

    fn emit_byte_pointer(emitter: &mut OpEmitter<'_>, address: u64, ty: Type, span: SourceSpan) {
        emitter.emit_push(Felt::new_unchecked(address), span);
        emitter.push(Type::from(PointerType::new(ty)));
    }

    #[test]
    fn wide_byte_load_store_at_high_addresses() {
        for element in [1u32 << 30, (1 << 31) + 16, u32::MAX] {
            for offset in 0..4 {
                let initial = [(element, 0x1234_5678)];
                let expected =
                    [(element, (0x1234_5678 & !(0xff << (8 * offset))) | (0xab << (8 * offset)))];
                check_wide_memory(&initial, Some(&expected), |emitter, span| {
                    let address = u64::from(element) * 4 + offset;
                    emitter.literal(0xabu8, span);
                    emit_byte_pointer(emitter, address, Type::U8, span);
                    emitter.store(span);
                    emit_byte_pointer(emitter, address, Type::U8, span);
                    emitter.load(Type::U8, span);
                    emitter.emit_push(0xabu8, span);
                    emitter.emit(masm::Instruction::AssertEq, span);
                    emitter.pop();
                });
            }
        }
        for address in [1u64 << 34, (1 << 34) + 3, super::super::P - 1] {
            check_wide_memory(&[(0, 7)], None, |emitter, span| {
                emit_byte_pointer(emitter, address, Type::U8, span);
                emitter.load(Type::U8, span);
                emitter.pop();
                emitter.emit(masm::Instruction::Drop, span);
            });
            check_wide_memory(&[(0, 7)], None, |emitter, span| {
                emitter.literal(0xabu8, span);
                emit_byte_pointer(emitter, address, Type::U8, span);
                emitter.store(span);
            });
        }
    }

    #[test]
    fn wide_byte_memset_preserves_neighbors_and_checks_domain_before_addition() {
        let element = (1u32 << 31) + 16;
        check_wide_memory(
            &[(element, 0x1234_5678)],
            Some(&[(element, 0x12ab_ab78)]),
            |emitter, span| {
                emitter.literal(0xabu8, span);
                emitter.literal(2u32, span);
                emit_byte_pointer(emitter, u64::from(element) * 4 + 1, Type::U8, span);
                emitter.memset(span);
            },
        );
        check_wide_memory(
            &[(u32::MAX, 0x1234_5678)],
            Some(&[(u32::MAX, 0xab34_5678)]),
            |emitter, span| {
                emitter.literal(0xabu8, span);
                emitter.literal(1u32, span);
                emit_byte_pointer(emitter, (1 << 34) - 1, Type::U8, span);
                emitter.memset(span);
            },
        );
        check_wide_memory(
            &[(u32::MAX, 0x1234_5678)],
            Some(&[(u32::MAX, 0x1234_5678)]),
            |emitter, span| {
                emitter.literal(0xabu8, span);
                emitter.literal(0u32, span);
                emit_byte_pointer(emitter, (1 << 34) - 1, Type::U8, span);
                emitter.memset(span);
            },
        );
        for (address, count) in [((1 << 34) - 1, 2), (1 << 34, 1), (super::super::P - 1, 2)] {
            check_wide_memory(&[(0, 7), (u32::MAX, 9)], None, |emitter, span| {
                emitter.literal(0xabu8, span);
                emitter.literal(count as u32, span);
                emit_byte_pointer(emitter, address, Type::U8, span);
                emitter.memset(span);
            });
        }
    }

    #[test]
    fn wide_byte_copies_preserve_overlap_and_cross_u32_boundary() {
        for base in [0x8000_0010u32, 0x3fff_fffc] {
            let bytes = (0..128).map(|i| (i * 17 + 11) as u8).collect::<Vec<_>>();
            let initial = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .map(|(i, chunk)| (base + i as u32, u32::from_le_bytes(*chunk) as u64))
                .collect::<Vec<_>>();
            for (ty, src, dst, count) in [
                (Type::U8, 1, 3, 11),
                (Type::U8, 3, 1, 11),
                (Type::U8, 0, 16, 8),
                (Type::U8, 12, 36, 12),
                (Type::U16, 1, 3, 5),
                (Type::U32, 0, 16, 2),
                (Type::U128, 0, 32, 1),
                (Type::from(ArrayType::new(Type::U8, 32)), 0, 64, 1),
                (Type::U8, 1, 1, 0),
                (Type::U128, 0, 0, 0),
            ] {
                let size = ty.size_in_bytes();
                for moving in [true, false] {
                    let overlap =
                        count != 0 && src < dst + count * size && dst < src + count * size;
                    let mut expected_bytes = bytes.clone();
                    expected_bytes.copy_within(src..src + count * size, dst);
                    let expected = expected_bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .enumerate()
                        .map(|(i, chunk)| (base + i as u32, u32::from_le_bytes(*chunk) as u64))
                        .collect::<Vec<_>>();
                    check_wide_memory(
                        &initial,
                        if !moving && overlap {
                            None
                        } else {
                            Some(&expected)
                        },
                        |emitter, span| {
                            emitter.literal(count as u32, span);
                            emit_byte_pointer(
                                emitter,
                                u64::from(base) * 4 + dst as u64,
                                ty.clone(),
                                span,
                            );
                            emit_byte_pointer(
                                emitter,
                                u64::from(base) * 4 + src as u64,
                                ty.clone(),
                                span,
                            );
                            if moving {
                                emitter.memmove(span)
                            } else {
                                emitter.memcpy(span)
                            }
                        },
                    );
                }
            }
        }
    }

    #[test]
    fn wide_byte_copies_enforce_native_boundary() {
        let initial = [
            (16, 0x1234_5678),
            (u32::MAX - 3, 0x89ab_cdef),
            (u32::MAX - 2, 7),
            (u32::MAX - 1, 11),
            (u32::MAX, 13),
        ];
        for moving in [false, true] {
            for (ty, src, dst, count, succeeds) in [
                (Type::U8, 64, (1 << 34) - 4, 4, true),
                (Type::U8, (1 << 34) - 4, 64, 4, true),
                (Type::U128, 65, 128, 0, false),
                (Type::U8, 1 << 34, 64, 0, false),
                (Type::U8, 64, 1 << 34, 0, false),
                (Type::U128, 64, (1 << 34) - 16, 1, true),
                (Type::U8, (1 << 34) - 1, 64, 2, false),
                (Type::U8, 64, (1 << 34) - 1, 2, false),
                (Type::U16, super::super::P - 1, 64, 1, false),
                (Type::U16, 64, super::super::P - 1, 1, false),
                (Type::U8, (1 << 34) - 1, (1 << 34) - 1, 0, true),
                (Type::U128, 64, 128, u32::MAX, false),
            ] {
                let mut expected = initial;
                if succeeds && count != 0 && dst == 64 {
                    expected[0].1 = initial[4].1;
                } else if succeeds && count != 0 {
                    let start = if ty == Type::U8 { 4 } else { 1 };
                    expected[start].1 = 0x1234_5678;
                    if ty == Type::U128 {
                        expected[2].1 = 0;
                        expected[3].1 = 0;
                        expected[4].1 = 0;
                    }
                }
                check_wide_memory(
                    &initial,
                    succeeds.then_some(expected.as_slice()),
                    |emitter, span| {
                        emitter.literal(count, span);
                        emit_byte_pointer(emitter, dst, ty.clone(), span);
                        emit_byte_pointer(emitter, src, ty, span);
                        if moving {
                            emitter.memmove(span)
                        } else {
                            emitter.memcpy(span)
                        }
                    },
                );
            }
        }
    }

    #[test]
    fn wide_byte_native_conversion_covers_native_domain() {
        for address in [0u64, 1 << 32, 1 << 33, (1 << 34) - 4] {
            for offset in 0..4 {
                let byte = address + offset;
                let span = SourceSpan::UNKNOWN;
                let mut stack = OperandStack::new(Rc::new(Context::default()));
                let mut invoked = BTreeSet::new();
                let mut ops = Vec::new();
                let mut emitter = OpEmitter::new(&mut invoked, &mut ops, &mut stack)
                    .with_byte_addressing(ByteAddressing::Wide);
                emitter.emit_push(Felt::new_unchecked(byte), span);
                emitter.convert_to_native_ptr(&PointerType::new(Type::U8), span);
                let body = masm::Block::new(span, ops).to_pretty_string();
                execute_memory_program(
                    &format!(
                        "begin {body} push.{} assert_eq push.{offset} assert_eq end",
                        byte / 4
                    ),
                    true,
                );
            }
        }
    }

    #[test]
    fn native_fill_uses_contiguous_bytes() {
        let base = 0xc000_0010; // Native address above the u32 byte-pointer domain.
        for (value, expected) in [
            (Immediate::I1(true), vec![0x0101_0101, 1]),
            (Immediate::U8(0xab), vec![0xabab_abab, 0xab]),
            (Immediate::U16(0xabcd), vec![0xabcd_abcd, 0xabcd_abcd, 0xabcd]),
            (Immediate::U32(0x1234_5678), vec![0x1234_5678; 5]),
            (
                Immediate::U64(0x0123_4567_89ab_cdef),
                vec![
                    0x89ab_cdef,
                    0x0123_4567,
                    0x89ab_cdef,
                    0x0123_4567,
                    0x89ab_cdef,
                    0x0123_4567,
                    0x89ab_cdef,
                    0x0123_4567,
                    0x89ab_cdef,
                    0x0123_4567,
                ],
            ),
            (
                Immediate::U128(0x7654_3210_fedc_ba98_0123_4567_89ab_cdef),
                [0x89ab_cdef, 0x0123_4567, 0xfedc_ba98, 0x7654_3210].repeat(5),
            ),
            (
                Immediate::Felt(Felt::new_unchecked((1u64 << 40) + 7)),
                vec![(1u64 << 40) + 7; 5],
            ),
        ] {
            let expected = expected
                .into_iter()
                .enumerate()
                .map(|(i, v)| (base + i as u32, v))
                .collect::<Vec<_>>();
            check_fill(value.ty(), value, AddressSpace::Element, base, 5, &[], Some(&expected));
        }
        let ty = Type::from(PointerType::new(Type::U8));
        check_fill(
            ty,
            Immediate::U32(0x1234_5678),
            AddressSpace::Element,
            base,
            2,
            &[],
            Some(&[(base, 0x1234_5678), (base + 1, 0x1234_5678)]),
        );
    }

    #[test]
    fn fill_range_policy_is_explicit() {
        for (space, base, value, count, succeeds) in [
            (AddressSpace::Byte, 0xffff_fff8, Immediate::U32(7), 1, true),
            (AddressSpace::Byte, 0xffff_fffb, Immediate::U8(7), 1, true),
            (AddressSpace::Byte, 0xffff_fffc, Immediate::U8(7), 0, true),
            (AddressSpace::Byte, 65, Immediate::Felt(Felt::ONE), 0, true),
            (AddressSpace::Byte, 0xffff_fffd, Immediate::U8(7), 0, false),
            (AddressSpace::Byte, 0xffff_fffc, Immediate::U8(7), 1, false),
            (AddressSpace::Byte, 0xffff_fff8, Immediate::U32(7), 2, false),
            (AddressSpace::Element, u32::MAX, Immediate::U16(0xabcd), 2, true),
            (AddressSpace::Element, u32::MAX, Immediate::U16(0xabcd), 3, false),
            (AddressSpace::Element, u32::MAX, Immediate::U128(7), 0, true),
            (AddressSpace::Element, u32::MAX, Immediate::U64(7), 1, false),
            (AddressSpace::Element, 16, Immediate::U128(7), u32::MAX, false),
        ] {
            let cell = if space == AddressSpace::Byte {
                base / 4
            } else {
                base
            };
            let initial = [(cell, 0x1234_5678)];
            let updated = if count == 0 {
                0x1234_5678
            } else if space == AddressSpace::Element {
                0xabcd_abcd
            } else if base % 4 == 3 {
                0x0734_5678
            } else {
                7
            };
            let expected = [(cell, updated)];
            check_fill(
                value.ty(),
                value,
                space,
                base,
                count,
                &initial,
                if succeeds { Some(&expected) } else { None },
            );
        }
    }

    #[test]
    fn fill_preserves_partial_cells_and_suffix() {
        let base = 0xc000_0010;
        check_fill(
            Type::U16,
            Immediate::U16(0xabcd),
            AddressSpace::Element,
            base,
            1,
            &[(base, 0x1234_5678), (base + 1, (1u64 << 40) + 7)],
            Some(&[(base, 0x1234_abcd), (base + 1, (1u64 << 40) + 7)]),
        );
        check_fill(
            Type::I1,
            Immediate::I1(true),
            AddressSpace::Byte,
            65,
            2,
            &[(16, 0xaabb_ccdd)],
            Some(&[(16, 0xaabb_cddd)]),
        );
        for (value, cells) in [
            (Immediate::U32(0x1234_5678), vec![0x1234_5678]),
            (Immediate::U64(0x0123_4567_89ab_cdef), vec![0x89ab_cdef, 0x0123_4567]),
            (
                Immediate::U128(0x7654_3210_fedc_ba98_0123_4567_89ab_cdef),
                vec![0x89ab_cdef, 0x0123_4567, 0xfedc_ba98, 0x7654_3210],
            ),
        ] {
            let initial = (0..cells.len())
                .map(|i| (base + i as u32, (1u64 << 40) + 7))
                .collect::<Vec<_>>();
            let expected = cells
                .into_iter()
                .enumerate()
                .map(|(i, v)| (base + i as u32, v))
                .collect::<Vec<_>>();
            check_fill(
                value.ty(),
                value,
                AddressSpace::Element,
                base,
                1,
                &initial,
                Some(&expected),
            );
        }
        // Partial writes use the low32 byte view even if an earlier owner stored a full Felt.
        check_fill(
            Type::U16,
            Immediate::U16(0xabcd),
            AddressSpace::Element,
            base,
            1,
            &[(base, 1u64 << 40)],
            Some(&[(base, 0xabcd)]),
        );
        // Full-cell Felt stores still require natural alignment.
        check_fill(
            Type::Felt,
            Immediate::Felt(Felt::ONE),
            AddressSpace::Byte,
            65,
            1,
            &[(16, 0x1234_5678)],
            None,
        );
        // Byte-pointer fills at all offsets reuse exactly the scalar store algorithms.
        for (value, bytes) in [
            (Immediate::U8(0xab), vec![0xab]),
            (Immediate::U16(0xabcd), 0xabcdu16.to_le_bytes().to_vec()),
            (Immediate::U32(0x1234_5678), 0x1234_5678u32.to_le_bytes().to_vec()),
            (
                Immediate::U64(0x0123_4567_89ab_cdef),
                0x0123_4567_89ab_cdefu64.to_le_bytes().to_vec(),
            ),
            (
                Immediate::U128(0x7654_3210_fedc_ba98_0123_4567_89ab_cdef),
                0x7654_3210_fedc_ba98_0123_4567_89ab_cdefu128.to_le_bytes().to_vec(),
            ),
        ] {
            for offset in 0..4 {
                for count in [0, 1, 3] {
                    let mut expected_bytes = [0x12u8; 64];
                    for i in 0..count {
                        expected_bytes[offset + i * bytes.len()..offset + (i + 1) * bytes.len()]
                            .copy_from_slice(&bytes);
                    }
                    let initial = (16..32).map(|addr| (addr, 0x1212_1212)).collect::<Vec<_>>();
                    let expected = expected_bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .enumerate()
                        .map(|(i, b)| (16 + i as u32, u32::from_le_bytes(*b) as u64))
                        .collect::<Vec<_>>();
                    check_fill(
                        value.ty(),
                        value,
                        AddressSpace::Byte,
                        64 + offset as u32,
                        count as u32,
                        &initial,
                        Some(&expected),
                    );
                }
            }
        }
    }

    #[test]
    fn local_fill_preserves_final_frame_spill_sentinel() {
        use crate::plan::{
            FinalFrameLayout, FrameRef, SlotOwner, SourceLocalLayout, SpillSlotId, ValueId,
        };
        let frame = FinalFrameLayout::new(
            SourceLocalLayout {
                offsets: vec![0],
                extent: 1,
            },
            [(
                SpillSlotId {
                    owner: SlotOwner::Value(ValueId(0)),
                    index: 0,
                },
                1,
            )]
            .into_iter()
            .collect(),
            Default::default(),
            4,
        )
        .unwrap();
        let local = frame.resolve(FrameRef::SourceLocal(0)).unwrap();
        let spill = frame
            .resolve(FrameRef::Spill(SpillSlotId {
                owner: SlotOwner::Value(ValueId(0)),
                index: 0,
            }))
            .unwrap();
        let span = SourceSpan::UNKNOWN;
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::new();
        let mut body = Vec::new();
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            emitter.literal(Immediate::U16(0xabcd), span);
            emitter.literal(2u32, span);
            emitter.emit(masm::Instruction::Locaddr(local.into()), span);
            emitter.push(Type::from(PointerType::new_with_address_space(
                Type::U16,
                AddressSpace::Element,
            )));
            emitter.memset(span);
            assert_eq!(emitter.stack_len(), 0);
        }
        let body = masm::Block::new(span, body).to_pretty_string();
        execute_memory_program(
            &format!(
                "@locals({}) proc fill push.0 loc_store.{local} push.1099511627783 \
                 loc_store.{spill} {body} loc_load.{local} push.2882382797 assert_eq \
                 loc_load.{spill} push.1099511627783 assert_eq end begin exec.fill \
                 exec.::miden::core::sys::truncate_stack end",
                frame.aligned_size()
            ),
            true,
        );
    }

    /// Exercise the shared store owner independently of MemSet and check every suffix lane.
    fn check_packed_store(
        ty: Type,
        value: impl Into<Immediate>,
        ptr: NativePtr,
        immediate: bool,
        initial: &[(u32, u64)],
        expected: Option<&[(u32, u64)]>,
    ) {
        let span = SourceSpan::UNKNOWN;
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::new();
        let mut body = Vec::new();
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            for lane in (0..24u32).rev() {
                emitter.literal(2000 + lane, span);
            }
            let value: Immediate = value.into();
            emitter.literal(value, span);
            if immediate {
                emitter.store_small(&ty, Some(ptr), span);
            } else {
                emitter.push_native_ptr(ptr, span);
                emitter.store_small(&ty, None, span);
            }
            emitter.pop();
            assert_eq!(emitter.stack_len(), 24);
        }
        let body = masm::Block::new(span, body).to_pretty_string();
        let initialize = initial
            .iter()
            .map(|(addr, value)| format!("push.{value} mem_store.{addr}\n"))
            .collect::<String>();
        let suffix = (0..24)
            .map(|lane| format!("push.{} assert_eq\n", 2000 + lane))
            .collect::<String>();
        let processor = execute_memory_program(
            &format!(
                "begin {initialize} {body} {suffix} exec.::miden::core::sys::truncate_stack end"
            ),
            expected.is_some(),
        );
        for &(addr, value) in expected.unwrap_or(initial) {
            assert_eq!(
                processor.memory().read_element(ContextId::root(), Felt::from(addr)).unwrap(),
                Felt::new_unchecked(value),
                "{ty} at {ptr:?}, immediate={immediate}, cell={addr}"
            );
        }
    }

    fn packed_types() -> [Type; 4] {
        [
            Type::from(ArrayType::new(Type::I1, 2)),
            Type::from(ArrayType::new(Type::I1, 3)),
            Type::from(ArrayType::new(Type::U8, 3)),
            Type::from(ArrayType::new(Type::I1, 4)),
        ]
    }

    fn check_packed_crossings(immediate: bool) {
        for ty in packed_types() {
            let bits = ty.size_in_bits();
            let mask = (1u32 << bits) - 1;
            for value in [mask, 0, 0x0055_aa55 & mask] {
                for offset in 0..4 {
                    let initial = if value == 0 {
                        [(16, u32::MAX as u64), (17, u32::MAX as u64)]
                    } else {
                        [(16, 0xa1b2_c3d4), (17, 0x5566_7788)]
                    };
                    let previous = (initial[1].1 << 32) | initial[0].1;
                    let shift = offset * 8;
                    let positioned_mask = (mask as u64) << shift;
                    let updated = (previous & !positioned_mask) | ((value as u64) << shift);
                    let expected = [(16, updated & 0xffff_ffff), (17, updated >> 32)];
                    let ptr = NativePtr::new(16, offset as u8);
                    check_packed_store(
                        ty.clone(),
                        value,
                        ptr,
                        immediate,
                        &initial,
                        Some(&expected),
                    );
                    let high = (1u64 << 40) + 7;
                    let neighbor = [(16, initial[0].1), (17, high)];
                    if shift + bits <= 32 {
                        check_packed_store(
                            ty.clone(),
                            value,
                            ptr,
                            immediate,
                            &neighbor,
                            Some(&[(16, expected[0].1), (17, high)]),
                        );
                        check_packed_store(
                            ty.clone(),
                            value,
                            NativePtr::new(u32::MAX, offset as u8),
                            immediate,
                            &[(u32::MAX, initial[0].1)],
                            Some(&[(u32::MAX, expected[0].1)]),
                        );
                    } else {
                        let previous = ((high as u32 as u64) << 32) | initial[0].1;
                        let updated = (previous & !positioned_mask) | ((value as u64) << shift);
                        check_packed_store(
                            ty.clone(),
                            value,
                            ptr,
                            immediate,
                            &neighbor,
                            Some(&[(16, updated & 0xffff_ffff), (17, updated >> 32)]),
                        );
                        check_packed_store(
                            ty.clone(),
                            value,
                            NativePtr::new(u32::MAX, offset as u8),
                            immediate,
                            &[(u32::MAX, initial[0].1)],
                            None,
                        );
                    }
                    let previous = (initial[1].1 << 32) | high as u32 as u64;
                    let updated = (previous & !positioned_mask) | ((value as u64) << shift);
                    check_packed_store(
                        ty.clone(),
                        value,
                        ptr,
                        immediate,
                        &[(16, high), (17, initial[1].1)],
                        Some(&[(16, updated & 0xffff_ffff), (17, updated >> 32)]),
                    );
                }
            }
        }
    }

    #[test]
    fn byte_pointer_store_initializes_reused_frame() {
        let span = SourceSpan::UNKNOWN;
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::new();
        let mut body = Vec::new();
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            emitter.literal(0xaau8, span);
            emitter.emit_push(0u8, span);
            emitter.emit(masm::Instruction::Locaddr(0u16.into()), span);
            emitter.store_small(&Type::U8, None, span);
        }
        let body = masm::Block::new(span, body).to_pretty_string();
        execute_memory_program(
            &format!(
                "@locals(4) proc dirty push.1099799081796 loc_store.0 end @locals(4) proc \
                 initialize loc_load.0 push.1099799081796 assert_eq {body} loc_load.0 \
                 push.287454122 assert_eq end begin exec.dirty exec.initialize end"
            ),
            true,
        );
    }

    #[test]
    fn byte_pointer_immediate_accesses_use_low_bytes_of_full_field_cells() {
        for (ty, value) in [(Type::U8, 0x44), (Type::U16, 0x4444)] {
            for offset in 0..4 {
                let span = SourceSpan::UNKNOWN;
                let mut stack = OperandStack::new(Rc::new(Context::default()));
                let mut invoked = BTreeSet::new();
                let mut body = Vec::new();
                {
                    let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
                    emitter.load_imm(256 + offset, ty.clone(), span);
                    emitter.emit_push(value as u32, span);
                    emitter.emit(masm::Instruction::AssertEq, span);
                    emitter.pop();
                    // Writing the same bytes isolates the partial write's treatment of high bits.
                    emitter.emit_push(value as u32, span);
                    emitter.push(ty.clone());
                    emitter.store_imm(256 + offset, span);
                }
                let body = masm::Block::new(span, body).to_pretty_string();
                let next = if offset as usize + ty.size_in_bytes() > 4 {
                    0x4444_4444u64
                } else {
                    (1u64 << 40) | 0x4444_4444
                };
                execute_memory_program(
                    &format!(
                        "begin push.1100656952388 mem_store.64 push.1100656952388 mem_store.65 \
                         {body} mem_load.64 push.1145324612 assert_eq mem_load.65 push.{next} \
                         assert_eq end"
                    ),
                    true,
                );
            }
        }
    }

    #[test]
    fn packed_dynamic_stores_preserve_crossed_bits() {
        check_packed_crossings(false);
    }

    #[test]
    fn packed_immediate_stores_preserve_crossed_bits() {
        check_packed_crossings(true);
    }

    #[test]
    fn packed_whole_word_immediates_preserve_full_field_wrapper() {
        let high = (1u64 << 40) + 7;
        let ty = Type::from(ArrayType::new(Type::Felt, 1));
        for immediate in [false, true] {
            check_packed_store(
                ty.clone(),
                Immediate::Felt(Felt::new_unchecked(high)),
                NativePtr::new(16, 0),
                immediate,
                &[(16, 11), (17, high + 1)],
                Some(&[(16, high), (17, high + 1)]),
            );
            check_packed_store(
                ty.clone(),
                Immediate::Felt(Felt::new_unchecked(high)),
                NativePtr::new(16, 1),
                immediate,
                &[(16, 11), (17, 13)],
                None,
            );
        }
        check_fill(
            ty,
            Immediate::Felt(Felt::new_unchecked(high)),
            AddressSpace::Element,
            0xc000_0010,
            2,
            &[],
            Some(&[(0xc000_0010, high), (0xc000_0011, high)]),
        );
    }

    #[test]
    fn packed_fill_range_and_terminal_footprints() {
        let ty = Type::from(ArrayType::new(Type::U8, 3));
        for count in [0, 1, 2] {
            let expected = [(u32::MAX, if count == 1 { 0x12ff_ffff } else { 0x1234_5678 })];
            check_fill(
                ty.clone(),
                Immediate::U32(0x00ff_ffff),
                AddressSpace::Element,
                u32::MAX,
                count,
                &[(u32::MAX, 0x1234_5678)],
                if count < 2 { Some(&expected) } else { None },
            );
        }
        for (base, count) in [(0xffff_fff8, 2), (64, u32::MAX)] {
            check_fill(
                ty.clone(),
                Immediate::U32(0x00ff_ffff),
                AddressSpace::Byte,
                base,
                count,
                &[(base / 4, 0x1234_5678)],
                None,
            );
        }
    }
}
