use alloc::{rc::Rc, sync::Arc};

use midenc_session::diagnostics::Span;

/// The field modulus for Miden's prime field
pub const P: u64 = (2u128.pow(64) - 2u128.pow(32) + 1) as u64;

/// Assert that an argument specifying an integer size in bits follows the rules about what
/// integer sizes we support as a general rule.
macro_rules! assert_valid_integer_size {
    ($n:ident) => {
        assert!($n > 0, "invalid integer size: size in bits must be non-zero");
        assert!(
            $n.is_power_of_two(),
            "invalid integer size: size in bits must be a power of two, got {}",
            $n
        );
    };

    ($n:ident, $min:literal) => {
        assert_valid_integer_size!($n);
        assert!(
            $n >= $min,
            "invalid integer size: expected size in bits greater than or equal to {}, got {}",
            $n,
            $min
        );
    };

    ($n:ident, $min:ident) => {
        assert_valid_integer_size!($n);
        assert!(
            $n >= $min,
            "invalid integer size: expected size in bits greater than or equal to {}, got {}",
            $n,
            $min
        );
    };

    ($n:ident, $min:literal, $max:literal) => {
        assert_valid_integer_size!($n, $min);
        assert!(
            $n <= $max,
            "invalid integer size: expected size in bits less than or equal to {}, got {}",
            $n,
            $max
        );
    };

    ($n:ident, $min:ident, $max:literal) => {
        assert_valid_integer_size!($n, $min);
        assert!(
            $n <= $max,
            "invalid integer size: expected size in bits less than or equal to {}, got {}",
            $n,
            $max
        );
    };
}

/// Assert that an argument specifying a zero-based stack index does not access out of bounds
macro_rules! assert_valid_stack_index {
    ($idx:ident) => {
        assert!(
            $idx < 16,
            "invalid stack index: only the first 16 elements on the stack are directly \
             accessible, got {}",
            $idx
        );
    };

    ($idx:expr) => {
        assert!(
            ($idx) < 16,
            "invalid stack index: only the first 16 elements on the stack are directly \
             accessible, got {}",
            $idx
        );
    };
}

pub mod advice;
pub mod binary;
pub mod crypto;
pub mod events;
pub mod ext2;
pub mod felt;
pub mod int128;
pub mod int32;
pub mod int64;
pub mod mem;
pub mod merkle;
pub mod primop;
pub mod smallint;
pub mod unary;

use alloc::collections::BTreeSet;
use core::ops::{Deref, DerefMut};

use miden_assembly::ast::InvokeKind;
use midenc_hir::{Context, Immediate, Operation, SourceSpan, Type, ValueRef};

use super::{Operand, OperandStack};
use crate::{
    Event,
    masm::{self as masm, Op},
};

/// This structure is used to emit the Miden Assembly ops corresponding to an IR instruction.
///
/// When dropped, it ensures that the operand stack is updated to reflect the results of the
/// instruction it was created on behalf of.
pub struct InstOpEmitter<'a> {
    inst: &'a Operation,
    emitter: OpEmitter<'a>,
}
impl<'a> InstOpEmitter<'a> {
    #[inline(always)]
    pub fn new(
        inst: &'a Operation,
        invoked: &'a mut BTreeSet<masm::Invoke>,
        block: &'a mut Vec<masm::Op>,
        stack: &'a mut OperandStack,
    ) -> Self {
        Self {
            inst,
            emitter: OpEmitter::new(invoked, block, stack),
        }
    }
}
impl<'a> Deref for InstOpEmitter<'a> {
    type Target = OpEmitter<'a>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.emitter
    }
}
impl DerefMut for InstOpEmitter<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.emitter
    }
}
impl Drop for InstOpEmitter<'_> {
    fn drop(&mut self) {
        for (i, result) in self.inst.results().iter().copied().enumerate() {
            self.emitter.stack.rename(i, result as ValueRef);
        }
    }
}

/// This structure is used to emit Miden Assembly ops into a given function and block.
///
/// The [OpEmitter] carries limited context of its own, and expects to receive arguments
/// to it's various builder functions to provide necessary context for specific constructs.
pub struct OpEmitter<'a> {
    stack: &'a mut OperandStack,
    invoked: &'a mut BTreeSet<masm::Invoke>,
    current_block: &'a mut Vec<masm::Op>,
}
impl<'a> OpEmitter<'a> {
    /// Build a MASM `assert` instruction with an inline diagnostic.
    #[inline]
    pub fn assert_with_message_inst(
        message: impl Into<Arc<str>>,
        span: SourceSpan,
    ) -> masm::Instruction {
        masm::Instruction::AssertWithError(masm::Immediate::Value(Span::new(span, message.into())))
    }

    /// Build a MASM `assert_eq` instruction with an inline diagnostic.
    #[inline]
    pub fn assert_eq_with_message_inst(
        message: impl Into<Arc<str>>,
        span: SourceSpan,
    ) -> masm::Instruction {
        masm::Instruction::AssertEqWithError(masm::Immediate::Value(Span::new(
            span,
            message.into(),
        )))
    }

    /// Build a MASM `assert_eqw` instruction with an inline diagnostic.
    #[inline]
    pub fn assert_eqw_with_message_inst(
        message: impl Into<Arc<str>>,
        span: SourceSpan,
    ) -> masm::Instruction {
        masm::Instruction::AssertEqwWithError(masm::Immediate::Value(Span::new(
            span,
            message.into(),
        )))
    }

    /// Build a MASM `assertz` instruction with an inline diagnostic.
    #[inline]
    pub fn assertz_with_message_inst(
        message: impl Into<Arc<str>>,
        span: SourceSpan,
    ) -> masm::Instruction {
        masm::Instruction::AssertzWithError(masm::Immediate::Value(Span::new(span, message.into())))
    }

    #[inline(always)]
    pub fn new(
        invoked: &'a mut BTreeSet<masm::Invoke>,
        block: &'a mut Vec<masm::Op>,
        stack: &'a mut OperandStack,
    ) -> Self {
        Self {
            stack,
            invoked,
            current_block: block,
        }
    }

    #[inline]
    pub fn context_rc(&self) -> Rc<Context> {
        self.stack.context_rc()
    }

    #[cfg(test)]
    #[inline(always)]
    pub fn stack_len(&self) -> usize {
        self.stack.len()
    }

