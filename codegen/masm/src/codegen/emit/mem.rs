use midenc_hir::{self as hir, Felt, FieldElement, SourceSpan, StructType, Type};

use super::OpEmitter;
use crate::masm::{NativePtr, Op};

/// Allocation
impl OpEmitter<'_> {
    /// Allocate a procedure-local memory slot of sufficient size to store a value
    /// indicated by the given pointer type, i.e the pointee type dictates the
    /// amount of memory allocated.
    ///
    /// The address of that slot is placed on the operand stack.
    pub fn alloca(&mut self, ptr: &Type, span: SourceSpan) {
        match ptr {
            Type::Ptr(pointee) => {
                let local = self.function.alloc_local(pointee.as_ref().clone());
                self.emit(Op::LocAddr(local), span);
                self.push(ptr.clone());
            }
            ty => panic!("expected a pointer type, got {ty}"),
        }
    }

    /// Return the base address of the heap
    #[allow(unused)]
    pub fn heap_base(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::mem::heap_base".parse().unwrap()), span);
        self.push(Type::Ptr(Box::new(Type::U8)));
    }

    /// Return the address of the top of the heap
    #[allow(unused)]
    pub fn heap_top(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::mem::heap_top".parse().unwrap()), span);
        self.push(Type::Ptr(Box::new(Type::U8)));
    }

    /// Grow the heap (from the perspective of Wasm programs) by N pages, returning the previous
    /// size of the heap (in pages) if successful, or -1 if the heap could not be grown.
    pub fn mem_grow(&mut self, span: SourceSpan) {
        let _num_pages = self.stack.pop().expect("operand stack is empty");
        self.emit(Op::Exec("intrinsics::mem::memory_grow".parse().unwrap()), span);
        self.push(Type::I32);
    }

    /// Returns the size (in pages) of the heap (from the perspective of Wasm programs)
    pub fn mem_size(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::mem::memory_size".parse().unwrap()), span);
        self.push(Type::U32);
    }
}

