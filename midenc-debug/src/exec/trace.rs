use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
};

use miden_assembly::Library as CompiledLibrary;
use miden_core::{Program, StackInputs, Word};
use miden_processor::{
    AdviceInputs, ContextId, ExecutionError, Felt, MastForest, MemAdviceProvider, Process,
    ProcessState, RowIndex, StackOutputs, TraceLenSummary, VmState, VmStateIterator,
};
use midenc_codegen_masm::NativePtr;
pub use midenc_codegen_masm::TraceEvent;
use midenc_hir::Type;
use midenc_session::Session;

use super::MemoryChiplet;
use crate::{debug::CallStack, felt::PopFromStack, DebuggerHost, TestFelt};

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
        T: PopFromStack,
    {
        let mut stack = VecDeque::from_iter(self.outputs.clone().iter().copied().map(TestFelt));
        T::try_pop(&mut stack)
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

        const ZERO: Word = [Felt::ZERO; 4];

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
        T: core::any::Any + PopFromStack,
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
        T: core::any::Any + PopFromStack,
    {
        use core::any::TypeId;

        let ptr = NativePtr::from_ptr(addr);
        if TypeId::of::<T>() == TypeId::of::<Felt>() {
            assert_eq!(ptr.offset, 0, "cannot read values of type Felt from unaligned addresses");
            let elem = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
            let mut stack = VecDeque::from([TestFelt(elem)]);
            return Some(T::try_pop(&mut stack).unwrap_or_else(|| {
                panic!(
                    "could not decode a value of type {} from {}",
                    core::any::type_name::<T>(),
                    addr
                )
            }));
        }
        match core::mem::size_of::<T>() {
            n if n < 4 => {
                if (4 - ptr.offset as usize) < n {
                    todo!("unaligned, split read")
                }
                let elem = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
                let elem = if ptr.offset > 0 {
                    let mask = 2u64.pow(32 - (ptr.offset as u32 * 8)) - 1;
                    let elem = elem.as_int() & mask;
                    Felt::new(elem << (ptr.offset as u64 * 8))
                } else {
                    elem
                };
                let mut stack = VecDeque::from([TestFelt(elem)]);
                Some(T::try_pop(&mut stack).unwrap_or_else(|| {
                    panic!(
                        "could not decode a value of type {} from {}",
                        core::any::type_name::<T>(),
                        addr
                    )
                }))
            }
            4 if ptr.offset > 0 => {
                todo!("unaligned, split read")
            }
            4 => {
                let elem = self.read_memory_element_in_context(ptr.addr, ctx, clk)?;
                let mut stack = VecDeque::from([TestFelt(elem)]);
                Some(T::try_pop(&mut stack).unwrap_or_else(|| {
                    panic!(
                        "could not decode a value of type {} from {}",
                        core::any::type_name::<T>(),
                        addr
                    )
                }))
            }
            n if n <= 16 && ptr.offset > 0 => {
                todo!("unaligned, split read")
            }
            n if n <= 16 => {
                let word = self.read_memory_word_in_context(ptr.addr, ctx, clk)?;
                let mut stack = VecDeque::from_iter(word.into_iter().map(TestFelt));
                Some(T::try_pop(&mut stack).unwrap_or_else(|| {
                    panic!(
                        "could not decode a value of type {} from {}",
                        core::any::type_name::<T>(),
                        addr
                    )
                }))
            }
            n => {
                let mut buf = VecDeque::default();
                let chunks_needed = ((n / 4) as u32) + ((n % 4) > 0) as u32;
                if ptr.offset > 0 {
                    todo!()
                } else {
                    for i in 0..chunks_needed {
                        let elem = self
                            .read_memory_element_in_context(ptr.addr + i, ctx, clk)
                            .expect("invalid memory access");
                        buf.push_back(TestFelt(elem));
                    }
                }
                Some(T::try_pop(&mut buf).unwrap_or_else(|| {
                    panic!(
                        "could not decode a value of type {} from {}",
                        core::any::type_name::<T>(),
                        addr
                    )
                }))
            }
        }
    }
}