    #[inline(always)]
    #[allow(unused)]
    pub fn stack<'c, 'b: 'c>(&'b self) -> &'c OperandStack {
        self.stack
    }

    pub(crate) fn felt_stack_transform(
        &mut self,
        instruction: masm::Instruction,
        inputs: usize,
        outputs: usize,
        context: &str,
        span: SourceSpan,
    ) {
        for _ in 0..inputs {
            let operand = self.pop().expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected {context} operand to be felt");
        }
        self.emit(instruction, span);
        for _ in 0..outputs {
            self.push(Type::Felt);
        }
    }

    #[inline]
    fn maybe_register_invoke(&mut self, op: &masm::Instruction) {
        match op {
            masm::Instruction::Exec(id) => {
                self.invoked.insert(masm::Invoke::new(InvokeKind::Exec, id.clone()));
            }
            masm::Instruction::Call(id) => {
                self.invoked.insert(masm::Invoke::new(InvokeKind::Call, id.clone()));
            }
            masm::Instruction::SysCall(id) => {
                self.invoked.insert(masm::Invoke::new(InvokeKind::SysCall, id.clone()));
            }
            masm::Instruction::ProcRef(id) => {
                self.invoked.insert(masm::Invoke::new(InvokeKind::ProcRef, id.clone()));
            }
            _ => (),
        }
    }

    /// Emit an 'exec' for `callee`, where `callee` is an absolute procedure path.
    ///
    /// It is assumed that all necessary operands are on the operand stack in the correct position
    fn raw_exec(&mut self, callee: &str, span: SourceSpan) {
        let callee: midenc_hir::FunctionIdent = callee.parse().unwrap();
        let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
            span,
            callee.function.as_str().into(),
        )));
        let module_name = callee.module.as_str();
        let module = masm::LibraryPath::new(module_name).unwrap();
        let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
        let target = masm::InvocationTarget::Path(Span::new(span, qualified.into_inner()));
        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::Exec(target), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
    }

    /// Emit `op` to the current block
    #[inline(always)]
    pub fn emit(&mut self, inst: masm::Instruction, span: SourceSpan) {
        self.maybe_register_invoke(&inst);
        self.current_block.push(Op::Inst(Span::new(span, inst)));
    }

    /// Emit a `push` instruction of `value` to the current block
    #[inline]
    pub fn emit_push(
        &mut self,
        value: impl Into<miden_assembly_syntax::parser::PushValue>,
        span: SourceSpan,
    ) {
        let inst = masm::Instruction::Push(masm::Immediate::Value(Span::new(span, value.into())));
        self.current_block.push(Op::Inst(Span::new(span, inst)));
    }

    /// Emit `n` copies of `op` to the current block
    #[inline(always)]
    pub fn emit_n(&mut self, count: usize, inst: masm::Instruction, span: SourceSpan) {
        self.maybe_register_invoke(&inst);
        let op = Op::Inst(Span::new(span, inst));
        for _ in 0..count {
            self.current_block.push(op.clone());
        }
    }

    /// Emit `ops` to the current block
    #[inline(always)]
    pub fn emit_all(&mut self, ops: impl IntoIterator<Item = masm::Instruction>, span: SourceSpan) {
        for op in ops {
            self.emit(op, span);
        }
    }

    /// Emit `n` copies of the sequence `ops` to the current block
    #[inline(always)]
    pub fn emit_repeat(&mut self, count: usize, ops: &[Span<masm::Instruction>]) {
        for op in ops {
            self.maybe_register_invoke(op.inner());
        }
        for _ in 0..count {
            self.current_block.extend(ops.iter().cloned().map(|inst| {
                let (span, inst) = inst.into_parts();
                Op::Inst(Span::new(span, inst))
            }));
        }
    }

    /// Emit `n` copies of the sequence `ops` to the current block
    #[inline]
    pub fn emit_template<const N: usize, F>(&mut self, count: usize, template: F)
    where
        F: Fn(usize) -> [Span<masm::Instruction>; N],
    {
        for op in template(0) {
            self.maybe_register_invoke(op.inner());
        }

        for n in 0..count {
            self.current_block.extend(template(n).into_iter().map(|inst| {
                let (span, inst) = inst.into_parts();
                Op::Inst(Span::new(span, inst))
            }));
        }
    }

    /// Push an immediate value on the operand stack
    ///
    /// This has no effect on the state of the emulated operand stack
    #[inline]
    pub fn push_immediate(&mut self, imm: Immediate, span: SourceSpan) {
        match imm {
            Immediate::I1(i) => self.emit_push(i as u8, span),
            Immediate::I8(i) => self.emit_push(i as u8, span),
            Immediate::U8(i) => self.emit_push(i, span),
            Immediate::U16(i) => self.emit_push(i, span),
            Immediate::I16(i) => self.emit_push(i as u16, span),
            Immediate::U32(i) => self.emit_push(i, span),
            Immediate::I32(i) => self.emit_push(i as u32, span),
            Immediate::U64(i) => self.push_u64(i, span),
            Immediate::I64(i) => self.push_i64(i, span),
            Immediate::U128(i) => self.push_u128(i, span),
            Immediate::I128(i) => self.push_i128(i, span),
            Immediate::Felt(i) => self.emit_push(i, span),
            Immediate::F64(_) => unimplemented!("floating-point immediates are not supported"),
        }
    }

    /// Push a literal on the operand stack, and update the emulated stack accordingly
    pub fn literal<I: Into<Immediate>>(&mut self, imm: I, span: SourceSpan) {
        let imm = imm.into();
        self.push_immediate(imm, span);
        self.stack.push_immediate(imm);
    }

    /// Make the `n`th operand from the top of the stack `value`, whose type may differ from the
    /// operand's but must occupy the same number of elements.
    ///
    /// See [OperandStack::retype].
    #[inline(always)]
    pub fn retype(&mut self, n: usize, value: ValueRef) {
        self.stack.retype(n, value);
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<Operand> {
        self.stack.pop()
    }

    /// Push an operand on the stack
    #[inline(always)]
    pub fn push<O: Into<Operand>>(&mut self, operand: O) {
        self.stack.push(operand)
    }

    /// Duplicate an item on the stack to the top
    #[inline]
    #[track_caller]
    pub fn dup(&mut self, i: u8, span: SourceSpan) {
        assert_valid_stack_index!(i);
        let index = i as usize;
        let i = self.stack.effective_index(index);
        self.stack.dup(index);
        // Emit low-level instructions corresponding to the operand we duplicated
        let last = self.stack.peek().expect("operand stack is empty");
        let n = last.size();
        let offset = n - 1;
        for _ in 0..n {
            self.emit(dup_from_offset(i + offset), span);
        }
    }

    /// Move an item on the stack to the top
    #[inline]
    #[track_caller]
    pub fn movup(&mut self, i: u8, span: SourceSpan) {
        assert_valid_stack_index!(i);
        let index = i as usize;
        let i = self.stack.effective_index(index);
        self.stack.movup(index);
        // Emit low-level instructions corresponding to the operand we moved
        let moved = self.stack.peek().expect("operand stack is empty");
        let n = moved.size();
        let offset = n - 1;
        for _ in 0..n {
            self.emit(movup_from_offset(i + offset), span);
        }
    }

    /// Move an item from the top of the stack to the `n`th position
    #[inline]
    #[track_caller]
    pub fn movdn(&mut self, i: u8, span: SourceSpan) {
        assert_valid_stack_index!(i);
        let index = i as usize;
        let i = self.stack.effective_index_inclusive(index);
        let top = self.stack.peek().expect("operand stack is empty");
        let top_size = top.size();
        self.stack.movdn(index);
        // Emit low-level instructions corresponding to the operand we moved
        for _ in 0..top_size {
            self.emit(movdn_from_offset(i), span);
        }
    }

    /// Swap an item with the top of the stack
    #[inline]
    #[track_caller]
    pub fn swap(&mut self, i: u8, span: SourceSpan) {
        assert!(i > 0, "swap requires a non-zero index");
        assert_valid_stack_index!(i);
        let index = i as usize;
        let src = self.stack[0].size();
        let dst = self.stack[index].size();
        let i = self.stack.effective_index(index);
        self.stack.swap(index);
        match (src, dst) {
            (1, 1) => {
                self.emit(swap_from_offset(i), span);
            }
            (1, n) if i == 1 => {
                // We can simply move the top element below the `dst` operand
                self.emit(movdn_from_offset(i + (n - 1)), span);
            }
            (n, 1) if i == n => {
                // We can simply move the `dst` element to the top
                self.emit(movup_from_offset(i), span);
            }
            (n, m) if i == n => {
                // We can simply move `dst` down
                for _ in 0..n {
                    self.emit(movdn_from_offset(i + (m - 1)), span);
                }
            }
            (n, m) => {
                assert!(i >= n);
                let offset = m - 1;
                for _ in 0..n {
                    self.emit(movdn_from_offset(i + offset), span);
                }
                let i = (i as i8 + (m as i8 - n as i8)) as u8 as usize;
                match i - 1 {
                    1 => {
                        assert_eq!(m, 1);
                        self.emit(masm::Instruction::Swap1, span);
                    }
                    i => {
                        for _ in 0..m {
                            self.emit(movup_from_offset(i), span);
                        }
                    }
                }
            }
        }
    }

    /// Drop the top operand on the stack
    #[inline]
    #[track_caller]
    pub fn drop(&mut self, span: SourceSpan) {
        let elem = self.stack.pop().expect("operand stack is empty");
        match elem.size() {
            1 => {
                self.emit(masm::Instruction::Drop, span);
            }
            4 => {
                self.emit(masm::Instruction::DropW, span);
            }
            n => {
                for _ in 0..n {
                    self.emit(masm::Instruction::Drop, span);
                }
            }
        }
    }

    /// Drop the top `n` operands on the stack
    #[inline]
    #[track_caller]
    pub fn dropn(&mut self, n: usize, span: SourceSpan) {
        assert!(self.stack.len() >= n);
        assert_ne!(n, 0);
        let raw_len: usize = self.stack.iter().rev().take(n).map(|o| o.size()).sum();
        self.stack.dropn(n);
        match raw_len {
            1 => {
                self.emit(masm::Instruction::Drop, span);
            }
            4 => {
                self.emit(masm::Instruction::DropW, span);
            }
            n => {
                self.emit_n(n / 4, masm::Instruction::DropW, span);
                self.emit_n(n % 4, masm::Instruction::Drop, span);
            }
        }
    }

    /// Remove all but the top `n` values on the operand stack
    pub fn truncate_stack(&mut self, n: usize, span: SourceSpan) {
        let stack_size = self.stack.len();
        let num_to_drop = stack_size - n;

        if num_to_drop == 0 {
            return;
        }

        if stack_size == num_to_drop {
            let raw_size = self.stack.raw_len();
            self.stack.dropn(num_to_drop);
            self.emit_n(raw_size / 4, masm::Instruction::DropW, span);
            self.emit_n(raw_size % 4, masm::Instruction::Drop, span);
            return;
        }

        // This is the common case, and can be handled simply
        // by moving the value to the bottom of the stack and
        // dropping everything in-between
        if n == 1 {
            match stack_size {
                2 => {
                    self.swap(1, span);
                    self.drop(span);
                }
                n => {
                    self.movdn(n as u8 - 1, span);
                    self.dropn(n - 1, span);
                }
            }
            return;
        }

        // TODO: This is a very neive algorithm for clearing
        // the stack of all but the top `n` values, we should
        // come up with a smarter/more efficient method
        for offset in 0..num_to_drop {
            let index = stack_size - 1 - offset;
            self.drop_operand_at_position(index, span);
        }
    }

    /// Remove the `n`th value from the top of the operand stack
    pub fn drop_operand_at_position(&mut self, n: usize, span: SourceSpan) {
        match n {
            0 => {
                self.drop(span);
            }
            1 => {
                self.swap(1, span);
                self.drop(span);
            }
            n => {
                self.movup(n as u8, span);
                self.drop(span);
            }
        }
    }

    /// Copy the `n`th operand on the stack, and make it the `m`th operand on the stack.
    ///
    /// If the operand is for a commutative, binary operator, indicated by
    /// `is_commutative_binary_operand`, and the desired position is just below the top of
    /// stack, this function may leave it on top of the stack instead, since the order of the
    /// operands is not strict. This can result in fewer stack manipulation instructions in some
    /// scenarios.
    pub fn copy_operand_to_position(
        &mut self,
        n: usize,
        m: usize,
        is_commutative_binary_operand: bool,
        span: SourceSpan,
    ) {
        match (n, m) {
            (0, 0) => {
                self.dup(0, span);
            }
            (actual, 0) => {
                self.dup(actual as u8, span);
            }
            (actual, 1) => {
                // If the dependent is binary+commutative, we can
                // leave operands in either the 0th or 1st position,
                // as long as both operands are on top of the stack
                if !is_commutative_binary_operand {
                    self.dup(actual as u8, span);
                    self.swap(1, span);
                } else {
                    self.dup(actual as u8, span);
                }
            }
            (actual, expected) => {
                self.dup(actual as u8, span);
                self.movdn(expected as u8, span);
            }
        }
    }

    /// Make the `n`th operand on the stack, the `m`th operand on the stack.
    ///
    /// If the operand is for a commutative, binary operator, indicated by
    /// `is_commutative_binary_operand`, and the desired position is one of the first two items
    /// on the stack, this function may leave the operand in it's current position if it is
    /// already one of the first two items on the stack, since the order of the operands is not
    /// strict. This can result in fewer stack manipulation instructions in some scenarios.
    pub fn move_operand_to_position(
        &mut self,
        n: usize,
        m: usize,
        is_commutative_binary_operand: bool,
        span: SourceSpan,
    ) {
        match (n, m) {
            (n, m) if n == m => (),
            (1, 0) | (0, 1) => {
                // If the dependent is binary+commutative, we can
                // leave operands in either the 0th or 1st position,
                // as long as both operands are on top of the stack
                if !is_commutative_binary_operand {
                    self.swap(1, span);
                }
            }
            (actual, 0) => {
                self.movup(actual as u8, span);
            }
            (actual, 1) => {
                self.movup(actual as u8, span);
                self.swap(1, span);
            }
            (actual, expected) => {
                self.movup(actual as u8, span);
                self.movdn(expected as u8, span);
            }
        }
    }

    /// Get mutable access to the current block we're emitting to
    #[cfg(test)]
    #[inline(always)]
    pub fn current_block<'c, 'b: 'c>(&'b mut self) -> &'c mut Vec<masm::Op> {
        self.current_block
    }
}