/// Loads
impl OpEmitter<'_> {
    /// Load a value corresponding to the type of the given local, from the memory allocated for
    /// that local.
    ///
    /// Internally, this pushes the address of the local on the stack, then delegates to
    /// [OpEmitter::load]
    pub fn load_local(&mut self, local: hir::LocalId, span: SourceSpan) {
        let ty = self.function.local(local).ty.clone();
        self.emit(Op::LocAddr(local), span);
        self.push(Type::Ptr(Box::new(ty.clone())));
        self.load(ty, span)
    }

    /// Load a value corresponding to the pointee type of a pointer operand on the stack.
    ///
    /// The type of the pointer determines what address space the pointer value represents;
    /// either the Miden-native address space (word-addressable), or the IR's byte-addressable
    /// address space.
    pub fn load(&mut self, ty: Type, span: SourceSpan) {
        let ptr = self.stack.pop().expect("operand stack is empty");
        match ptr.ty() {
            Type::Ptr(_) => {
                // Convert the pointer to a native pointer representation
                self.emit_native_ptr(span);
                match &ty {
                    Type::I128 => self.load_quad_word(None, span),
                    Type::I64 | Type::U64 => self.load_double_word(None, span),
                    Type::Felt => self.load_felt(None, span),
                    Type::I32 | Type::U32 => self.load_word(None, span),
                    ty @ (Type::I16 | Type::U16 | Type::U8 | Type::I8 | Type::I1) => {
                        self.load_word(None, span);
                        self.trunc_int32(ty.size_in_bits() as u32, span);
                    }
                    ty => todo!("support for loading {ty} is not yet implemented"),
                }
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
    pub fn load_imm(&mut self, addr: u32, ty: Type, span: SourceSpan) {
        let ptr = NativePtr::from_ptr(addr);
        match &ty {
            Type::I128 => self.load_quad_word(Some(ptr), span),
            Type::I64 | Type::U64 => self.load_double_word(Some(ptr), span),
            Type::Felt => self.load_felt(Some(ptr), span),
            Type::I32 | Type::U32 => self.load_word(Some(ptr), span),
            Type::I16 | Type::U16 | Type::U8 | Type::I8 | Type::I1 => {
                self.load_word(Some(ptr), span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            ty => todo!("support for loading {ty} is not yet implemented"),
        }
        self.push(ty);
    }

    /// Emit a sequence of instructions to translate a raw pointer value to
    /// a native pointer value, as a triple of `(waddr, index, offset)`, in
    /// that order on the stack.
    ///
    /// Instructions which must act on a pointer will expect the stack to have
    /// these values in that order so that they can perform any necessary
    /// re-alignment.
    fn emit_native_ptr(&mut self, span: SourceSpan) {
        self.emit_all(
            &[
                // Copy the address
                //
                // [addr, addr]
                Op::Dup(0),
                // Obtain the absolute offset
                //
                // [abs_offset, addr]
                Op::U32ModImm(16),
                // Obtain the byte offset
                //
                // [abs_offset, abs_offset, addr]
                Op::Dup(0),
                // [offset, abs_offset, addr]
                Op::U32ModImm(4),
                // Obtain the element index
                //
                // [abs_offset, offset, addr]
                Op::Swap(1),
                // [index, byte_offset, addr]
                Op::U32DivImm(4),
                // Translate the address to Miden's address space
                //
                // [addr, index, offset]
                Op::Movup(2),
                // [waddr, index, offset]
                Op::U32DivImm(16),
            ],
            span,
        );
    }

    /// Load a field element from a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer triplet is expected on the stack if an immediate is not given.
    fn load_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_felt_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::load_felt".parse().unwrap()), span);
    }

    fn load_felt_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.is_element_aligned(), "felt values must be naturally aligned");
        match ptr.index {
            0 => self.emit(Op::MemLoadImm(ptr.waddr), span),
            1 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Drop,
                        Op::Drop,
                        Op::Swap(1),
                        Op::Drop,
                    ],
                    span,
                );
            }
            2 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Drop,
                        Op::Movdn(2),
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            3 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Movdn(3),
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            _ => unreachable!(),
        }
    }

    /// Loads a single 32-bit machine word, i.e. a single field element, not the Miden notion of a
    /// word
    ///
    /// Expects a native pointer triplet on the stack if an immediate address is not given.
    fn load_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_word_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::load_sw".parse().unwrap()), span);
    }

    /// Loads a single 32-bit machine word from the given immediate address.
    fn load_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        let is_aligned = ptr.is_element_aligned();
        let rshift = 32 - ptr.offset as u32;
        match ptr.index {
            0 if is_aligned => self.emit(Op::MemLoadImm(ptr.waddr), span),
            0 => {
                self.emit_all(
                    &[
                        // Load a quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Drop,
                        Op::Drop,
                        // shift low bits
                        Op::U32ShrImm(rshift),
                        // shift high bits left by the offset
                        Op::Swap(1),
                        Op::U32ShlImm(ptr.offset as u32),
                        // OR the high and low bits together
                        Op::U32Or,
                    ],
                    span,
                );
            }
            1 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    Op::MemLoadwImm(ptr.waddr),
                    // Drop w3, w2
                    Op::Drop,
                    Op::Drop,
                    // Drop w1
                    Op::Swap(1),
                    Op::Drop,
                ],
                span,
            ),
            1 => {
                self.emit_all(
                    &[
                        // Load a quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop unused elements
                        Op::Drop,
                        Op::Movup(2),
                        Op::Drop,
                        // Shift the low bits
                        Op::U32ShrImm(rshift),
                        // Shift the high bits
                        Op::Swap(1),
                        Op::U32ShlImm(ptr.offset as u32),
                        // OR the high and low bits together
                        Op::U32Or,
                    ],
                    span,
                );
            }
            2 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    Op::MemLoadwImm(ptr.waddr),
                    // Drop w3
                    Op::Drop,
                    // Move w2 to bottom
                    Op::Movdn(2),
                    // Drop w1, w0
                    Op::Drop,
                    Op::Drop,
                ],
                span,
            ),
            2 => {
                self.emit_all(
                    &[
                        // Load a quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop unused elements
                        Op::Movup(3),
                        Op::Movup(3),
                        Op::Drop,
                        Op::Drop,
                        // Shift low bits
                        Op::U32ShrImm(rshift),
                        // Shift high bits
                        Op::U32ShlImm(ptr.offset as u32),
                        // OR the high and low bits together
                        Op::U32Or,
                    ],
                    span,
                );
            }
            3 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    Op::MemLoadwImm(ptr.waddr),
                    // Move w3 to bottom
                    Op::Movdn(3),
                    // Drop the three unused elements
                    Op::Drop,
                    Op::Drop,
                    Op::Drop,
                ],
                span,
            ),
            3 => {
                self.emit_all(
                    &[
                        // Load the quad-word containing the low bits
                        Op::MemLoadImm(ptr.waddr + 1),
                        // Shift the low bits
                        Op::U32ShrImm(rshift),
                        // Load the quad-word containing the high bits
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop unused elements
                        Op::Movdn(3),
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                        // Shift the high bits
                        Op::U32ShlImm(ptr.offset as u32),
                        // OR the high and low bits together
                        Op::U32Or,
                    ],
                    span,
                );
            }
            _ => unreachable!(),
        }
    }

    /// Load a pair of machine words (32-bit elements) to the operand stack
    fn load_double_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_double_word_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::load_dw".parse().unwrap()), span);
    }

    fn load_double_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        let aligned = ptr.is_element_aligned();
        match ptr.index {
            0 if aligned => {
                self.emit_all(
                    &[
                        // Load quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Move the two elements we need to the bottom temporarily
                        Op::Movdn(4),
                        Op::Movdn(4),
                        // Drop the unused elements
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            0 => {
                // An unaligned double-word load spans three elements
                self.emit_all(
                    &[
                        // Load quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Move the unused element to the top and drop it
                        Op::Movup(4),
                        Op::Drop,
                        // Move into stack order for realign_dw
                        Op::Swap(2),
                    ],
                    span,
                );
                self.realign_double_word(ptr, span);
            }
            1 if aligned => {
                self.emit_all(
                    &[
                        // Load quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop the first word, its unused
                        Op::Drop,
                        // Move the last word up and drop it, also unused
                        Op::Movup(3),
                        Op::Drop,
                    ],
                    span,
                );
            }
            1 => {
                // An unaligned double-word load spans three elements
                self.emit_all(
                    &[
                        // Load a quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop the unused element
                        Op::Drop,
                        // Move into stack order for realign_dw
                        Op::Swap(2),
                    ],
                    span,
                );
                self.realign_double_word(ptr, span);
            }
            2 if aligned => {
                self.emit_all(
                    &[
                        // Load quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop unused words
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            2 => {
                // An unaligned double-word load spans three elements,
                // and in this case, two quad-words, because the last
                // element is across a quad-word boundary
                self.emit_all(
                    &[
                        // Load the second quad-word first
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        // Move the element we need to the bottom temporarily
                        Op::Movdn(4),
                        // Drop the three unused elements of this word
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                        // Load the first quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop the two unused elements
                        Op::Drop,
                        Op::Drop,
                        // Move into stack order for realign_dw
                        Op::Swap(2),
                    ],
                    span,
                );
                self.realign_double_word(ptr, span);
            }
            3 if aligned => {
                self.emit_all(
                    &[
                        // Load second word, drop unused elements
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        Op::Movup(4),
                        Op::Drop,
                        Op::Movup(3),
                        Op::Drop,
                        // Load first word, drop unused elements
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            3 => {
                self.emit_all(
                    &[
                        // Load second word, drop unused element
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        Op::Movup(4),
                        Op::Drop,
                        // Load first word, drop unused elements
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                        // Move into stack order for realign_dw
                        Op::Swap(2),
                    ],
                    span,
                );
                self.realign_double_word(ptr, span);
            }
            _ => unimplemented!("unaligned loads are not yet implemented: {ptr:#?}"),
        }
    }

    /// Load a quartet of machine words (32-bit elements) to the operand stack
    fn load_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_quad_word_imm(imm, span);
        }
        self.emit(Op::Exec("intrinsics::mem::load_qw".parse().unwrap()), span);
    }

    fn load_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // For all other cases, more complicated loads are required
        let aligned = ptr.is_element_aligned();
        match ptr.index {
            // Naturally-aligned
            0 if aligned => self.emit_all(
                &[
                    // Load the word
                    Op::Padw,
                    // [w3, w2, w1, w0]
                    Op::MemLoadwImm(ptr.waddr),
                    // Swap the element order to lowest-address-first
                    // [w2, w3, w1, w0]
                    Op::Swap(1),
                    // [w1, w3, w2, w0]
                    Op::Swap(2),
                    // [w3, w1, w2, w0]
                    Op::Swap(1),
                    // [w0, w1, w2, w3]
                    Op::Swap(3),
                ],
                span,
            ),
            0 => {
                // An unaligned quad-word load spans five elements
                self.emit_all(
                    &[
                        // Load first element of second quad-word
                        // [e]
                        Op::MemLoadImm(ptr.waddr + 1),
                        // Load first quad-word
                        Op::Padw,
                        // [d, c, b, a, e]
                        Op::MemLoadwImm(ptr.waddr),
                        // [a, c, b, d, e]
                        Op::Swap(3),
                        // [c, a, b, d, e]
                        Op::Swap(1),
                        // [a, b, c, d, e]
                        Op::Movdn(2),
                    ],
                    span,
                );
                self.realign_quad_word(ptr, span);
            }
            1 if aligned => {
                self.emit_all(
                    &[
                        // Load first element of second quad-word
                        // [d]
                        Op::MemLoadImm(ptr.waddr + 1),
                        // Load first quad-word
                        Op::Padw,
                        // [c, b, a, _, d]
                        Op::MemLoadwImm(ptr.waddr),
                        // [_, b, a, c, d]
                        Op::Swap(3),
                        Op::Drop,
                        // [a, b, c, d]
                        Op::Swap(1),
                    ],
                    span,
                );
            }
            1 => {
                // An unaligned double-word load spans five elements
                self.emit_all(
                    &[
                        // Load first two elements of second quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        Op::Drop,
                        // [e, d]
                        Op::Drop,
                        // Load last three elements of first quad-word
                        Op::Padw,
                        // [c, b, a, _, e, d]
                        Op::MemLoadwImm(ptr.waddr),
                        // [_, b, a, c, e, d]
                        Op::Swap(3),
                        // [b, a, c, e, d]
                        Op::Drop,
                        // [e, a, c, b, d]
                        Op::Swap(3),
                        // [d, a, c, b, e]
                        Op::Swap(4),
                        // [b, a, c, d, e]
                        Op::Swap(3),
                        // [a, b, c, d, e]
                        Op::Swap(1),
                    ],
                    span,
                );
                self.realign_quad_word(ptr, span);
            }
            2 if aligned => {
                self.emit_all(
                    &[
                        // Load first two elements of second quad-word
                        Op::Padw,
                        // [_, _, d, c]
                        Op::MemLoadwImm(ptr.waddr),
                        // Drop last two elements
                        Op::Drop,
                        // [d, c]
                        Op::Drop,
                        // Load last two elements of first quad-word
                        Op::Padw,
                        // [b, a, _, _, d, c]
                        Op::MemLoadwImm(ptr.waddr),
                        // [d, a, _, _, b, c]
                        Op::Swap(4),
                        // [a, _, _, b, c, d]
                        Op::Movdn(5),
                        // [_, _, a, b, c, d]
                        Op::Swap(2),
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            2 => {
                // An unaligned double-word load spans five elements
                self.emit_all(
                    &[
                        // Load the first three elements of the second quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        // [e, d, c]
                        Op::Drop,
                        // Load the last two elements of the first quad-word
                        Op::Padw,
                        // [b, a, _, _, e, d, c]
                        Op::MemLoadwImm(ptr.waddr),
                        // [a, _, _, b, e, d, c]
                        Op::Movdn(3),
                        // [_, _, a, b, e, d, c]
                        Op::Movdn(2),
                        // [c, _, a, b, e, d, _]
                        Op::Swap(6),
                        // [e, _, a, b, c, d, _]
                        Op::Swap(4),
                        // [_, _, a, b, c, d, e]
                        Op::Swap(6),
                        Op::Drop,
                        // [a, b, c, d, e]
                        Op::Drop,
                    ],
                    span,
                );
                self.realign_quad_word(ptr, span);
            }
            3 if aligned => {
                self.emit_all(
                    &[
                        // Load first three elements of second quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr + 1),
                        Op::Drop,
                        // Load last element of first quad-word
                        Op::Padw,
                        Op::MemLoadwImm(ptr.waddr),
                        Op::Movdn(3),
                        Op::Drop,
                        Op::Drop,
                        Op::Drop,
                    ],
                    span,
                );
            }
            3 => {
                // An unaligned quad-word load spans five elements,
                self.emit_all(
                    &[
                        // Load second quad-word
                        Op::Padw,
                        // [e, d, c, b]
                        Op::MemLoadwImm(ptr.waddr + 1),
                        // Load last element of first quad-word
                        Op::Padw,
                        // [a, _, _, _, e, d, c, b]
                        Op::MemLoadwImm(ptr.waddr),
                        // [_, _, _, a, e, d, c, b]
                        Op::Movdn(3),
                        Op::Drop,
                        Op::Drop,
                        // [a, e, d, c, b]
                        Op::Drop,
                        // [e, a, d, c, b]
                        Op::Swap(1),
                        // [b, a, d, c, e]
                        Op::Swap(4),
                        // [d, a, b, c, e]
                        Op::Swap(2),
                        // [a, b, c, d, e]
                        Op::Movdn(3),
                    ],
                    span,
                );
                self.realign_quad_word(ptr, span);
            }
            _ => unimplemented!("unaligned loads are not yet implemented: {ptr:#?}"),
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
    fn realign_double_word(&mut self, _ptr: NativePtr, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::mem::realign_dw".parse().unwrap()), span);
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
    fn realign_quad_word(&mut self, ptr: NativePtr, span: SourceSpan) {
        // The stack starts as: [chunk_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
        //
        // We will refer to the parts of our desired quad-word value
        // as four parts, `x_hi2`, `x_hi1`, `x_lo2`, and `x_lo1`, where
        // the integer suffix should appear in decreasing order on the
        // stack when we're done.
        self.emit_all(
            &[
                // Re-align the high bits by shifting out the offset
                //
                // This gives us the first half of `x_hi2`.
                //
                // [x_hi2_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk__lo]
                Op::U32ShlImm(ptr.offset as u32),
                // Move the value below the other chunks temporarily
                //
                // [chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk__lo, x_hi2_hi]
                Op::Movdn(5),
                // We must split the `chunk_mid_hi` chunk into two parts,
                // one containing the bits to be combined with `x_hi2_hi`;
                // the other to be combined with `x_hi1_hi`.
                //
                // First, we duplicate the chunk, since we need two
                // copies of it:
                //
                // [chunk_mid_hi, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2_hi]
                Op::Dup(0),
                // Then, we shift the chunk right by 32 - offset bits,
                // re-aligning the low bits of `x_hi2`, and isolating them.
                //
                // [x_hi2_lo, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2_hi]
                Op::U32ShrImm(32 - ptr.offset as u32),
                // Move the high bits of `x_hi2` back to the top
                //
                // [x_hi2_hi, x_hi2_lo, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
                Op::Movup(3),
                // OR the two parts of the `x_hi2` chunk together
                //
                // [x_hi2, chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo]
                Op::U32Or,
                // Move `x_hi2` to the bottom for later
                //
                // [chunk_mid_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                Op::Movdn(5),
                // Now, we need to re-align the high bits of `x_hi1` by shifting
                // the remaining copy of `chunk_mid_hi`, similar to what we did for `x_hi2`
                //
                // This gives us the first half of `x_hi1`
                //
                // [x_hi1_hi, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                Op::U32ShlImm(ptr.offset as u32),
                // Next, move the chunk containing the low bits of `x_hi1` to the top temporarily
                //
                // [chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                Op::Movdn(5),
                // Duplicate it, as we need two copies
                //
                // [chunk_mid_mid, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                Op::Dup(0),
                // Shift the value right, as done previously for the low bits of `x_hi2`
                //
                // [x_hi1_lo, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1_hi]
                Op::U32ShrImm(32 - ptr.offset as u32),
                // Move the high bits of `x_hi1` to the top
                Op::Movup(5),
                // OR the two halves together, giving us our second word, `x_hi1`
                //
                // [x_hi1, chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2]
                Op::U32Or,
                // Move the word to the bottom of the stack
                //
                // [chunk_mid_mid, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                Op::Movdn(5),
                // Now, we need to re-align the high bits of `x_lo2` by shifting
                // the remaining copy of `chunk_mid_mid`, as done previously.
                //
                // [x_lo2_hi, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                Op::U32ShlImm(ptr.offset as u32),
                // Next, move the chunk containing the low bits of `x_lo2` to the top temporarily
                //
                // [chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                Op::Movdn(5),
                // Duplicate it, as done previously
                //
                // [chunk_mid_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                Op::Dup(0),
                // Shift the value right to get the low bits of `x_lo2`
                //
                // [x_lo2_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2_hi]
                Op::U32ShrImm(32 - ptr.offset as u32),
                // Move the high bits of `x_lo2` to the top
                //
                // [x_lo2_hi, x_lo2_lo, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                Op::Movup(6),
                // OR the two halves together, giving us our third word, `x_lo2`
                //
                // [x_lo2, chunk_mid_lo, chunk_lo, x_hi2, x_hi1]
                Op::U32Or,
                // Move to the bottom of the stack
                //
                // [chunk_mid_lo, chunk_lo, x_hi2, x_hi1, x_lo2]
                Op::Movdn(5),
                // Re-align the high bits of `x_lo1`
                //
                // [x_lo1_hi, chunk_lo, x_hi2, x_hi1, x_lo2]
                Op::U32ShlImm(ptr.offset as u32),
                // Move the chunk containing the low bits to the top
                //
                // [chunk_lo, x_hi2, x_hi1, x_lo2, x_lo1_hi]
                Op::Movdn(5),
                // Shift the value right to get the low bits of `x_lo1`
                Op::U32ShrImm(32 - ptr.offset as u32),
                // Move the high bits of `x_lo1` to the top
                //
                // [x_lo1_hi, x_lo1_lo, x_hi2, x_hi1, x_lo2]
                Op::Movup(5),
                // OR the two halves together, giving us our fourth word, `x_lo1`
                //
                // [x_lo1, x_hi2, x_hi1, x_lo2]
                Op::U32Or,
                // Move to the bottom
                //
                // [x_hi2, x_hi1, x_lo2, x_lo1]
                Op::Movdn(5),
            ],
            span,
        );
    }
}

/// Stores
impl OpEmitter<'_> {
    /// Store a value of the type given by the specified [hir::LocalId], using the memory allocated
    /// for that local.
    ///
    /// Internally, this pushes the address of the given local on the stack, and delegates to
    /// [OpEmitter::store] to perform the actual store.
    pub fn store_local(&mut self, local: hir::LocalId, span: SourceSpan) {
        let ty = self.function.local(local).ty.clone();
        self.emit(Op::LocAddr(local), span);
        self.push(Type::Ptr(Box::new(ty)));
        self.store(span)
    }

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
            Type::Ptr(_) => {
                // Convert the pointer to a native pointer representation
                self.emit_native_ptr(span);
                match value_ty {
                    Type::I128 => self.store_quad_word(None, span),
                    Type::I64 | Type::U64 => self.store_double_word(None, span),
                    Type::Felt => self.store_felt(None, span),
                    Type::I32 | Type::U32 => self.store_word(None, span),
                    ref ty if ty.size_in_bytes() <= 4 => self.store_small(ty, None, span),
                    Type::Array(ref elem_ty, _) => self.store_array(elem_ty, None, span),
                    Type::Struct(ref struct_ty) => self.store_struct(struct_ty, None, span),
                    ty => unimplemented!(
                        "invalid store: support for storing {ty} has not been implemented"
                    ),
                }
            }
            ty if !ty.is_pointer() => {
                panic!("invalid operand to store: expected pointer, got {ty}")
            }
            ty => unimplemented!("store support for pointers of type {ty} is not implemented"),
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
            Type::I128 => self.store_quad_word(Some(ptr), span),
            Type::I64 | Type::U64 => self.store_double_word(Some(ptr), span),
            Type::Felt => self.store_felt(Some(ptr), span),
            Type::I32 | Type::U32 => self.store_word(Some(ptr), span),
            ref ty if ty.size_in_bytes() <= 4 => self.store_small(ty, Some(ptr), span),
            Type::Array(ref elem_ty, _) => self.store_array(elem_ty, Some(ptr), span),
            Type::Struct(ref struct_ty) => self.store_struct(struct_ty, Some(ptr), span),
            ty => {
                unimplemented!("invalid store: support for storing {ty} has not been implemented")
            }
        }
    }

    pub fn memset(&mut self, span: SourceSpan) {
        let dst = self.stack.pop().expect("operand stack is empty");
        let count = self.stack.pop().expect("operand stack is empty");
        let value = self.stack.pop().expect("operand stack is empty");
        assert_eq!(count.ty(), Type::U32, "expected count operand to be a u32");
        let ty = value.ty();
        assert!(dst.ty().is_pointer());
        assert_eq!(&ty, dst.ty().pointee().unwrap(), "expected value and pointee type to match");

        // Prepare to loop until `count` iterations have been performed
        let current_block = self.current_block;
        let body = self.function.create_block();
        self.emit_all(
            &[
                // [dst, count, value..]
                Op::PushU32(0),         // [i, dst, count, value..]
                Op::Dup(2),             // [count, i, dst, count, value..]
                Op::GteImm(Felt::ZERO), // [count > 0, i, dst, count, value..]
                Op::While(body),
            ],
            span,
        );

        // Loop body - compute address for next value to be written
        let value_size = value.ty().size_in_bytes();
        self.switch_to_block(body);
        self.emit_all(
            &[
                // [i, dst, count, value..]
                // Offset the pointer by the current iteration count * aligned size of value, and
                // trap if it overflows
                Op::Dup(1), // [dst, i, dst, count, value]
                Op::Dup(1), // [i, dst, i, dst, count, value]
                Op::PushU32(value_size.try_into().expect("invalid value size")), /* [value_size, i,
                             * dst, ..] */
                Op::U32OverflowingMadd, // [value_size * i + dst, i, dst, count, value]
                Op::Assertz,            // [aligned_dst, i, dst, count, value..]
            ],
            span,
        );

        // Loop body - move value to top of stack, swap with pointer
        self.push(value);
        self.push(count);
        self.push(dst.clone());
        self.push(dst.ty());
        self.push(dst.ty());
        self.dup(4, span); // [value, aligned_dst, i, dst, count, value]
        self.swap(1, span); // [aligned_dst, value, i, dst, count, value]

        // Loop body - write value to destination
        self.store(span); // [i, dst, count, value]

        // Loop body - increment iteration count, determine whether to continue loop
        self.emit_all(
            &[
                Op::U32WrappingAddImm(1),
                Op::Dup(0), // [i++, i++, dst, count, value]
                Op::Dup(3), // [count, i++, i++, dst, count, value]
                Op::U32Gte, // [i++ >= count, i++, dst, count, value]
            ],
            span,
        );

        // Cleanup - at end of 'while' loop, drop the 4 operands remaining on the stack
        self.switch_to_block(current_block);
        self.dropn(4, span);
    }

    /// Copy `count * sizeof(*ty)` from a source address to a destination address.
    ///
    /// The order of operands on the stack is `src`, `dst`, then `count`.
    ///
    /// The addresses on the stack are interpreted based on the pointer type: native pointers are
    /// in the Miden address space; non-native pointers are assumed to be in the IR's byte
    /// addressable address space, and require translation.
    ///
    /// The semantics of this instruction are as follows:
    ///
    /// * The ``
    pub fn memcpy(&mut self, span: SourceSpan) {
        let src = self.stack.pop().expect("operand stack is empty");
        let dst = self.stack.pop().expect("operand stack is empty");
        let count = self.stack.pop().expect("operand stack is empty");
        assert_eq!(count.ty(), Type::U32, "expected count operand to be a u32");
        let ty = src.ty();
        assert!(ty.is_pointer());
        assert_eq!(ty, dst.ty(), "expected src and dst operands to have the same type");
        let value_ty = ty.pointee().unwrap();
        let value_size = u32::try_from(value_ty.size_in_bytes()).expect("invalid value size");

        // Use optimized intrinsics when available
        match value_size {
            // Word-sized values have an optimized intrinsic we can lean on
            16 => {
                self.emit_all(
                    &[
                        // [src, dst, count]
                        Op::Movup(2), // [count, src, dst]
                        Op::Exec("std::mem::memcopy".parse().unwrap()),
                    ],
                    span,
                );
                return;
            }
            // Values which can be broken up into word-sized chunks can piggy-back on the
            // intrinsic for word-sized values, but we have to compute a new `count` by
            // multiplying `count` by the number of words in each value
            size if size % 16 == 0 => {
                let factor = size / 16;
                self.emit_all(
                    &[
                        // [src, dst, count]
                        Op::Movup(2), // [count, src, dst]
                        Op::U32OverflowingMulImm(factor),
                        Op::Assertz, // [count * (size / 16), src, dst]
                        Op::Exec("std::mem::memcopy".parse().unwrap()),
                    ],
                    span,
                );
                return;
            }
            // For now, all other values fallback to the default implementation
            _ => (),
        }

        // Prepare to loop until `count` iterations have been performed
        let current_block = self.current_block;
        let body = self.function.create_block();
        self.emit_all(
            &[
                // [src, dst, count]
                Op::PushU32(0),         // [i, src, dst, count]
                Op::Dup(3),             // [count, i, src, dst, count]
                Op::GteImm(Felt::ZERO), // [count > 0, i, src, dst, count]
                Op::While(body),
            ],
            span,
        );

        // Loop body - compute address for next value to be written
        self.switch_to_block(body);

        // Compute the source and destination addresses
        self.emit_all(
            &[
                // [i, src, dst, count]
                Op::Dup(2),              // [dst, i, src, dst, count]
                Op::Dup(1),              // [i, dst, i, src, dst, count]
                Op::PushU32(value_size), // [offset, i, dst, i, src, dst, count]
                Op::U32OverflowingMadd,
                Op::Assertz, // [new_dst := i * offset + dst, i, src, dst, count]
                Op::Dup(2),  // [src, new_dst, i, src, dst, count]
                Op::Dup(2),  // [i, src, new_dst, i, src, dst, count]
                Op::PushU32(value_size), // [offset, i, src, new_dst, i, src, dst, count]
                Op::U32OverflowingMadd,
                Op::Assertz, // [new_src := i * offset + src, new_dst, i, src, dst, count]
            ],
            span,
        );

        // Load the source value
        self.push(count.clone());
        self.push(dst.clone());
        self.push(src.clone());
        self.push(Type::U32);
        self.push(dst.clone());
        self.push(src.clone());
        self.load(value_ty.clone(), span); // [value, new_dst, i, src, dst, count]

        // Write to the destination
        self.swap(1, span); // [new_dst, value, i, src, dst, count]
        self.store(span); // [i, src, dst, count]

        // Increment iteration count, determine whether to continue loop
        self.emit_all(
            &[
                Op::U32WrappingAddImm(1),
                Op::Dup(0), // [i++, i++, src, dst, count]
                Op::Dup(4), // [count, i++, i++, src, dst, count]
                Op::U32Gte, // [i++ >= count, i++, src, dst, count]
            ],
            span,
        );

        // Cleanup - at end of 'while' loop, drop the 4 operands remaining on the stack
        self.switch_to_block(current_block);
        self.dropn(4, span);
    }

    /// Store a quartet of machine words (32-bit elements) to the operand stack
    fn store_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_quad_word_imm(imm, span);
        }
        self.emit(Op::Exec("intrinsics::mem::store_qw".parse().unwrap()), span);
    }

    fn store_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // For all other cases, more complicated loads are required
        let aligned = ptr.is_element_aligned();
        match ptr.index {
            // Naturally-aligned
            0 if aligned => self.emit_all(
                &[
                    // Stack: [a, b, c, d]
                    // Swap to highest-address-first order
                    // [d, b, c, a]
                    Op::Swap(3),
                    // [c, d, b, a]
                    Op::Movup(2),
                    // [d, c, b, a]
                    Op::Swap(1),
                    // Write to heap
                    Op::MemStorewImm(ptr.waddr),
                    Op::Dropw,
                ],
                span,
            ),
            _ => {
                todo!("quad-word stores currently require 32-byte alignment")
            }
        }
    }

    /// Store a pair of machine words (32-bit elements) to the operand stack
    fn store_double_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_double_word_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::store_dw".parse().unwrap()), span);
    }

    fn store_double_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // For all other cases, more complicated stores are required
        let aligned = ptr.is_element_aligned();
        match ptr.index {
            // Naturally-aligned
            0 if aligned => self.emit_all(
                &[
                    // Swap value to highest-address-first order
                    Op::Swap(1),
                    // Load existing word
                    Op::Padw,
                    // [d, c, b, a, v_lo, v_hi]
                    Op::MemLoadwImm(ptr.waddr),
                    // Replace bottom two elements with value
                    // [b, c, d, a, v_lo, v_hi]
                    Op::Swap(2),
                    // [c, d, a, v_lo, v_hi]
                    Op::Drop,
                    // [a, d, c, v_lo, v_hi]
                    Op::Swap(2),
                    // [d, c, v_lo, v_hi]
                    Op::Drop,
                    Op::MemStorewImm(ptr.waddr),
                    Op::Dropw,
                ],
                span,
            ),
            _ => {
                // TODO: Optimize double-word stores when pointer is contant
                self.emit_all(
                    &[Op::PushU8(ptr.offset), Op::PushU8(ptr.index), Op::PushU32(ptr.waddr)],
                    span,
                );
                self.emit(Op::Exec("intrinsics::mem::store_dw".parse().unwrap()), span);
            }
        }
    }

    /// Stores a single 32-bit machine word, i.e. a single field element, not the Miden notion of a
    /// word
    ///
    /// Expects a native pointer triplet on the stack if an immediate address is not given.
    fn store_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_word_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::store_sw".parse().unwrap()), span);
    }

    /// Stores a single 32-bit machine word to the given immediate address.
    fn store_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        let is_aligned = ptr.is_element_aligned();
        let rshift = 32 - ptr.offset as u32;
        match ptr.index {
            0 if is_aligned => self.emit(Op::MemStoreImm(ptr.waddr), span),
            0 => {
                let mask_hi = u32::MAX << rshift;
                let mask_lo = u32::MAX >> (ptr.offset as u32);
                self.emit_all(
                    &[
                        // Load the word
                        Op::Padw,
                        // [w3, w2, w1, w0, value]
                        Op::MemLoadwImm(ptr.waddr),
                        // [w1, w3, w2, w0, value]
                        Op::Movup(2),
                        Op::PushU32(mask_lo),
                        // [w1_masked, w3, w2, w0, value]
                        Op::U32And,
                        // [w0, w1_masked, w3, w2, value]
                        Op::Movup(3),
                        Op::PushU32(mask_hi),
                        // [w0_masked, w1_masked, w3, w2, value]
                        Op::U32And,
                        // [value, w0_masked, w1_masked, w3, w2, value]
                        Op::Dup(4),
                        // [value, w0_masked, w1_masked, w3, w2, value]
                        Op::U32ShrImm(ptr.offset as u32),
                        // [w0', w1_masked, w3, w2, value]
                        Op::U32Or,
                        // [w1_masked, w0', w3, w2, value]
                        Op::Swap(1),
                        Op::Movup(4),
                        Op::U32ShlImm(rshift),
                        // [w1', w0', w3, w2]
                        Op::U32Or,
                        Op::Movup(3),
                        // [w3, w2, w1', w0']
                        Op::Movup(3),
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            1 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    // [d, c, _, a, value]
                    Op::MemLoadwImm(ptr.waddr),
                    // [value, d, c, _, a]
                    Op::Movup(4),
                    // [_, d, c, value, a]
                    Op::Swap(3),
                    // [d, c, value, a]
                    Op::Drop,
                    // Write the word back to the cell
                    Op::MemStorewImm(ptr.waddr),
                    // Clean up the operand stack
                    Op::Dropw,
                ],
                span,
            ),
            1 => {
                let mask_hi = u32::MAX << rshift;
                let mask_lo = u32::MAX >> (ptr.offset as u32);
                self.emit_all(
                    &[
                        Op::Padw,
                        // the load is across both the second and third elements
                        // [w3, w2, w1, w0, value]
                        Op::MemLoadwImm(ptr.waddr),
                        // [w2, w3, w1, w0, value]
                        Op::Swap(1),
                        Op::PushU32(mask_lo),
                        // [w2_masked, w3, w1, w0, value]
                        Op::U32And,
                        // [w1, w2_masked, w3, w0, value]
                        Op::Movup(2),
                        Op::PushU32(mask_hi),
                        // [w1_masked, w2_masked, w3, w0, value]
                        Op::U32And,
                        // [value, w1_masked, w2_masked, w3, w0, value]
                        Op::Dup(4),
                        Op::U32ShrImm(ptr.offset as u32),
                        // [w1', w2_masked, w3, w0, value]
                        Op::U32Or,
                        // [w2_masked, w1', w3, w0, value]
                        Op::Swap(1),
                        // [value, w2_masked, w1', w3, w0]
                        Op::Movup(4),
                        Op::U32ShlImm(rshift),
                        // [w2', w1', w3, w0, value]
                        Op::U32Or,
                        // [w0, w2', w1', w3, value]
                        Op::Movup(3),
                        // [w3, w2', w1', w0, value]
                        Op::Swap(3),
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            2 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    // [d, _, b, a, value]
                    Op::MemLoadwImm(ptr.waddr),
                    // [value, d, _, b, a]
                    Op::Movup(4),
                    // [_, d, value, b, a]
                    Op::Swap(2),
                    Op::Drop,
                    // Write the word back to the cell
                    Op::MemStorewImm(ptr.waddr),
                    // Clean up the operand stack
                    Op::Dropw,
                ],
                span,
            ),
            2 => {
                let mask_hi = u32::MAX << rshift;
                let mask_lo = u32::MAX >> (ptr.offset as u32);
                self.emit_all(
                    &[
                        // the load is across both the third and fourth elements
                        Op::Padw,
                        // [w3, w2, w1, w0, value]
                        Op::MemLoadwImm(ptr.waddr),
                        Op::PushU32(mask_lo),
                        // [w3_masked, w2, w1, w0, value]
                        Op::U32And,
                        // [w2, w3_masked, w1, w0, value]
                        Op::Swap(1),
                        Op::PushU32(mask_hi),
                        // [w2_masked, w3_masked, w1, w0, value]
                        Op::U32And,
                        // [value, w2_masked, w3_masked, w1, w0, value]
                        Op::Dup(4),
                        Op::U32ShrImm(ptr.offset as u32),
                        // [w2', w3_masked, w1, w0, value]
                        Op::U32Or,
                        // [w3_masked, w2', w1, w0, value]
                        Op::Swap(1),
                        // [value, w3_masked, w2', w1, w0]
                        Op::Movup(4),
                        Op::U32ShlImm(rshift),
                        // [w3', w2', w1, w0]
                        Op::U32Or,
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            3 if is_aligned => self.emit_all(
                &[
                    // Load a quad-word
                    Op::Padw,
                    // [_, c, b, a, value]
                    Op::MemLoadwImm(ptr.waddr),
                    // [c, b, a, value]
                    Op::Drop,
                    // [value, c, b, a]
                    Op::Movup(3),
                    // Write the word back to the cell
                    Op::MemStorewImm(ptr.waddr),
                    // Clean up the operand stack
                    Op::Dropw,
                ],
                span,
            ),
            3 => {
                // This is a rather annoying edge case, as it requires us to store bits
                // across two different words. We start with the "hi" bits that go at
                // the end of the first word, and then handle the "lo" bits in a simpler
                // fashion
                let mask_hi = u32::MAX << rshift;
                let mask_lo = u32::MAX >> (ptr.offset as u32);
                self.emit_all(
                    &[
                        // the load crosses a word boundary, start with the element containing
                        // the highest-addressed bits
                        // [w0, value]
                        Op::MemLoadImm(ptr.waddr + 1),
                        Op::PushU32(mask_lo),
                        // [w0_masked, value]
                        Op::U32And,
                        // [value, w0_masked, value]
                        Op::Dup(1),
                        // [w0', value]
                        Op::U32ShlImm(rshift),
                        Op::U32Or,
                        // Store it
                        // [value]
                        Op::MemStoreImm(ptr.waddr + 1),
                        // Load the first word
                        Op::Padw,
                        // [w3, w2, w1, w0, value]
                        Op::MemLoadwImm(ptr.waddr),
                        Op::PushU32(mask_hi),
                        // [w3_masked, w2, w1, w0, value]
                        Op::U32And,
                        // [value, w3_masked, w2, w1, w0]
                        Op::Movup(4),
                        Op::U32ShrImm(ptr.offset as u32),
                        // [w3', w2, w1, w0]
                        Op::U32Or,
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            _ => unreachable!(),
        }
    }

    /// Store a field element to a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer triplet is expected on the stack if an immediate is not given.
    fn store_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_felt_imm(imm, span);
        }

        self.emit(Op::Exec("intrinsics::mem::store_felt".parse().unwrap()), span);
    }

    fn store_felt_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.is_element_aligned(), "felt values must be naturally aligned");
        match ptr.index {
            0 => self.emit(Op::MemStoreImm(ptr.waddr), span),
            1 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        // [d, c, _, a, value]
                        Op::MemLoadwImm(ptr.waddr),
                        // [value, d, c, _, a]
                        Op::Movup(4),
                        // [_, d, c, value, a]
                        Op::Swap(3),
                        // [d, c, value, a]
                        Op::Drop,
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            2 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        // [d, _, b, a, value]
                        Op::MemLoadwImm(ptr.waddr),
                        // [value, d, _, b, a]
                        Op::Movup(4),
                        // [_, d, value, b, a]
                        Op::Swap(2),
                        Op::Drop,
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            3 => {
                self.emit_all(
                    &[
                        Op::Padw,
                        // [_, c, b, a, value]
                        Op::MemLoadwImm(ptr.waddr),
                        // [c, b, a, value]
                        Op::Drop,
                        // [value, c, b, a]
                        Op::Movup(3),
                        Op::MemStorewImm(ptr.waddr),
                        Op::Dropw,
                    ],
                    span,
                );
            }
            _ => unreachable!(),
        }
    }

    fn store_small(&mut self, ty: &Type, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_small_imm(ty, imm, span);
        }

        let type_size = ty.size_in_bits();
        if type_size == 32 {
            self.store_word(ptr, span);
            return;
        }

        // Duplicate the address
        self.emit_all(&[Op::Dup(2), Op::Dup(2), Op::Dup(2)], span);

        // Load the current 32-bit value at `ptr`
        self.load_word(ptr, span);

        // Mask out the bits we're going to be writing from the loaded value
        let mask = u32::MAX << type_size;
        self.const_mask_u32(mask, span);

        // Mix in the bits we want to write: [masked, addr1, addr2, addr3, value]
        self.emit(Op::Movup(5), span);
        self.bor_u32(span);

        // Store the combined bits: [value, addr1, addr2, addr3]
        self.emit(Op::Movdn(4), span);
        self.store_word(ptr, span);
    }

    fn store_small_imm(&mut self, ty: &Type, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.alignment() as usize >= ty.min_alignment());

        let type_size = ty.size_in_bits();
        if type_size == 32 {
            self.store_word_imm(ptr, span);
            return;
        }

        // Load the current 32-bit value at `ptr`
        self.load_word_imm(ptr, span);

        // Mask out the bits we're going to be writing from the loaded value
        let mask = u32::MAX << type_size;
        self.const_mask_u32(mask, span);

        // Mix in the bits we want to write
        self.emit(Op::Movup(4), span);
        self.bor_u32(span);

        // Store the combined bits
        self.store_word_imm(ptr, span);
    }

    fn store_array(&mut self, _element_ty: &Type, _ptr: Option<NativePtr>, _span: SourceSpan) {
        todo!()
    }

    fn store_struct(&mut self, _ty: &StructType, _ptr: Option<NativePtr>, _span: SourceSpan) {
        todo!()
    }
}
