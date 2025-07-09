use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
};

use miden_assembly::Library as CompiledLibrary;
use miden_core::{FieldElement, Program, StackInputs, Word};
use miden_processor::{
    AdviceInputs, AdviceProvider, ContextId, ExecutionError, Felt, MastForest, Process,
    ProcessState, RowIndex, StackOutputs, TraceLenSummary, VmState, VmStateIterator,
};
use midenc_codegen_masm::NativePtr;
pub use midenc_codegen_masm::TraceEvent;
use midenc_hir::{SmallVec, ToSmallVec, Type};
use midenc_session::Session;

use super::MemoryChiplet;
use crate::{debug::CallStack, DebuggerHost, FromMidenRepr, TestFelt};

/// A callback to be executed when a [TraceEvent] occurs at a given clock cycle
pub type TraceHandler = dyn FnMut(RowIndex, TraceEvent);

/// Occurs when an attempt to read memory of the VM fails
#[derive(Debug, thiserror::Error)]
pub enum MemoryReadError {
    #[error("attempted to read beyond end of linear memory")]
    OutOfBounds,
    #[error("unaligned reads are not supported yet")]
    UnalignedRead,
}

/// An [ExecutionTrace] represents a final state of a program that was executed.
///
/// It can be used to examine the program results, and the memory of the program at
/// any cycle up to the last cycle. It is typically used for those purposes once
/// execution of a program terminates.
pub struct ExecutionTrace {
    pub(super) root_context: ContextId,
    pub(super) last_cycle: RowIndex,
    pub(super) memory: MemoryChiplet,
    pub(super) outputs: StackOutputs,
    pub(super) trace_len_summary: TraceLenSummary,
}

impl ExecutionTrace {
    /// Parse the program outputs on the operand stack as a value of type `T`
    pub fn parse_result<T>(&self) -> Option<T>
    where
        T: FromMidenRepr,
    {
        let size = <T as FromMidenRepr>::size_in_felts();
        let stack = self.outputs.stack_truncated(size);
        if stack.len() < size {
            return None;
        }
        dbg!(stack);
        let mut stack = stack.to_vec();
        stack.reverse();
        dbg!(&stack);
        Some(<T as FromMidenRepr>::pop_from_stack(&mut stack))
    }

    /// Consume the [ExecutionTrace], extracting just the outputs on the operand stack
    #[inline]
    pub fn into_outputs(self) -> StackOutputs {
        self.outputs
    }

    /// Return a reference to the operand stack outputs
    #[inline]
    pub fn outputs(&self) -> &StackOutputs {
        &self.outputs
    }

    /// Return a reference to the trace length summary
    #[inline]
    pub fn trace_len_summary(&self) -> &TraceLenSummary {
        &self.trace_len_summary
    }

    /// Read the word at the given Miden memory address
    pub fn read_memory_word(&self, addr: u32) -> Option<Word> {
        self.read_memory_word_in_context(addr, self.root_context, self.last_cycle)
    }

    /// Read the word at the given Miden memory address, under `ctx`, at cycle `clk`
    pub fn read_memory_word_in_context(
        &self,
        addr: u32,
        ctx: ContextId,
        clk: RowIndex,
    ) -> Option<Word> {
        use miden_core::FieldElement;

        const ZERO: Word = Word::new([Felt::ZERO; 4]);

        Some(
            self.memory
                .get_word(ctx, addr)
                .unwrap_or_else(|err| panic!("{err}"))
                .unwrap_or(ZERO),
        )
    }

    /// Read the word at the given Miden memory address and element offset
    #[track_caller]
    pub fn read_memory_element(&self, addr: u32) -> Option<Felt> {
        self.memory.get_value(self.root_context, addr)
    }

    /// Read the word at the given Miden memory address and element offset, under `ctx`, at cycle
    /// `clk`
    #[track_caller]
    pub fn read_memory_element_in_context(
        &self,
        addr: u32,
        ctx: ContextId,
        _clk: RowIndex,
    ) -> Option<Felt> {
        self.memory.get_value(ctx, addr)
    }

    /// Read a raw byte vector from `addr`, under `ctx`, at cycle `clk`, sufficient to hold a value
    /// of type `ty`
    pub fn read_bytes_for_type(
        &self,
        addr: NativePtr,
        ty: &Type,
        ctx: ContextId,
        clk: RowIndex,
    ) -> Result<Vec<u8>, MemoryReadError> {
        const U32_MASK: u64 = u32::MAX as u64;
        let size = ty.size_in_bytes();
        let mut buf = Vec::with_capacity(size);

        let size_in_felts = ty.size_in_felts();
        let mut elems = Vec::with_capacity(size_in_felts);

        if addr.is_element_aligned() {
            for i in 0..size_in_felts {
                let addr = addr.addr.checked_add(i as u32).ok_or(MemoryReadError::OutOfBounds)?;
                elems.push(self.read_memory_element_in_context(addr, ctx, clk).unwrap_or_default());
            }
        } else {
            return Err(MemoryReadError::UnalignedRead);
        }

        let mut needed = size - buf.len();
        for elem in elems {
            let bytes = ((elem.as_int() & U32_MASK) as u32).to_be_bytes();
            let take = core::cmp::min(needed, 4);
            buf.extend(&bytes[0..take]);
            needed -= take;
        }

        Ok(buf)
    }

    /// Read a value of the given type, given an address in Rust's address space
    #[track_caller]
    pub fn read_from_rust_memory<T>(&self, addr: u32) -> Option<T>
    where
        T: core::any::Any + FromMidenRepr,
    {
        self.read_from_rust_memory_in_context(addr, self.root_context, self.last_cycle)
    }

    /// Read a value of the given type, given an address in Rust's address space, under `ctx`, at
    /// cycle `clk`
    #[track_caller]
    pub fn read_from_rust_memory_in_context<T>(
        &self,
        addr: u32,
        ctx: ContextId,
        clk: RowIndex,
    ) -> Option<T>
    where
        T: core::any::Any + FromMidenRepr,
    {
        use core::any::TypeId;

        let ptr = NativePtr::from_ptr(addr);
        if TypeId::of::<T>() == TypeId::of::<Felt>() {
            assert_eq!(ptr.offset, 0, "cannot read values of type Felt from unaligned addresses");
        }
        assert_eq!(ptr.offset, 0, "support for unaligned reads is not yet implemented");
        match <T as FromMidenRepr>::size_in_felts() {
            1 => {
                let felt = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
                Some(T::from_felts(&[felt]))
            }
            2 => {
                let lo = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
                let hi = self.read_memory_element_in_context(ptr.addr + 1, ctx, clk)?;
                Some(T::from_felts(&[lo, hi]))
            }
            3 => {
                let lo_l = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
                let lo_h = self.read_memory_element_in_context(ptr.addr + 1, ctx, clk)?;
                let hi_l = self.read_memory_element_in_context(ptr.addr + 2, ctx, clk)?;
                Some(T::from_felts(&[lo_l, lo_h, hi_l]))
            }
            n => {
                assert_ne!(n, 0);
                let num_words = n.next_multiple_of(4) / 4;
                let mut words = SmallVec::<[_; 2]>::with_capacity(num_words);
                for word_index in 0..(num_words as u32) {
                    let addr = ptr.addr + (word_index * 4);
                    let mut word = self.read_memory_word(addr)?;
                    word.reverse();
                    dbg!(word_index, word);
                    words.push(word);
                }
                words.resize(num_words, Word::new([Felt::ZERO; 4]));
                Some(T::from_words(&words))
            }
        }
    }
}