pub fn dup_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        0 => masm::Instruction::Dup0,
        1 => masm::Instruction::Dup1,
        2 => masm::Instruction::Dup2,
        3 => masm::Instruction::Dup3,
        4 => masm::Instruction::Dup4,
        5 => masm::Instruction::Dup5,
        6 => masm::Instruction::Dup6,
        7 => masm::Instruction::Dup7,
        8 => masm::Instruction::Dup8,
        9 => masm::Instruction::Dup9,
        10 => masm::Instruction::Dup10,
        11 => masm::Instruction::Dup11,
        12 => masm::Instruction::Dup12,
        13 => masm::Instruction::Dup13,
        14 => masm::Instruction::Dup14,
        15 => masm::Instruction::Dup15,
        invalid => panic!("invalid stack offset for dup: {invalid} is out of range"),
    }
}

pub fn swap_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        1 => masm::Instruction::Swap1,
        2 => masm::Instruction::Swap2,
        3 => masm::Instruction::Swap3,
        4 => masm::Instruction::Swap4,
        5 => masm::Instruction::Swap5,
        6 => masm::Instruction::Swap6,
        7 => masm::Instruction::Swap7,
        8 => masm::Instruction::Swap8,
        9 => masm::Instruction::Swap9,
        10 => masm::Instruction::Swap10,
        11 => masm::Instruction::Swap11,
        12 => masm::Instruction::Swap12,
        13 => masm::Instruction::Swap13,
        14 => masm::Instruction::Swap14,
        15 => masm::Instruction::Swap15,
        invalid => panic!("invalid stack offset for swap: {invalid} is out of range"),
    }
}

#[allow(unused)]
pub fn swapw_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        1 => masm::Instruction::SwapW1,
        2 => masm::Instruction::SwapW2,
        3 => masm::Instruction::SwapW3,
        invalid => panic!("invalid stack offset for swapw: {invalid} is out of range"),
    }
}

pub fn movup_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        2 => masm::Instruction::MovUp2,
        3 => masm::Instruction::MovUp3,
        4 => masm::Instruction::MovUp4,
        5 => masm::Instruction::MovUp5,
        6 => masm::Instruction::MovUp6,
        7 => masm::Instruction::MovUp7,
        8 => masm::Instruction::MovUp8,
        9 => masm::Instruction::MovUp9,
        10 => masm::Instruction::MovUp10,
        11 => masm::Instruction::MovUp11,
        12 => masm::Instruction::MovUp12,
        13 => masm::Instruction::MovUp13,
        14 => masm::Instruction::MovUp14,
        15 => masm::Instruction::MovUp15,
        invalid => panic!("invalid stack offset for movup: {invalid} is out of range"),
    }
}

pub fn movdn_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        2 => masm::Instruction::MovDn2,
        3 => masm::Instruction::MovDn3,
        4 => masm::Instruction::MovDn4,
        5 => masm::Instruction::MovDn5,
        6 => masm::Instruction::MovDn6,
        7 => masm::Instruction::MovDn7,
        8 => masm::Instruction::MovDn8,
        9 => masm::Instruction::MovDn9,
        10 => masm::Instruction::MovDn10,
        11 => masm::Instruction::MovDn11,
        12 => masm::Instruction::MovDn12,
        13 => masm::Instruction::MovDn13,
        14 => masm::Instruction::MovDn14,
        15 => masm::Instruction::MovDn15,
        invalid => panic!("invalid stack offset for movdn: {invalid} is out of range"),
    }
}

#[allow(unused)]
pub fn movupw_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        2 => masm::Instruction::MovUpW2,
        3 => masm::Instruction::MovUpW3,
        invalid => panic!("invalid stack offset for movupw: {invalid} is out of range"),
    }
}

