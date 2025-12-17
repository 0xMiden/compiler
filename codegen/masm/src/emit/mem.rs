use miden_core::{Felt, FieldElement};
use midenc_hir::{
    AddressSpace, ArrayType, PointerType, SourceSpan, StructType, Type,
    dialects::builtin::LocalVariable,
};

use super::{OpEmitter, masm};
use crate::lower::NativePtr;

/// Allocation
impl OpEmitter<'_> {
    /// Grow the heap (from the perspective of Wasm programs) by N pages, returning the previous
    /// size of the heap (in pages) if successful, or -1 if the heap could not be grown.
    pub fn mem_grow(&mut self, span: SourceSpan) {
        let _num_pages = self.stack.pop().expect("operand stack is empty");
        self.raw_exec("intrinsics::mem::memory_grow", span);
        self.push(Type::I32);
    }

    /// Returns the size (in pages) of the heap (from the perspective of Wasm programs)
    pub fn mem_size(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::mem::memory_size", span);
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
    pub fn load_local(&mut self, local: &LocalVariable, span: SourceSpan) {
        let local_index = local.absolute_offset();
        let ty = local.ty();
        self.emit(masm::Instruction::Locaddr((local_index as u16).into()), span);
        self.push(Type::from(PointerType::new_with_address_space(
            ty.clone(),
            AddressSpace::Element,
        )));
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
            Type::Ptr(ref ptr_ty) => {
                // Convert the pointer to a native pointer representation
                self.convert_to_native_ptr(ptr_ty, span);
                assert_eq!(
                    ty.size_in_bits(),
                    ptr_ty.pointee().size_in_bits(),
                    "The size of the type of the value being loaded ({ty}) is different from the \
                     type of the pointee ({})",
                    ptr_ty.pointee()
                );
                match &ty {
                    Type::I128 => self.load_quad_word(None, span),
                    Type::I64 | Type::U64 => self.load_double_word_int(None, span),
                    Type::Felt => self.load_felt(None, span),
                    Type::I32 | Type::U32 => self.load_word(None, span),
                    ty @ (Type::I16 | Type::U16 | Type::U8 | Type::I8 | Type::I1) => {
                        self.load_small(ty, None, span);
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
    #[allow(unused)]
    pub fn load_imm(&mut self, addr: u32, ty: Type, span: SourceSpan) {
        let ptr = NativePtr::from_ptr(addr);
        match &ty {
            Type::I128 => self.load_quad_word(Some(ptr), span),
            Type::I64 | Type::U64 => self.load_double_word_int(Some(ptr), span),
            Type::Felt => self.load_felt(Some(ptr), span),
            Type::I32 | Type::U32 => self.load_word(Some(ptr), span),
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
            self.emit_all(
                [
                    // Obtain the byte offset and element address
                    //
                    // [offset, addr]
                    masm::Instruction::U32DivModImm(4.into()),
                    // Swap fields into expected order
                    masm::Instruction::Swap1,
                ],
                span,
            );
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

    /// Load a field element from a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer triplet is expected on the stack if an immediate is not given.
    fn load_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_felt_imm(imm, span);
        }

        self.raw_exec("intrinsics::mem::load_felt", span);
    }

    fn load_felt_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        assert!(ptr.is_element_aligned(), "felt values must be naturally aligned");
        self.emit(masm::Instruction::MemLoadImm(ptr.addr.into()), span);
    }

    /// Loads a single 32-bit machine word, i.e. a single field element, not the Miden notion of a
    /// word
    ///
    /// Expects a native pointer triplet on the stack if an immediate address is not given.
    fn load_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_word_imm(imm, span);
        }

        self.raw_exec("intrinsics::mem::load_sw", span);
    }

    /// Loads a single 32-bit machine word from the given immediate address.
    fn load_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            self.emit_all(
                [masm::Instruction::MemLoadImm(ptr.addr.into()), masm::Instruction::U32Assert],
                span,
            );
        } else {
            // Delegate to load_sw intrinsic to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("intrinsics::mem::load_sw", span);
        }
    }

    /// Load a 64-bit word from the given address.
    fn load_double_word_int(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            self.load_double_word_imm(imm, span);
        } else {
            self.raw_exec("intrinsics::mem::load_dw", span);
        }

        // The mem::intrinsic loads two 32-bit words with the first at the top of the stack.  Swap
        // them to make a big-endian-limbed stack value.
        self.emit(masm::Instruction::Swap1, span);
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
    /// After execution, the stack will contain: [loaded_value]
    fn load_small(&mut self, ty: &Type, ptr: Option<NativePtr>, span: SourceSpan) {
        // If we have an immediate pointer, handle it based on alignment
        if let Some(imm) = ptr {
            if imm.is_element_aligned() {
                // For aligned loads, we can use MemLoadImm
                self.emit(masm::Instruction::MemLoadImm(imm.addr.into()), span);
                // The value is already loaded, just need to mask it
                let mask = match ty.size_in_bits() {
                    1 => 0x1,
                    8 => 0xff,
                    16 => 0xffff,
                    _ => unreachable!("load_small called with non-small type"),
                };
                self.emit_push(mask as u32, span);
                self.emit(masm::Instruction::U32And, span);
                return;
            } else {
                self.emit_push(imm.addr, span);
                self.emit_push(imm.offset, span);
            }
        }

        // Stack: [element_addr, byte_offset]

        // First, load the aligned word containing our value
        // We need to temporarily move the offset out of the way
        self.emit(masm::Instruction::Swap1, span); // [byte_offset, element_addr]
        self.emit(masm::Instruction::Dup1, span); // [element_addr, byte_offset, element_addr]
        self.emit(masm::Instruction::MemLoad, span); // [loaded_word, byte_offset, element_addr]

        // Now we need to extract the correct byte(s) based on the offset
        // Stack: [loaded_word, byte_offset, element_addr]
        self.emit(masm::Instruction::Swap1, span); // [byte_offset, loaded_word, element_addr]

        // Shift right by (offset * 8) bits to get our byte at the low end
        self.emit_push(8u8, span); // [8, byte_offset, loaded_word, element_addr]
        self.emit_all(
            [
                masm::Instruction::U32WrappingMul, // [offset*8, loaded_word, element_addr]
                masm::Instruction::U32Shr,         // [shifted_word, element_addr]
            ],
            span,
        );

        // Clean up the element address
        self.emit(masm::Instruction::Swap1, span); // [element_addr, shifted_word]
        self.emit(masm::Instruction::Drop, span); // [shifted_word]

        // Mask to get only the bits we need
        let mask = match ty.size_in_bits() {
            1 => 0x1,
            8 => 0xff,
            16 => 0xffff,
            _ => unreachable!("load_small called with non-small type"),
        };

        self.emit_push(mask as u32, span);
        self.emit(masm::Instruction::U32And, span);
    }

    fn load_double_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            self.emit_all(
                [
                    masm::Instruction::MemLoadImm((ptr.addr + 1).into()),
                    masm::Instruction::MemLoadImm(ptr.addr.into()),
                    masm::Instruction::U32Assert2,
                ],
                span,
            )
        } else {
            // Delegate to load_dw to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("intrinsics::mem::load_dw", span);
        }
    }

    /// Load a quartet of machine words (32-bit elements) to the operand stack
    fn load_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.load_quad_word_imm(imm, span);
        }
        self.raw_exec("intrinsics::mem::load_qw", span);
    }

    fn load_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        // For all other cases, more complicated loads are required
        if ptr.is_word_aligned() {
            self.emit_all(
                [
                    // [w0, w1, w2, w3]
                    masm::Instruction::PadW,
                    masm::Instruction::MemLoadWLeImm(ptr.addr.into()),
                    masm::Instruction::U32AssertW,
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
                    masm::Instruction::U32AssertW,
                ],
                span,
            );
        } else {
            // Delegate to load_qw to handle the details of unaligned loads
            self.push_native_ptr(ptr, span);
            self.raw_exec("intrinsics::mem::load_qw", span);
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
        self.raw_exec("intrinsics::mem::realign_dw", span);
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
    /// Store a value of the type given by the specified [hir::LocalId], using the memory allocated
    /// for that local.
    ///
    /// Internally, this pushes the address of the given local on the stack, and delegates to
    /// [OpEmitter::store] to perform the actual store.
    pub fn store_local(&mut self, local: &LocalVariable, span: SourceSpan) {
        let local_index = local.absolute_offset();
        self.emit(masm::Instruction::Locaddr((local_index as u16).into()), span);
        self.push(Type::from(PointerType::new_with_address_space(
            local.ty(),
            AddressSpace::Element,
        )));
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
            Type::Ptr(ref ptr_ty) => {
                // Convert the pointer to a native pointer representation
                self.convert_to_native_ptr(ptr_ty, span);
                assert_eq!(
                    value_ty.size_in_bits(),
                    ptr_ty.pointee().size_in_bits(),
                    "The size of the type of the value being stored ({value_ty}) is different \
                     from the type of the pointee ({})",
                    ptr_ty.pointee()
                );
                match value_ty {
                    Type::I128 => self.store_quad_word(None, span),
                    Type::I64 | Type::U64 => self.store_double_word_int(None, span),
                    Type::Felt => self.store_felt(None, span),
                    Type::I32 | Type::U32 => self.store_word(None, span),
                    ref ty if ty.size_in_bytes() <= 4 => self.store_small(ty, None, span),
                    Type::Array(ref array_ty) => self.store_array(array_ty, None, span),
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
            Type::I64 | Type::U64 => self.store_double_word_int(Some(ptr), span),
            Type::Felt => self.store_felt(Some(ptr), span),
            Type::I32 | Type::U32 => self.store_word(Some(ptr), span),
            ref ty if ty.size_in_bytes() <= 4 => self.store_small(ty, Some(ptr), span),
            Type::Array(ref array_ty) => self.store_array(array_ty, Some(ptr), span),
            Type::Struct(ref struct_ty) => self.store_struct(struct_ty, Some(ptr), span),
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
        let value_size = u32::try_from(ty.size_in_bytes()).expect("invalid value size");

        // Create new block for loop body and switch to it temporarily
        let mut body = Vec::default();
        let mut body_emitter = OpEmitter::new(self.invoked, &mut body, self.stack);

        // Loop body - compute address for next value to be written
        body_emitter.emit_all(
            [
                // [i, dst, count, value..]
                // Offset the pointer by the current iteration count * aligned size of value, and
                // trap if it overflows
                masm::Instruction::Dup1, // [dst, i, dst, count, value]
                masm::Instruction::Dup1, // [i, dst, i, dst, count, value]
            ],
            span,
        );
        body_emitter.emit_push(value_size, span); // [value_size, i, * dst, ..]
        body_emitter.emit_all(
            [
                masm::Instruction::U32OverflowingMadd, // [value_size * i + dst, i, dst, count, value]
                masm::Instruction::Assertz,            // [aligned_dst, i, dst, count, value..]
            ],
            span,
        );

        // Loop body - move value to top of stack, swap with pointer
        body_emitter.push(value);
        body_emitter.push(count);
        body_emitter.push(dst.clone());
        body_emitter.push(dst.ty());
        body_emitter.push(dst.ty());
        body_emitter.dup(4, span); // [value, aligned_dst, i, dst, count, value]
        body_emitter.swap(1, span); // [aligned_dst, value, i, dst, count, value]

        // Loop body - write value to destination
        body_emitter.store(span); // [i, dst, count, value]

        // Loop body - increment iteration count, determine whether to continue loop
        body_emitter.emit_all(
            [
                masm::Instruction::U32WrappingAddImm(1.into()),
                masm::Instruction::Dup0,   // [i++, i++, dst, count, value]
                masm::Instruction::Dup3,   // [count, i++, i++, dst, count, value]
                masm::Instruction::U32Gte, // [i++ >= count, i++, dst, count, value]
            ],
            span,
        );

        // Switch back to original block and emit loop header and 'while.true' instruction
        //
        // Loop header - prepare to loop until `count` iterations have been performed
        // [dst, count, value..]
        self.emit_push(0u32, span); // [i, dst, count, value..]
        self.emit(masm::Instruction::Dup2, span); // [count, i, dst, count, value..]
        self.emit_push(Felt::ZERO, span);
        self.emit(
            masm::Instruction::Gte, // [count > 0, i, dst, count, value..]
            span,
        );
        self.current_block.push(masm::Op::While {
            span,
            body: masm::Block::new(span, body),
        });

        // Cleanup - at end of 'while' loop, drop the 4 operands remaining on the stack
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
                // We have to convert byte addresses to element addresses
                self.emit_all(
                    [
                        // Convert `src` to element address, and assert aligned to an element address
                        //
                        // TODO: We should probably also assert that the address is word-aligned, but
                        // that is going to happen anyway. That said, the closer to the source the
                        // better for debugging.
                        masm::Instruction::U32DivModImm(4.into()),
                        masm::Instruction::Assertz,
                        // Convert `dst` to an element address the same way
                        masm::Instruction::Swap1,
                        masm::Instruction::U32DivModImm(4.into()),
                        masm::Instruction::Assertz,
                        // Swap with `count` to get us into the correct ordering: [count, src, dst]
                        masm::Instruction::Swap2,
                    ],
                    span,
                );
                self.raw_exec("std::mem::memcopy_words", span);
                return;
            }
            // Values which can be broken up into word-sized chunks can piggy-back on the
            // intrinsic for word-sized values, but we have to compute a new `count` by
            // multiplying `count` by the number of words in each value
            size if size > 16 && size.is_multiple_of(16) => {
                let factor = size / 16;
                self.emit_all(
                    [
                        // Convert `src` to element address, and assert aligned to an element address
                        //
                        // TODO: We should probably also assert that the address is word-aligned, but
                        // that is going to happen anyway. That said, the closer to the source the
                        // better for debugging.
                        masm::Instruction::U32DivModImm(4.into()),
                        masm::Instruction::Assertz,
                        // Convert `dst` to an element address the same way
                        masm::Instruction::Swap1,
                        masm::Instruction::U32DivModImm(4.into()),
                        masm::Instruction::Assertz,
                        // Swap with `count` to get us into the correct ordering: [count, src, dst]
                        masm::Instruction::Swap2,
                        // Compute the corrected count
                        masm::Instruction::U32OverflowingMulImm(factor.into()),
                        masm::Instruction::Assertz, // [count * (size / 16), src, dst]
                    ],
                    span,
                );
                self.raw_exec("std::mem::memcopy_words", span);
                return;
            }
            // For now, all other values fallback to the default implementation
            _ => (),
        }

        // Create new block for loop body and switch to it temporarily
        let mut body = Vec::default();
        let mut body_emitter = OpEmitter::new(self.invoked, &mut body, self.stack);

        // Loop body - compute address for next value to be written
        // Compute the source and destination addresses
        body_emitter.emit_all(
            [
                // [i, src, dst, count]
                masm::Instruction::Dup2, // [dst, i, src, dst, count]
                masm::Instruction::Dup1, // [i, dst, i, src, dst, count]
            ],
            span,
        );
        body_emitter.emit_push(value_size, span); // [offset, i, dst, i, src, dst, count]
        body_emitter.emit_all(
            [
                masm::Instruction::U32OverflowingMadd,
                masm::Instruction::Assertz, // [new_dst := i * offset + dst, i, src, dst, count]
                masm::Instruction::Dup2,    // [src, new_dst, i, src, dst, count]
                masm::Instruction::Dup2,    // [i, src, new_dst, i, src, dst, count]
            ],
            span,
        );
        body_emitter.emit_push(value_size, span); // [offset, i, src, new_dst, i, src, dst, count]
        body_emitter.emit_all(
            [
                masm::Instruction::U32OverflowingMadd,
                masm::Instruction::Assertz, // [new_src := i * offset + src, new_dst, i, src, dst, count]
            ],
            span,
        );

        // Load the source value
        body_emitter.push(count.clone());
        body_emitter.push(dst.clone());
        body_emitter.push(src.clone());
        body_emitter.push(Type::U32);
        body_emitter.push(dst.clone());
        body_emitter.push(src.clone());
        body_emitter.load(value_ty.clone(), span); // [value, new_dst, i, src, dst, count]

        // Write to the destination
        body_emitter.swap(1, span); // [new_dst, value, i, src, dst, count]
        body_emitter.store(span); // [i, src, dst, count]

        // Increment iteration count, determine whether to continue loop
        body_emitter.emit_all(
            [
                masm::Instruction::U32WrappingAddImm(1.into()),
                masm::Instruction::Dup0,   // [i++, i++, src, dst, count]
                masm::Instruction::Dup4,   // [count, i++, i++, src, dst, count]
                masm::Instruction::U32Gte, // [i++ >= count, i++, src, dst, count]
            ],
            span,
        );

        // Switch back to original block and emit loop header and 'while.true' instruction
        //
        // Loop header - prepare to loop until `count` iterations have been performed

        // [src, dst, count]
        self.emit_push(0u32, span); // [i, src, dst, count]
        self.emit(masm::Instruction::Dup3, span); // [count, i, src, dst, count]
        self.emit_push(Felt::ZERO, span);
        self.emit(
            masm::Instruction::Gte, // [count > 0, i, src, dst, count]
            span,
        );
        self.current_block.push(masm::Op::While {
            span,
            body: masm::Block::new(span, body),
        });

        // Cleanup - at end of 'while' loop, drop the 4 operands remaining on the stack
        self.dropn(4, span);
    }

    /// Store a quartet of machine words (32-bit elements) to the operand stack
    fn store_quad_word(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_quad_word_imm(imm, span);
        }
        self.raw_exec("intrinsics::mem::store_qw", span);
    }

    fn store_quad_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_word_aligned() {
            self.emit_all(
                [
                    // Stack: [a, b, c, d]
                    masm::Instruction::U32AssertW,
                    // Write to heap
                    masm::Instruction::MemStoreWLeImm(ptr.addr.into()),
                    masm::Instruction::DropW,
                ],
                span,
            );
        } else if ptr.is_element_aligned() {
            self.emit_all(
                [
                    masm::Instruction::U32AssertW,
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
            self.raw_exec("intrinsics::mem::store_qw", span);
        }
    }

    /// Store a 64-bit word to the operand stack
    fn store_double_word_int(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        // The mem::intrinsic stores two 32-bit words in stack order.  Swap them (the 3rd and 4th
        // params) first to make a little-endian-limbed memory value.
        self.emit(masm::Instruction::MovUp2, span);
        self.emit(masm::Instruction::MovDn3, span);

        if let Some(imm) = ptr {
            self.store_double_word_imm(imm, span);
        } else {
            self.raw_exec("intrinsics::mem::store_dw", span);
        }
    }

    fn store_double_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            self.emit_all(
                [
                    masm::Instruction::U32Assert2,
                    masm::Instruction::MemStoreImm(ptr.addr.into()),
                    masm::Instruction::MemStoreImm((ptr.addr + 1).into()),
                ],
                span,
            );
        } else {
            // Delegate to `store_dw` to handle unaligned stores
            self.push_native_ptr(ptr, span);
            self.raw_exec("intrinsics::mem::store_dw", span);
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

        self.raw_exec("intrinsics::mem::store_sw", span);
    }

    /// Stores a single 32-bit machine word to the given immediate address.
    fn store_word_imm(&mut self, ptr: NativePtr, span: SourceSpan) {
        if ptr.is_element_aligned() {
            self.emit_all(
                [masm::Instruction::U32Assert, masm::Instruction::MemStoreImm(ptr.addr.into())],
                span,
            );
        } else {
            // Delegate to `store_sw` to handle unaligned stores
            self.push_native_ptr(ptr, span);
            self.raw_exec("intrinsics::mem::store_sw", span);
        }
    }

    /// Store a field element to a naturally aligned address, either immediate or dynamic
    ///
    /// A native pointer triplet is expected on the stack if an immediate is not given.
    fn store_felt(&mut self, ptr: Option<NativePtr>, span: SourceSpan) {
        if let Some(imm) = ptr {
            return self.store_felt_imm(imm, span);
        }

        self.raw_exec("intrinsics::mem::store_felt", span);
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
        if let Some(imm) = ptr {
            self.store_small_imm(ty, imm, span);
            return;
        }

        let type_size = ty.size_in_bits();
        if type_size == 32 {
            self.store_word(ptr, span);
            return;
        }

        // Stack: [addr, offset, value]
        // Load the current aligned value
        self.emit_all(
            [
                masm::Instruction::Dup0,    // [addr, addr, offset, value]
                masm::Instruction::MemLoad, // [prev, addr, offset, value]
            ],
            span,
        );

        // Calculate bit offset and create mask
        let type_size_mask = (1u32 << type_size) - 1;
        self.emit(masm::Instruction::Dup2, span); // [offset, prev, addr, offset, value]
        self.emit_push(8u8, span); // [8, offset, prev, addr, offset, value]
        self.emit(masm::Instruction::U32WrappingMul, span); // [bit_offset, prev, addr, offset, value]
        self.emit_push(type_size_mask, span); // [type_mask, bit_offset, prev, addr, offset, value]
        self.emit_all(
            [
                masm::Instruction::Swap1, // [bit_offset, type_mask, prev, addr, offset, value]
                masm::Instruction::U32Shl, // [shifted_mask, prev, addr, offset, value]
                masm::Instruction::U32Not, // [mask, prev, addr, offset, value]
                masm::Instruction::Swap1, // [prev, mask, addr, offset, value]
                masm::Instruction::U32And, // [masked_prev, addr, offset, value]
            ],
            span,
        );

        // Shift value to correct position and combine
        self.emit_all(
            [
                masm::Instruction::MovUp3, // [value, masked_prev, addr, offset]
                masm::Instruction::MovUp3, // [offset, value, masked_prev, addr]
            ],
            span,
        );
        self.emit_push(8u8, span); // [8, offset, value, masked_prev, addr]
        self.emit_all(
            [
                masm::Instruction::U32WrappingMul, // [bit_offset, value, masked_prev, addr]
                masm::Instruction::U32Shl,         // [shifted_value, masked_prev, addr]
                masm::Instruction::U32Or,          // [new_value, addr]
                masm::Instruction::Swap1,          // [addr, new_value]
                masm::Instruction::MemStore,       // []
            ],
            span,
        );
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
    /// - Before: [value] (where value is already truncated to the correct size)
    /// - After: []
    fn store_small_imm(&mut self, ty: &Type, imm: NativePtr, span: SourceSpan) {
        assert!(imm.alignment() as usize >= ty.min_alignment());

        // For immediate pointers, we always load from the element-aligned address
        // The offset determines which byte(s) within that element we're modifying
        self.emit(masm::Instruction::MemLoadImm(imm.addr.into()), span);

        // Calculate bit offset from byte offset
        let bit_offset = imm.offset * 8;

        // Create a mask that clears the bits where we'll place the new value
        let type_size = ty.size_in_bits();
        debug_assert!(type_size < 32, "type_size must be less than 32 bits");
        let type_size_mask = (1u32 << type_size) - 1;
        let mask = !(type_size_mask << bit_offset);

        // Apply mask to the loaded value
        self.const_mask_u32(mask, span);

        // Get the value and shift it to the correct position
        self.emit(masm::Instruction::MovUp4, span); // Move value to top
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