#[allow(unused)]
pub fn movdnw_from_offset(offset: usize) -> masm::Instruction {
    match offset {
        2 => masm::Instruction::MovDnW2,
        3 => masm::Instruction::MovDnW3,
        invalid => panic!("invalid stack offset for movdnw: {invalid} is out of range"),
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_expect_test::{Expect, expect};
    use midenc_hir::{
        ArrayType, Context, Felt, Overflow, PointerType, ValueRef,
        dialects::builtin::attributes::Signature, formatter::PrettyPrint,
    };

    use super::*;
    use crate::masm::{self, Op};

    macro_rules! push {
        ($x:expr) => {
            Op::Inst(Span::unknown(masm::Instruction::Push(masm::Immediate::Value(Span::unknown(
                miden_assembly_syntax::parser::PushValue::from($x),
            )))))
        };
    }

    /// Run `emit` on an emitter whose operand stack holds values of the types in `inputs`, the
    /// first on top, and pin the MASM it emits.
    fn assert_emits(inputs: &[Type], emit: impl FnOnce(&mut OpEmitter<'_>), masm: Expect) {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
            for ty in inputs.iter().rev() {
                emitter.push(ty.clone());
            }
            emit(&mut emitter);
        }
        // The block prints one level in, below an empty line
        let printed = masm::Block::new(SourceSpan::UNKNOWN, block).to_pretty_string();
        let mut lines = String::new();
        for line in printed.trim_start_matches('\n').lines() {
            lines.push_str(line.strip_prefix("    ").unwrap_or(line));
            lines.push('\n');
        }
        masm.assert_eq(&lines);
    }

    /// Assert that the emitted block ends by delegating to the dedicated 16-bit memory intrinsic.
    fn assert_unaligned_16bit_intrinsic(block: &[Op], intrinsic: &str) {
        let execs = block
            .iter()
            .filter_map(|op| match op {
                Op::Inst(inst) => match inst.inner() {
                    masm::Instruction::Exec(target) => Some(target.to_string()),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            execs.iter().any(|target| target == intrinsic),
            "expected block to delegate to `{intrinsic}`, found execs: {execs:?}"
        );
    }

    #[test]
    fn op_emitter_stack_manipulation_test() {
        let mut block = Vec::default();

        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);
        let three = Immediate::U8(3);
        let four = Immediate::U64(2u64.pow(32));
        let five = Immediate::U64(2u64.pow(32) | 2u64.pow(33) | u32::MAX as u64);

        let span = SourceSpan::default();
        emitter.literal(one, span);
        emitter.literal(two, span);
        emitter.literal(three, span);
        emitter.literal(four, span);
        emitter.literal(five, span);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 7);
            assert_eq!(&ops[0], &push!(1u32));
            assert_eq!(&ops[1], &push!(2u32));
            assert_eq!(&ops[2], &push!(3u8));
            // `u64` immediates are pushed as two `u32` limbs in LE stack order (`lo` on top),
            // which means the hi limb is pushed first, followed by the lo limb.
            assert_eq!(&ops[3], &push!(Felt::ONE));
            assert_eq!(&ops[4], &push!(Felt::ZERO));
            assert_eq!(&ops[5], &push!(Felt::new_unchecked(3)));
            assert_eq!(&ops[6], &push!(Felt::new_unchecked(u32::MAX as u64)));
        }

        assert_eq!(emitter.stack()[0], five);
        assert_eq!(emitter.stack()[1], four);
        assert_eq!(emitter.stack()[2], three);
        assert_eq!(emitter.stack()[3], two);
        assert_eq!(emitter.stack()[4], one);

        emitter.dup(0, SourceSpan::default());
        assert_eq!(emitter.stack()[0], five);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], four);
        assert_eq!(emitter.stack()[3], three);
        assert_eq!(emitter.stack()[4], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 9);
            assert_eq!(&ops[7], &Op::Inst(Span::new(span, masm::Instruction::Dup1)));
            assert_eq!(&ops[8], &Op::Inst(Span::new(span, masm::Instruction::Dup1)));
        }

        assert_eq!(emitter.stack().effective_index(3), 6);
        emitter.dup(3, SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], four);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 10);
            assert_eq!(&ops[8], &Op::Inst(Span::new(span, masm::Instruction::Dup1)));
            assert_eq!(&ops[9], &Op::Inst(Span::new(span, masm::Instruction::Dup6)));
        }

        assert_eq!(emitter.stack().effective_index(1), 1);
        emitter.swap(1, SourceSpan::default());
        assert_eq!(emitter.stack().effective_index(1), 2);
        assert_eq!(emitter.stack()[0], five);
        assert_eq!(emitter.stack()[1], three);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], four);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 11);
            assert_eq!(&ops[10], &Op::Inst(Span::new(span, masm::Instruction::MovDn2)));
        }

        assert_eq!(emitter.stack().effective_index(3), 5);
        emitter.swap(3, SourceSpan::default());
        assert_eq!(emitter.stack()[0], four);
        assert_eq!(emitter.stack()[1], three);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], five);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 15);
            assert_eq!(&ops[10], &Op::Inst(Span::new(span, masm::Instruction::MovDn2))); // [five_a, five_b, three, five_c, five_d, four_a, four_b]
            assert_eq!(&ops[11], &Op::Inst(Span::new(span, masm::Instruction::MovDn6))); // [five_b, three, five_c, five_d, four_a, four_b, five_a]
            assert_eq!(&ops[12], &Op::Inst(Span::new(span, masm::Instruction::MovDn6))); // [three, five_c, five_d, four_a, four_b, five_a, five_b]
            assert_eq!(&ops[13], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_b, three, five_c, five_d, four_a, five_a, five_b]
            assert_eq!(&ops[14], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_a, four_b, three, five_c,
            // five_d,
            // five_a,
            // five_b]
        }

        emitter.movdn(2, SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], four);
        assert_eq!(emitter.stack()[3], five);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 17);
            assert_eq!(&ops[12], &Op::Inst(Span::new(span, masm::Instruction::MovDn6))); // [three, five_c, five_d, four_a, four_b, five_a, five_b]
            assert_eq!(&ops[13], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_b, three, five_c, five_d, four_a, five_a, five_b]
            assert_eq!(&ops[14], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_a, four_b, three, five_c, five_d, five_a, five_b]
            assert_eq!(&ops[15], &Op::Inst(Span::new(span, masm::Instruction::MovDn4))); // [four_b, three, five_c, five_d, four_a, five_a, five_b]
            assert_eq!(&ops[16], &Op::Inst(Span::new(span, masm::Instruction::MovDn4))); // [three, five_c, five_d, four_a,
            // four_b,
            // five_a,
            // five_b]
        }

        emitter.movup(2, SourceSpan::default());
        assert_eq!(emitter.stack()[0], four);
        assert_eq!(emitter.stack()[1], three);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], five);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 19);
            assert_eq!(&ops[16], &Op::Inst(Span::new(span, masm::Instruction::MovDn4))); // [three, five_c, five_d, four_a, four_b, five_a, five_b]
            assert_eq!(&ops[17], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_b, three, five_c, five_d, four_a, five_a, five_b]
            assert_eq!(&ops[18], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_a, four_b, three, five_c,
            // five_d,
            // five_a,
            // five_b]
        }

        emitter.drop(SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], three);
        assert_eq!(emitter.stack()[4], two);
        assert_eq!(emitter.stack()[5], one);

        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 21);
            assert_eq!(&ops[18], &Op::Inst(Span::new(span, masm::Instruction::MovUp4))); // [four_a, four_b, three, five_c, five_d, five_a, five_b]
            assert_eq!(&ops[19], &Op::Inst(Span::new(span, masm::Instruction::Drop))); // [four_b, three, five_c, five_d, five_a, five_b]
            assert_eq!(&ops[20], &Op::Inst(Span::new(span, masm::Instruction::Drop))); // [three, five_c, five_d, five_a, five_b]
        }

        emitter.copy_operand_to_position(5, 3, false, SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], one);
        assert_eq!(emitter.stack()[4], three);
        assert_eq!(emitter.stack()[5], two);

        emitter.drop_operand_at_position(4, SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], five);
        assert_eq!(emitter.stack()[3], one);
        assert_eq!(emitter.stack()[4], two);

        emitter.move_operand_to_position(4, 2, false, SourceSpan::default());
        assert_eq!(emitter.stack()[0], three);
        assert_eq!(emitter.stack()[1], five);
        assert_eq!(emitter.stack()[2], two);
        assert_eq!(emitter.stack()[3], five);
        assert_eq!(emitter.stack()[4], one);
    }

    #[test]
    fn op_emitter_copy_operand_to_position_test() {
        let mut block = Vec::default();
        let mut invoked = BTreeSet::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let unused_block = context.create_block_with_params(vec![Type::U32; 7]);
        let unused_block = unused_block.borrow();
        let unused_block_args = unused_block.arguments();

        let v0 = unused_block_args[0] as ValueRef;
        let v1 = unused_block_args[1] as ValueRef;
        let v2 = unused_block_args[2] as ValueRef;
        let v3 = unused_block_args[3] as ValueRef;
        let v4 = unused_block_args[4] as ValueRef;
        let v5 = unused_block_args[5] as ValueRef;
        let v6 = unused_block_args[6] as ValueRef;

        emitter.push(v1);
        emitter.push(v0);
        emitter.push(v3);
        emitter.push(v4);
        emitter.push(v6);
        emitter.push(v5);
        emitter.push(v2);

        assert_eq!(emitter.stack().find(&v3), Some(4));

        assert_eq!(emitter.stack()[4], v3);
        assert_eq!(emitter.stack()[2], v6);
        emitter.copy_operand_to_position(4, 2, false, SourceSpan::default());
        assert_eq!(emitter.stack()[5], v3);
        assert_eq!(emitter.stack()[2], v3);

        let span = SourceSpan::default();
        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 2);
            assert_eq!(&ops[0], &Op::Inst(Span::new(span, masm::Instruction::Dup4)));
            assert_eq!(&ops[1], &Op::Inst(Span::new(span, masm::Instruction::MovDn2)));
        }
    }

    #[test]
    fn op_emitter_u32_add_test() {
        let mut block = Vec::default();
        let mut invoked = BTreeSet::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.add_imm(one, Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.add(Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);

        emitter.add_imm(one, Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);

        emitter.swap(1, SourceSpan::default());
        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.add(Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);
    }

    #[test]
    fn op_emitter_u32_sub_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.sub_imm(one, Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.sub(Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);

        emitter.sub_imm(one, Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);

        emitter.swap(1, SourceSpan::default());
        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.sub(Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);
    }

    #[test]
    fn op_emitter_u32_mul_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.mul_imm(one, Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.mul(Overflow::Checked, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);

        emitter.mul_imm(one, Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);

        emitter.swap(1, SourceSpan::default());
        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.mul(Overflow::Overflowing, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::I1);
    }

    #[test]
    fn op_emitter_u32_eq_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.eq_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.assert(None, None, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], one);

        emitter.dup(0, SourceSpan::default());
        emitter.eq(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_assert_uses_explicit_message() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.literal(Immediate::I1(true), SourceSpan::default());
        emitter.assert(None, Some("preserved MASM assertion"), SourceSpan::default());

        let Some(Op::Inst(inst)) = block.last() else {
            panic!("expected emitted assert instruction");
        };
        let masm::Instruction::AssertWithError(masm::Immediate::Value(message)) = inst.inner()
        else {
            panic!("expected assert.err instruction, got {:?}", inst.inner());
        };
        assert_eq!(message.inner().as_ref(), "preserved MASM assertion");
    }

    #[test]
    fn op_emitter_u32_neq_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.neq_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.assertz(None, None, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], one);

        emitter.dup(0, SourceSpan::default());
        emitter.neq(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_i1_and_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let t = Immediate::I1(true);
        let f = Immediate::I1(false);

        emitter.literal(t, SourceSpan::default());
        emitter.literal(f, SourceSpan::default());

        emitter.and_imm(t, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], t);

        emitter.and(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_i1_or_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let t = Immediate::I1(true);
        let f = Immediate::I1(false);

        emitter.literal(t, SourceSpan::default());
        emitter.literal(f, SourceSpan::default());

        emitter.or_imm(t, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], t);

        emitter.or(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_i1_xor_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let t = Immediate::I1(true);
        let f = Immediate::I1(false);

        emitter.literal(t, SourceSpan::default());
        emitter.literal(f, SourceSpan::default());

        emitter.xor_imm(t, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], t);

        emitter.xor(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_i1_not_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let t = Immediate::I1(true);

        emitter.literal(t, SourceSpan::default());

        emitter.not(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_gt_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.gt_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.gt(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_gte_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.gte_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.gte(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_lt_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.lt_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.lt(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_lte_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.lte_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I1);
        assert_eq!(emitter.stack()[1], one);

        emitter.drop(SourceSpan::default());
        emitter.dup(0, SourceSpan::default());
        emitter.lte(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_checked_div_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.checked_div_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.checked_div(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_unchecked_div_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.unchecked_div_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.unchecked_div(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_checked_mod_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.checked_mod_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.checked_mod(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_unchecked_mod_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.unchecked_mod_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.unchecked_mod(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_checked_divmod_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.checked_divmod_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::U32);
        assert_eq!(emitter.stack()[2], one);

        emitter.checked_divmod(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::U32);
        assert_eq!(emitter.stack()[2], one);
    }

    #[test]
    fn op_emitter_u32_unchecked_divmod_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.unchecked_divmod_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::U32);
        assert_eq!(emitter.stack()[2], one);

        emitter.unchecked_divmod(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], Type::U32);
        assert_eq!(emitter.stack()[2], one);
    }

    #[test]
    fn op_emitter_u32_exp_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.exp_imm(two, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.exp(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_band_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.band_imm(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.band(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_bor_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.bor_imm(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.bor(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_bxor_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.bxor_imm(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.bxor(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_shl_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.shl_imm(1, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.shl(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_shr_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.shr_imm(1, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.shr(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_rotl_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.rotl_imm(1, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.rotl(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_rotr_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.rotr_imm(1, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.rotr(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_min_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.min_imm(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.min(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_max_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());

        emitter.max_imm(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::U32);
        assert_eq!(emitter.stack()[1], one);

        emitter.max(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_trunc_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let max = Immediate::U32(u32::MAX);

        emitter.literal(max, SourceSpan::default());

        emitter.trunc(&Type::U16, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U16);
    }

    #[test]
    fn op_emitter_u32_zext_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let one = Immediate::U16(1);

        emitter.literal(one, SourceSpan::default());

        emitter.zext(&Type::U32, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_sext_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let num = Immediate::I16(-128);

        emitter.literal(num, SourceSpan::default());

        emitter.sext(&Type::I32, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I32);
    }

    #[test]
    fn op_emitter_u32_cast_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let num = Immediate::U32(128);

        emitter.literal(num, SourceSpan::default());

        emitter.cast(&Type::I32, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I32);
    }

    #[test]
    fn op_emitter_u32_is_odd_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let num = Immediate::U32(128);

        emitter.literal(num, SourceSpan::default());

        emitter.is_odd(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I1);
    }

    #[test]
    fn op_emitter_u32_popcnt_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let num = Immediate::U32(128);

        emitter.literal(num, SourceSpan::default());

        emitter.popcnt(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_bnot_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let num = Immediate::U32(128);

        emitter.literal(num, SourceSpan::default());

        emitter.bnot(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u128_bnot_shadowing_fix_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        // Push a U128-typed operand; value is irrelevant for emission semantics here
        emitter.push(Type::U128);
        assert_eq!(emitter.stack_len(), 1);

        let span = SourceSpan::default();
        emitter.bnot(span);

        // Type preserved, no panic
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U128);

        // For 128-bit values, limb count is 4; template should emit MovUp3 + U32Not per limb, which
        // inverts the four limbs of the value and nothing below it, and leaves them in order
        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 8);
            assert_eq!(&ops[0], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[1], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[2], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[3], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[4], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[5], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[6], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[7], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
        }
    }

    #[test]
    fn op_emitter_i128_bnot_shadowing_fix_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        // Push an I128-typed operand
        emitter.push(Type::I128);
        assert_eq!(emitter.stack_len(), 1);

        let span = SourceSpan::default();
        emitter.bnot(span);

        // Type preserved, no panic
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I128);

        // Expect same instruction pattern as U128
        {
            let ops = emitter.current_block();
            assert_eq!(ops.len(), 8);
            assert_eq!(&ops[0], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[1], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[2], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[3], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[4], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[5], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
            assert_eq!(&ops[6], &Op::Inst(Span::new(span, masm::Instruction::MovUp3)));
            assert_eq!(&ops[7], &Op::Inst(Span::new(span, masm::Instruction::U32Not)));
        }
    }

    #[test]
    fn op_emitter_u32_pow2_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::U32(10);

        emitter.literal(ten, SourceSpan::default());

        emitter.pow2(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_incr_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::U32(10);

        emitter.literal(ten, SourceSpan::default());

        emitter.incr(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_inv_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::Felt(Felt::new_unchecked(10));

        emitter.literal(ten, SourceSpan::default());

        emitter.inv(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::Felt);
    }

    #[test]
    fn op_emitter_neg_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::Felt(Felt::new_unchecked(10));

        emitter.literal(ten, SourceSpan::default());

        emitter.neg(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::Felt);
    }

    #[test]
    fn op_emitter_u32_assert_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::U32(10);

        emitter.literal(ten, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);

        emitter.assert(None, None, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 0);
    }

    #[test]
    fn op_emitter_u32_assertz_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::U32(10);

        emitter.literal(ten, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);

        emitter.assertz(None, None, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 0);
    }

    #[test]
    fn op_emitter_u32_assert_eq_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let ten = Immediate::U32(10);

        emitter.literal(ten, SourceSpan::default());
        emitter.literal(ten, SourceSpan::default());
        emitter.literal(ten, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);

        emitter.assert_eq_imm(ten, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);

        emitter.assert_eq(None, None, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 0);
    }

    #[test]
    fn op_emitter_u32_select_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let t = Immediate::I1(true);
        let one = Immediate::U32(1);
        let two = Immediate::U32(2);

        emitter.literal(one, SourceSpan::default());
        emitter.literal(two, SourceSpan::default());
        emitter.literal(t, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 3);

        emitter.select(SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
    }

    #[test]
    fn op_emitter_u32_exec_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let return_ty = Type::from(ArrayType::new(Type::U32, 1));
        let callee = {
            let name = masm::ProcedureName::new("add").unwrap();
            let module = masm::LibraryPath::new("test").unwrap();
            let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
            masm::InvocationTarget::Path(Span::new(SourceSpan::default(), qualified.into_inner()))
        };
        let signature = Signature::new(&context, [Type::U32, Type::I1], [return_ty.clone()]);

        let t = Immediate::I1(true);
        let one = Immediate::U32(1);

        emitter.literal(t, SourceSpan::default());
        emitter.literal(one, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);

        emitter.exec(callee, &signature, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], return_ty);
    }

    #[test]
    fn op_emitter_u32_load_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let addr = Type::from(PointerType::new(Type::U32));

        emitter.push(addr);
        assert_eq!(emitter.stack_len(), 1);

        emitter.load(Type::U32, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);

        emitter.load_imm(128, Type::I32, SourceSpan::default());
        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::I32);
        assert_eq!(emitter.stack()[1], Type::U32);
    }

    #[test]
    fn op_emitter_unaligned_u16_load_imm_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.load_imm(130, Type::U16, SourceSpan::default());

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U16);
        assert_unaligned_16bit_intrinsic(&block, "::intrinsics::mem::load_u16");
    }

    #[test]
    fn op_emitter_unaligned_i16_load_imm_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.load_imm(130, Type::I16, SourceSpan::default());

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I16);
        assert_unaligned_16bit_intrinsic(&block, "::intrinsics::mem::load_u16");
    }

    #[test]
    fn op_emitter_unaligned_u16_store_imm_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.push(Type::U16);
        emitter.store_imm(130, SourceSpan::default());

        assert_eq!(emitter.stack_len(), 0);
        assert_unaligned_16bit_intrinsic(&block, "::intrinsics::mem::store_u16");
    }

    #[test]
    fn op_emitter_unaligned_i16_store_imm_test() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.push(Type::I16);
        emitter.store_imm(130, SourceSpan::default());

        assert_eq!(emitter.stack_len(), 0);
        assert_unaligned_16bit_intrinsic(&block, "::intrinsics::mem::store_u16");
    }

    #[test]
    fn op_emitter_truncate_stack_drops_all_with_remainder() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let span = SourceSpan::default();

        // Push 5 elements (raw_size = 5, so 5 / 4 = 1 DropW, 5 % 4 = 1 Drop)
        emitter.push(Type::U32);
        emitter.push(Type::U32);
        emitter.push(Type::U32);
        emitter.push(Type::U32);
        emitter.push(Type::U32);
        assert_eq!(emitter.stack_len(), 5);

        // truncate_stack(0) should drop all 5 elements
        emitter.truncate_stack(0, span);
        assert_eq!(emitter.stack_len(), 0);

        {
            let ops = emitter.current_block();
            // Should emit 1 DropW (for 4 elements) + 1 Drop (for remainder 1 element)
            assert_eq!(ops.len(), 2);
            assert_eq!(&ops[0], &Op::Inst(Span::new(span, masm::Instruction::DropW)));
            assert_eq!(&ops[1], &Op::Inst(Span::new(span, masm::Instruction::Drop)));
        }
    }

    /// A `u128` literal is pushed most significant half first, so its least significant limb ends
    /// on top: `[x0, x1, x2, x3]`.
    #[test]
    fn a_u128_literal_leaves_its_least_significant_limb_on_top() {
        assert_emits(
            &[],
            |emitter| {
                let value = 0x4444_4444_3333_3333_2222_2222_1111_1111u128;
                emitter.literal(Immediate::U128(value), SourceSpan::UNKNOWN);
            },
            expect![[r#"
                push.1145324612
                push.858993459
                push.572662306
                push.286331153
            "#]],
        );
    }

    /// Truncating to 64 bits drops the two most significant limbs, which lie below the low half:
    /// `[x0, x1, x2, x3] => [x0, x1]`.
    #[test]
    fn a_128_bit_trunc_to_64_bits_keeps_the_low_limbs() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.trunc(&Type::U64, SourceSpan::UNKNOWN),
            expect![[r#"
                movup.3
                drop
                movup.2
                drop
            "#]],
        );
    }

    /// Truncating to 32 bits keeps the limb on top: `[x0, x1, x2, x3] => [x0]`.
    #[test]
    fn a_128_bit_trunc_to_32_bits_keeps_the_least_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.trunc(&Type::U32, SourceSpan::UNKNOWN),
            expect![[r#"
                movdn.3
                drop
                drop
                drop
            "#]],
        );
    }

    /// Truncating below 32 bits drops the three high limbs, then masks the one left.
    #[test]
    fn a_128_bit_trunc_below_32_bits_masks_the_least_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.trunc(&Type::U16, SourceSpan::UNKNOWN),
            expect![[r#"
                movdn.3
                drop
                drop
                drop
                push.65535
                u32and
            "#]],
        );
    }

    /// Truncating to a felt combines the low half: `[x0, x1, x2, x3] => [x1 * 2^32 + x0]`.
    #[test]
    fn a_128_bit_trunc_to_felt_combines_the_low_limbs() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.trunc(&Type::Felt, SourceSpan::UNKNOWN),
            expect![[r#"
                movup.3
                drop
                movup.2
                drop
                swap.1
                mul.4294967296
                add
            "#]],
        );
    }

    /// Casting to `u64` asserts that the two most significant limbs are zero, and keeps the low
    /// half: `[x0, x1, x2, x3] => [x0, x1]`.
    #[test]
    fn a_128_bit_cast_to_u64_asserts_the_high_limbs_are_zero() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::U64, SourceSpan::UNKNOWN),
            expect![[r#"
                movup.3
                assertz.err="128-bit value does not fit in u64"
                movup.2
                assertz.err="128-bit value does not fit in u64"
            "#]],
        );
    }

    /// Casting to `u32` asserts that the three high limbs are zero: `[x0, x1, x2, x3] => [x0]`.
    #[test]
    fn a_128_bit_cast_to_u32_asserts_the_three_high_limbs_are_zero() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::U32, SourceSpan::UNKNOWN),
            expect![[r#"
                movdn.3
                assertz.err="128-bit value does not fit in u32"
                assertz.err="128-bit value does not fit in u32"
                assertz.err="128-bit value does not fit in u32"
            "#]],
        );
    }

    /// Casting to a felt casts to `u64`, then range-checks the low half against the modulus.
    #[test]
    fn a_128_bit_cast_to_felt_range_checks_the_low_limbs() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::Felt, SourceSpan::UNKNOWN),
            expect![[r#"
                movup.3
                assertz.err="128-bit value does not fit in u64"
                movup.2
                assertz.err="128-bit value does not fit in u64"
                dup.1
                dup.1
                push.4294967295
                push.1
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u64::lt
                push.6229491882474008289 emit drop
                assert.err="u64 value does not fit in felt"
                swap.1
                mul.4294967296
                add
            "#]],
        );
    }

    /// Casting a `u128` to `i64` casts to `u64`, then asserts the sign bit of `x1` is clear.
    #[test]
    fn a_u128_cast_to_i64_asserts_the_low_limbs_are_non_negative() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::I64, SourceSpan::UNKNOWN),
            expect![[r#"
                movup.3
                assertz.err="128-bit value does not fit in u64"
                movup.2
                assertz.err="128-bit value does not fit in u64"
                swap.1
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i64 value"
                swap.1
            "#]],
        );
    }

    /// Casting an `i128` to `i64` asserts that both high limbs extend the sign of `x1`.
    #[test]
    fn an_i128_cast_to_i64_asserts_the_high_limbs_extend_the_sign() {
        assert_emits(
            &[Type::I128],
            |emitter| emitter.cast(&Type::I64, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.1
                push.2147483648
                u32and
                eq.2147483648
                push.0
                push.4294967295
                movup.2
                cdrop
                movup.3
                dup.1
                assert_eq.err="128-bit value does not fit in i64"
                movup.3
                assert_eq.err="128-bit value does not fit in i64"
            "#]],
        );
    }

    /// Casting an `i128` to `i32` casts to `i64`, then to `i32`.
    #[test]
    fn an_i128_cast_to_i32_narrows_through_the_low_limbs() {
        assert_emits(
            &[Type::I128],
            |emitter| emitter.cast(&Type::I32, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.1
                push.2147483648
                u32and
                eq.2147483648
                push.0
                push.4294967295
                movup.2
                cdrop
                movup.3
                dup.1
                assert_eq.err="128-bit value does not fit in i64"
                movup.3
                assert_eq.err="128-bit value does not fit in i64"
                swap.1
                dup.0
                eq.0
                dup.0
                movdn.2
                push.4294967295
                push.0
                movup.2
                cdrop
                assert_eq.err="i64 value does not fit in signed 32-bit range"
                dup.1
                push.2147483648
                u32and
                push.2147483648
                push.0
                movup.3
                cdrop
                assert_eq.err="i64 value does not fit in signed 32-bit range"
            "#]],
        );
    }

    /// Casting an `i64` to `i32` asserts that the high limb, below the low one, extends the sign of
    /// the low limb.
    #[test]
    fn an_i64_cast_to_i32_asserts_the_high_limb_extends_the_sign() {
        assert_emits(
            &[Type::I64],
            |emitter| emitter.cast(&Type::I32, SourceSpan::UNKNOWN),
            expect![[r#"
                swap.1
                dup.0
                eq.0
                dup.0
                movdn.2
                push.4294967295
                push.0
                movup.2
                cdrop
                assert_eq.err="i64 value does not fit in signed 32-bit range"
                dup.1
                push.2147483648
                u32and
                push.2147483648
                push.0
                movup.3
                cdrop
                assert_eq.err="i64 value does not fit in signed 32-bit range"
            "#]],
        );
    }

    /// Casting a `u128` to `i128` asserts the sign bit of the most significant limb, `x3`, is
    /// clear.
    #[test]
    fn a_u128_cast_to_i128_asserts_the_most_significant_limb_is_non_negative() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::I128, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.3
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i128 value"
            "#]],
        );
    }

    /// Casting a felt to 128 bits splits it, and places the two zero limbs below its own:
    /// `[a] => [a_lo, a_hi, 0, 0]`.
    #[test]
    fn a_felt_cast_to_128_bits_keeps_its_limbs_on_top() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::U128, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                push.0
                push.0
                movup.3
                movup.3
            "#]],
        );
    }

    /// A 64-bit load from an aligned constant address moves the two elements as they are, as the
    /// dynamic `load_dw` does: `[] => [m[a], m[a + 1]]`.
    #[test]
    fn an_aligned_64_bit_load_from_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(128, Type::U64, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_load.33
                mem_load.32
            "#]],
        );
    }

    /// A 64-bit store to an aligned constant address moves the two elements as they are, as the
    /// dynamic `store_dw` does: the limb on top to the lower address.
    #[test]
    fn an_aligned_64_bit_store_to_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[Type::U64],
            |emitter| emitter.store_imm(128, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_store.32
                mem_store.33
            "#]],
        );
    }

    /// A 32-bit load from an aligned constant address moves the element as it is, as the dynamic
    /// `load_sw` does: `[] => [m[a]]`.
    #[test]
    fn an_aligned_32_bit_load_from_a_constant_address_moves_the_element_unchecked() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(128, Type::U32, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_load.32
            "#]],
        );
    }

    /// A 32-bit store to an aligned constant address moves the element as it is, as the dynamic
    /// `store_sw` does.
    #[test]
    fn an_aligned_32_bit_store_to_a_constant_address_moves_the_element_unchecked() {
        assert_emits(
            &[Type::U32],
            |emitter| emitter.store_imm(128, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_store.32
            "#]],
        );
    }

    /// A 128-bit load from a word-aligned constant address moves the four elements as they are, as
    /// the dynamic `load_qw` does: `[] => [m[a], m[a + 1], m[a + 2], m[a + 3]]`.
    #[test]
    fn a_word_aligned_128_bit_load_from_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(128, Type::U128, SourceSpan::UNKNOWN),
            expect![[r#"
                padw
                mem_loadw_le.32
            "#]],
        );
    }

    /// A 128-bit load from a constant address that is element-aligned but not word-aligned moves
    /// the four elements as they are, as the dynamic `load_qw` does: `[] => [m[a], .., m[a + 3]]`.
    #[test]
    fn an_element_aligned_128_bit_load_from_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(132, Type::U128, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_load.36
                mem_load.35
                mem_load.34
                mem_load.33
            "#]],
        );
    }

    /// A 128-bit store to a word-aligned constant address moves the four elements as they are, as
    /// the dynamic `store_qw` does: the limb on top to the lowest address.
    #[test]
    fn a_word_aligned_128_bit_store_to_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.store_imm(128, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_storew_le.32
                dropw
            "#]],
        );
    }

    /// A 128-bit store to a constant address that is element-aligned but not word-aligned moves
    /// the four elements as they are, as the dynamic `store_qw` does: the limb on top to the lowest
    /// address.
    #[test]
    fn an_element_aligned_128_bit_store_to_a_constant_address_moves_the_elements_unchecked() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.store_imm(132, SourceSpan::UNKNOWN),
            expect![[r#"
                mem_store.33
                mem_store.34
                mem_store.35
                mem_store.36
            "#]],
        );
    }

    /// A 64-bit value is odd when its least significant limb, on top, is: `[lo, hi] => [lo]`.
    #[test]
    fn the_is_odd_of_a_u64_tests_its_least_significant_limb() {
        assert_emits(
            &[Type::U64],
            |emitter| emitter.is_odd(SourceSpan::UNKNOWN),
            expect![[r#"
            swap.1
            drop
            is_odd
        "#]],
        );
    }

    /// A 128-bit value is odd when its least significant limb, on top, is: `[x0, ..] => [x0]`.
    #[test]
    fn the_is_odd_of_a_u128_tests_its_least_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.is_odd(SourceSpan::UNKNOWN),
            expect![[r#"
            movdn.3
            drop
            drop
            drop
            is_odd
        "#]],
        );
    }

    /// `pow2` of a `u64` asserts that the high limb, below the low one, is zero, and raises 2 to the
    /// low limb.
    #[test]
    fn pow2_of_a_u64_asserts_its_high_limb_is_zero() {
        assert_emits(
            &[Type::U64],
            |emitter| emitter.pow2(SourceSpan::UNKNOWN),
            expect![[r#"
            swap.1
            assertz.err="u64 exponent for pow2 must fit in u32"
            pow2
            u32split
        "#]],
        );
    }

    /// The leading zeros of a `u128` are counted by the core library, which takes the value with
    /// its least significant limb on top.
    #[test]
    fn clz_of_a_u128_counts_from_its_most_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.clz(SourceSpan::UNKNOWN),
            expect![[r#"
            push.1093736776208885424 emit drop
            exec.::miden::core::math::u128::clz
            push.6229491882474008289 emit drop
        "#]],
        );
    }

    /// The leading ones of a `u128` are counted by the core library.
    #[test]
    fn clo_of_a_u128_counts_from_its_most_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.clo(SourceSpan::UNKNOWN),
            expect![[r#"
            push.1093736776208885424 emit drop
            exec.::miden::core::math::u128::clo
            push.6229491882474008289 emit drop
        "#]],
        );
    }

    /// The trailing zeros of a `u128` are counted by the core library.
    #[test]
    fn ctz_of_a_u128_counts_from_its_least_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.ctz(SourceSpan::UNKNOWN),
            expect![[r#"
            push.1093736776208885424 emit drop
            exec.::miden::core::math::u128::ctz
            push.6229491882474008289 emit drop
        "#]],
        );
    }

    /// The trailing ones of a `u128` are counted by the core library.
    #[test]
    fn cto_of_a_u128_counts_from_its_least_significant_limb() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cto(SourceSpan::UNKNOWN),
            expect![[r#"
            push.1093736776208885424 emit drop
            exec.::miden::core::math::u128::cto
            push.6229491882474008289 emit drop
        "#]],
        );
    }

    /// `ilog2` of a `u128` is `127 - clz`, and traps on zero.
    #[test]
    fn ilog2_of_a_u128_subtracts_its_leading_zeros_from_127() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.ilog2(SourceSpan::UNKNOWN),
            expect![[r#"
            push.1093736776208885424 emit drop
            exec.::miden::core::math::u128::clz
            push.6229491882474008289 emit drop
            push.128
            swap.1
            sub
            u32overflowing_sub.1
            assertz.err="ilog2 is undefined for zero"
        "#]],
        );
    }

    /// Casting a felt to `u32` splits it and asserts that the high limb, below the low one, is
    /// zero: `[a] => [a_lo]`.
    #[test]
    fn a_felt_cast_to_u32_asserts_its_high_limb_is_zero() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::U32, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                swap.1
                assertz.err="felt value does not fit in 32 bits"
            "#]],
        );
    }

    /// Casting a felt to `u8` casts it to `u32`, then checks the `u32` is below `2^8`.
    #[test]
    fn a_felt_cast_to_u8_checks_its_low_limb_is_below_2_to_the_8() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::U8, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                swap.1
                assertz.err="felt value does not fit in 32 bits"
                dup.0
                push.4294967040
                u32and
                assertz.err="value does not fit in unsigned 8-bit range"
            "#]],
        );
    }

    /// Casting a felt to `i32` casts it to `u32`, then asserts its sign bit is clear.
    #[test]
    fn a_felt_cast_to_i32_asserts_its_low_limb_is_non_negative() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::I32, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                swap.1
                assertz.err="felt value does not fit in 32 bits"
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i32 value"
            "#]],
        );
    }

    /// Casting a felt to `i16` casts it to `u32`, then checks the `u32` is below `2^15`.
    #[test]
    fn a_felt_cast_to_i16_checks_its_low_limb_is_below_2_to_the_15() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::I16, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                swap.1
                assertz.err="felt value does not fit in 32 bits"
                dup.0
                push.4294901760
                u32and
                assertz.err="value does not fit in unsigned 16-bit range"
                dup.0
                push.32768
                u32and
                eq.32768
                assertz.err="16-bit integer signedness check failed"
            "#]],
        );
    }

    /// Casting a felt to `i64` splits it and asserts bit 63, the sign bit of the high limb, is
    /// clear.
    #[test]
    fn a_felt_cast_to_i64_asserts_bit_63_is_clear() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.cast(&Type::I64, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
                dup.1
                push.2147483648
                u32and
                assertz.err="felt value does not fit in i64"
            "#]],
        );
    }

    /// Sign-extending a felt to `i64` splits it without a check: only `cast` checks.
    #[test]
    fn a_felt_sign_extended_to_i64_is_split_unchecked() {
        assert_emits(
            &[Type::Felt],
            |emitter| emitter.sext(&Type::I64, SourceSpan::UNKNOWN),
            expect![[r#"
                u32split
            "#]],
        );
    }

    /// Casting an `i32` to `i16` checks that bits 31 to 15 are all equal, then keeps the low 16
    /// bits: `-1` is `0x0000FFFF`.
    #[test]
    fn an_i32_cast_to_i16_checks_the_sign_bits_then_masks_to_16_bits() {
        assert_emits(
            &[Type::I32],
            |emitter| emitter.cast(&Type::I16, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                push.0
                push.4294934528
                movup.2
                cdrop
                dup.1
                push.4294934528
                u32and
                assert_eq.err="value does not fit in signed 16-bit range"
                push.65535
                u32and
            "#]],
        );
    }

    /// Casting an `i64` to `i16` checks the value is in range, then keeps the low 16 bits of the
    /// low limb.
    #[test]
    fn an_i64_cast_to_i16_masks_its_low_limb_to_16_bits() {
        assert_emits(
            &[Type::I64],
            |emitter| emitter.cast(&Type::I16, SourceSpan::UNKNOWN),
            expect![[r#"
                swap.1
                dup.0
                eq.0
                dup.0
                movdn.2
                push.4294967295
                push.0
                movup.2
                cdrop
                assert_eq.err="i64 value does not fit in signed 16-bit range"
                dup.1
                push.4294934528
                u32and
                push.4294934528
                push.0
                movup.3
                cdrop
                assert_eq.err="i64 value does not fit in signed 16-bit range"
                push.65535
                u32and
            "#]],
        );
    }

    /// Casting an `i16` to `i8` sign-extends its 16-bit pattern to 32 bits, then narrows it as an
    /// `i32`.
    #[test]
    fn an_i16_cast_to_i8_sign_extends_its_pattern_then_narrows_it() {
        assert_emits(
            &[Type::I16],
            |emitter| emitter.cast(&Type::I8, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.32768
                u32and
                eq.32768
                push.0
                push.4294901760
                movup.2
                cdrop
                u32or
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                push.0
                push.4294967168
                movup.2
                cdrop
                dup.1
                push.4294967168
                u32and
                assert_eq.err="value does not fit in signed 8-bit range"
                push.255
                u32and
            "#]],
        );
    }

    /// Casting an `i16` to `u8` checks its 16-bit pattern is below `2^8`: a negative `i16` has bit
    /// 15 set.
    #[test]
    fn an_i16_cast_to_u8_checks_its_pattern_is_below_2_to_the_8() {
        assert_emits(
            &[Type::I16],
            |emitter| emitter.cast(&Type::U8, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.4294967040
                u32and
                assertz.err="value does not fit in unsigned 8-bit range"
            "#]],
        );
    }

    /// Casting a `u32` to `i16` checks the value is below `2^15`.
    #[test]
    fn a_u32_cast_to_i16_checks_it_is_below_2_to_the_15() {
        assert_emits(
            &[Type::U32],
            |emitter| emitter.cast(&Type::I16, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.4294901760
                u32and
                assertz.err="value does not fit in unsigned 16-bit range"
                dup.0
                push.32768
                u32and
                eq.32768
                assertz.err="16-bit integer signedness check failed"
            "#]],
        );
    }

    /// Casting a `u128` to `i16` casts it to `u32`, then checks the `u32` is below `2^15`.
    #[test]
    fn a_u128_cast_to_i16_checks_it_is_below_2_to_the_15() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.cast(&Type::I16, SourceSpan::UNKNOWN),
            expect![[r#"
                movdn.3
                assertz.err="128-bit value does not fit in u32"
                assertz.err="128-bit value does not fit in u32"
                assertz.err="128-bit value does not fit in u32"
                dup.0
                push.4294901760
                u32and
                assertz.err="value does not fit in unsigned 16-bit range"
                dup.0
                push.32768
                u32and
                eq.32768
                assertz.err="16-bit integer signedness check failed"
            "#]],
        );
    }

    /// Casting a `u32` to `i32` checks the value is at most `i32::MAX`.
    #[test]
    fn a_u32_cast_to_i32_checks_it_is_at_most_i32_max() {
        assert_emits(
            &[Type::U32],
            |emitter| emitter.cast(&Type::I32, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.2147483647
                u32lte
                assert.err="value does not fit in i32"
            "#]],
        );
    }

    /// Casting an `i32` to `u32` checks the value is non-negative, that is at most `i32::MAX` as a
    /// `u32`.
    #[test]
    fn an_i32_cast_to_u32_checks_it_is_non_negative() {
        assert_emits(
            &[Type::I32],
            |emitter| emitter.cast(&Type::U32, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i32 value"
            "#]],
        );
    }

    /// Casting an `i32` to `u64` checks the value is non-negative, then places a zero high limb
    /// below it: `[x] => [x, 0]`.
    #[test]
    fn an_i32_cast_to_u64_places_the_zero_high_limb_below_it() {
        assert_emits(
            &[Type::I32],
            |emitter| emitter.cast(&Type::U64, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i32 value"
                push.0
                swap.1
            "#]],
        );
    }

    /// Casting a `u64` to `i64` checks the value is at most `i64::MAX`.
    #[test]
    fn a_u64_cast_to_i64_checks_it_is_at_most_i64_max() {
        assert_emits(
            &[Type::U64],
            |emitter| emitter.cast(&Type::I64, SourceSpan::UNKNOWN),
            expect![[r#"
                dup.1
                dup.1
                push.2147483647
                push.4294967295
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u64::lte
                push.6229491882474008289 emit drop
                assert.err="value does not fit in i64"
            "#]],
        );
    }

    /// Casting an `i64` to `u64` asserts the sign bit of the high limb is clear, and says so for an
    /// `i64`.
    #[test]
    fn an_i64_cast_to_u64_asserts_the_high_limb_is_non_negative() {
        assert_emits(
            &[Type::I64],
            |emitter| emitter.cast(&Type::U64, SourceSpan::UNKNOWN),
            expect![[r#"
                swap.1
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i64 value"
                swap.1
            "#]],
        );
    }

    /// Casting an `i64` to a felt asserts the value is non-negative; a non-negative `i64` is below
    /// the modulus, so the limbs are then combined.
    #[test]
    fn an_i64_cast_to_felt_asserts_it_is_non_negative() {
        assert_emits(
            &[Type::I64],
            |emitter| emitter.cast(&Type::Felt, SourceSpan::UNKNOWN),
            expect![[r#"
                swap.1
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i64 value"
                mul.4294967296
                add
            "#]],
        );
    }

    /// Casting an `i64` to `u128` asserts the value is non-negative, then zero-extends it.
    #[test]
    fn an_i64_cast_to_u128_asserts_it_is_non_negative() {
        assert_emits(
            &[Type::I64],
            |emitter| emitter.cast(&Type::U128, SourceSpan::UNKNOWN),
            expect![[r#"
                swap.1
                dup.0
                push.2147483648
                u32and
                eq.2147483648
                assertz.err="expected a non-negative i64 value"
                swap.1
                push.0
                push.0
                movup.2
                movup.3
                swap.1
            "#]],
        );
    }

    /// `hir.assert` of a 64-bit value checks the low limb, on top, is 1 and the high limb is 0.
    #[test]
    fn assert_of_a_u64_checks_its_low_limb_is_one() {
        assert_emits(
            &[Type::U64],
            |emitter| emitter.assert(None, None, SourceSpan::UNKNOWN),
            expect![[r#"
                assert.err="expected u64 value to equal 1"
                assertz.err="expected u64 value to equal 1"
            "#]],
        );
    }

    /// `hir.assert` of a 128-bit value compares it with the word `[1, 0, 0, 0]`, the least
    /// significant limb on top.
    #[test]
    fn assert_of_a_u128_compares_it_with_one() {
        assert_emits(
            &[Type::U128],
            |emitter| emitter.assert(None, None, SourceSpan::UNKNOWN),
            expect![[r#"
                push.[1,0,0,0]
                assert_eqw.err="expected u128 value to equal 1"
            "#]],
        );
    }

    /// Asserting a 64-bit value equals a constant compares the low limb, on top, with the low half
    /// of the constant first.
    #[test]
    fn assert_eq_imm_of_a_u64_compares_the_low_limb_first() {
        assert_emits(
            &[Type::U64],
            |emitter| {
                emitter.assert_eq_imm(Immediate::U64(0x0000_0001_0000_0002), SourceSpan::UNKNOWN)
            },
            expect![[r#"
                eq.2
                assert.err="expected u64 value to equal 4294967298"
                eq.1
                assert.err="expected u64 value to equal 4294967298"
            "#]],
        );
    }

    /// Zero-extending a small unsigned integer to `u64` places the zero high limb below it.
    #[test]
    fn a_small_unsigned_integer_zero_extended_to_u64_stays_on_top() {
        assert_emits(
            &[],
            |emitter| emitter.uint_to_u64(8, SourceSpan::UNKNOWN),
            expect![[r#"
            push.0
            swap.1
        "#]],
        );
    }

    /// Converting a small signed integer to `u64` asserts it is non-negative, then places the zero
    /// high limb below it.
    #[test]
    fn a_small_signed_integer_converted_to_u64_stays_on_top() {
        assert_emits(
            &[],
            |emitter| emitter.int_to_u64(8, SourceSpan::UNKNOWN),
            expect![[r#"
            dup.0
            push.128
            u32and
            eq.128
            assertz.err="8-bit integer signedness check failed"
            push.0
            swap.1
        "#]],
        );
    }

    /// Zero-extending a small unsigned integer to `i128` places the three zero limbs below it.
    #[test]
    fn a_small_unsigned_integer_zero_extended_to_i128_stays_on_top() {
        assert_emits(
            &[],
            |emitter| emitter.uint_to_i128(8, SourceSpan::UNKNOWN),
            expect![[r#"
                push.0
                push.0
                push.0
                movup.3
            "#]],
        );
    }

    /// Wrapping `u128` addition calls the core library, as the `i128` one does.
    #[test]
    fn a_wrapping_u128_add_calls_the_core_library() {
        assert_emits(
            &[Type::U128, Type::U128],
            |emitter| emitter.add(Overflow::Wrapping, SourceSpan::UNKNOWN),
            expect![[r#"
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u128::wrapping_add
                push.6229491882474008289 emit drop
            "#]],
        );
    }

    /// Wrapping `u128` subtraction calls the core library, as the `i128` one does.
    #[test]
    fn a_wrapping_u128_sub_calls_the_core_library() {
        assert_emits(
            &[Type::U128, Type::U128],
            |emitter| emitter.sub(Overflow::Wrapping, SourceSpan::UNKNOWN),
            expect![[r#"
                push.1093736776208885424 emit drop
                exec.::miden::core::math::u128::wrapping_sub
                push.6229491882474008289 emit drop
            "#]],
        );
    }

    /// An 8-bit store to the first byte of an element masks the byte out of the element, and ors
    /// the value into it: `[v] => []`, with nothing left behind.
    #[test]
    fn an_8_bit_store_to_a_constant_address_ors_the_value_into_its_element() {
        assert_emits(
            &[Type::U8],
            |emitter| emitter.store_imm(128, SourceSpan::UNKNOWN),
            expect![[r#"
            mem_load.32
            push.4294967040
            u32and
            swap.1
            u32or
            mem_store.32
        "#]],
        );
    }

    /// An 8-bit store to the second byte of an element shifts the value into place first.
    #[test]
    fn an_8_bit_store_to_a_constant_byte_offset_shifts_the_value_into_place() {
        assert_emits(
            &[Type::U8],
            |emitter| emitter.store_imm(129, SourceSpan::UNKNOWN),
            expect![[r#"
            mem_load.32
            push.4294902015
            u32and
            swap.1
            u32shl.8
            u32or
            mem_store.32
        "#]],
        );
    }

    /// A 16-bit store to the first byte of an element masks the low half out of the element.
    #[test]
    fn a_16_bit_store_to_a_constant_aligned_address_ors_the_value_into_its_element() {
        assert_emits(
            &[Type::U16],
            |emitter| emitter.store_imm(128, SourceSpan::UNKNOWN),
            expect![[r#"
            mem_load.32
            push.4294901760
            u32and
            swap.1
            u32or
            mem_store.32
        "#]],
        );
    }

    /// A small store to address 0, which is aligned to every power of two, compiles.
    #[test]
    fn a_small_store_to_address_zero_compiles() {
        assert_emits(
            &[Type::I1],
            |emitter| emitter.store_imm(0, SourceSpan::UNKNOWN),
            expect![[r#"
            mem_load.0
            push.4294967294
            u32and
            swap.1
            u32or
            mem_store.0
        "#]],
        );
    }

    /// A 16-bit load from a constant address that is not element-aligned passes the pointer to
    /// `load_u16` as `push_native_ptr` does, the element address on top:
    ///
    /// - `push.2`, the byte offset `130 % 4`, pushed first: `[2]`
    /// - `push.32`, the element address `130 / 4`: `[32, 2]`, that is `[addr, offset]`
    #[test]
    fn an_unaligned_16_bit_load_from_a_constant_address_passes_the_element_address_on_top() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(130, Type::U16, SourceSpan::UNKNOWN),
            expect![[r#"
                push.2
                push.32
                push.1093736776208885424 emit drop
                exec.::intrinsics::mem::load_u16
                push.6229491882474008289 emit drop
            "#]],
        );
    }

    /// An 8-bit load from a constant address that is not element-aligned passes the pointer as
    /// `[addr, offset]`, as the 16-bit one does.
    #[test]
    fn an_unaligned_8_bit_load_from_a_constant_address_passes_the_element_address_on_top() {
        assert_emits(
            &[],
            |emitter| emitter.load_imm(129, Type::U8, SourceSpan::UNKNOWN),
            expect![[r#"
                push.1
                push.32
                swap.1
                dup.1
                mem_load
                swap.1
                push.8
                u32wrapping_mul
                u32shr
                swap.1
                drop
                push.255
                u32and
            "#]],
        );
    }
}
