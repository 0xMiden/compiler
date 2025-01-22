use smallvec::smallvec;

use super::*;
use crate::{
    diagnostics::{SourceSpan, Span},
    CallConv, Felt, FunctionIdent, Inst, InstBuilder, Instruction, Overflow, Value,
};

/// Used to construct an [InlineAsm] instruction, while checking the input/output types,
/// and enforcing various safety invariants.
pub struct MasmBuilder<B> {
    /// The [InstBuilderBase] which we are building from
    builder: B,
    /// The span of the resulting inline assembly block
    span: SourceSpan,
    /// The inline assembly block we're building
    asm: InlineAsm,
    /// The current code block in the inline assembly that the builder is inserting into
    current_block: MasmBlockId,
    /// The emulated operand stack, primarily used to track the number of stack elements
    /// upon entry and exit from the inline assembly block.
    ///
    /// The only `Type` which is represented on this stack is `Type::Felt`, since we're only
    /// interested in the number of stack elements at any given point. In the future, we may
    /// choose to do additional type checking.
    ///
    /// Upon exit from the inline assembly block, the state of the stack must have enough elements
    /// to store a value of the expected result type, represented by `ty`. Whether those elements
    /// actually store a valid value of that type is beyond the scope of this builder, for now.
    stack: OperandStack<Type>,
}
impl<'f, B: InstBuilder<'f>> MasmBuilder<B> {
    /// Construct a new inline assembly builder in the function represented by `dfg`, to be inserted
    /// at `ip`.
    ///
    /// The `args` list represents the arguments which will be visible on the operand stack in this
    /// inline assembly block.
    ///
    /// The `results` set represents the types that are expected to be found on the operand stack
    /// when the inline assembly block finishes executing. Use an empty set to represent no
    /// results.
    ///
    /// Any attempt to modify the operand stack beyond what is made visible via arguments, or
    /// introduced within the inline assembly block, will cause an assertion to fail.
    pub fn new(mut builder: B, args: &[Value], results: Vec<Type>, span: SourceSpan) -> Self {
        // Construct the initial operand stack with the given arguments
        let mut stack = OperandStack::<Type>::default();
        {
            let dfg = builder.data_flow_graph();
            for arg in args.iter().rev().copied() {
                stack.push(dfg.value_type(arg).clone());
            }
        }

        // Construct an empty inline assembly block with the given arguments
        let mut asm = InlineAsm::new(results);
        {
            let dfg = builder.data_flow_graph_mut();
            let mut vlist = ValueList::default();
            vlist.extend(args.iter().copied(), &mut dfg.value_lists);
            asm.args = vlist;
        }

        let current_block = asm.body;
        Self {
            builder,
            span,
            asm,
            current_block,
            stack,
        }
    }

    /// Create a new, empty MASM code block, for use with control flow instructions
    #[inline]
    pub fn create_block(&mut self) -> MasmBlockId {
        self.asm.create_block()
    }

    /// Change the insertion point of the builder to the end of `block`
    #[inline(always)]
    pub fn switch_to_block(&mut self, block: MasmBlockId) {
        self.current_block = block;
    }

    /// Get a builder for a single MASM instruction
    pub fn ins<'a, 'b: 'a>(&'b mut self) -> MasmOpBuilder<'a> {
        MasmOpBuilder {
            dfg: self.builder.data_flow_graph_mut(),
            asm: &mut self.asm,
            stack: &mut self.stack,
            ip: self.current_block,
        }
    }

    /// Finalize this inline assembly block, inserting it into the `Function` from which this
    /// builder was derived.
    ///
    /// Returns the [Inst] which corresponds to the inline assembly instruction, and the inner
    /// [DataFlowGraph] reference held by the underlying [InstBuilderBase].
    pub fn build(self) -> Inst {
        if self.asm.results.is_empty() {
            assert!(
                self.stack.is_empty(),
                "invalid inline assembly: expected operand stack to be empty upon exit, found: \
                 {:?}",
                self.stack.debug()
            );
        } else {
            let mut len = 0;
            for ty in self.asm.results.iter() {
                len += ty.size_in_felts();
            }
            assert_eq!(
                len,
                self.stack.len(),
                "invalid inline assembly: needed {} elements on the operand stack, found: {:?}",
                len,
                self.stack.debug()
            );
        }

        let span = self.span;
        let data = Instruction::InlineAsm(self.asm);
        self.builder.build(data, Type::Unit, span).0
    }
}

/// Used to construct a single MASM opcode
pub struct MasmOpBuilder<'a> {
    dfg: &'a mut DataFlowGraph,
    /// The inline assembly block being created
    asm: &'a mut InlineAsm,
    /// The state of the operand stack at this point in the program
    stack: &'a mut OperandStack<Type>,
    /// The block to which this builder should append the instruction it builds
    ip: MasmBlockId,
}
impl<'a> MasmOpBuilder<'a> {
    /// Pads the stack with four zero elements
    pub fn padw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Padw, span);
    }

    /// Pushes an element on the stack
    pub fn push(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::Push(imm), span);
    }

    /// Pushes a word on the stack
    pub fn pushw(mut self, word: [Felt; 4], span: SourceSpan) {
        self.build(self.ip, MasmOp::Pushw(word), span);
    }

    /// Pushes an element representing an unsigned 8-bit integer on the stack
    pub fn push_u8(mut self, imm: u8, span: SourceSpan) {
        self.build(self.ip, MasmOp::PushU8(imm), span);
    }

    /// Pushes an element representing an unsigned 16-bit integer on the stack
    pub fn push_u16(mut self, imm: u16, span: SourceSpan) {
        self.build(self.ip, MasmOp::PushU16(imm), span);
    }

    /// Pushes an element representing an unsigned 32-bit integer on the stack
    pub fn push_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::PushU32(imm), span);
    }

    /// Drops the element on the top of the stack
    pub fn drop(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Drop, span);
    }

    /// Drops the word (first four elements) on the top of the stack
    pub fn dropw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Dropw, span);
    }

    /// Duplicates the `n`th element from the top of the stack, to the top of the stack
    ///
    /// A `n` of zero, duplicates the element on top of the stack
    ///
    /// The valid range for `n` is 0..=15
    pub fn dup(mut self, n: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Dup(n as u8), span);
    }

    /// Duplicates the `n`th word from the top of the stack, to the top of the stack
    ///
    /// A `n` of zero, duplicates the word on top of the stack
    ///
    /// The valid range for `n` is 0..=3
    pub fn dupw(mut self, n: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Dupw(n as u8), span);
    }

    /// Swaps the `n`th element and the element on top of the stack
    ///
    /// The valid range for `n` is 1..=15
    pub fn swap(mut self, n: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Swap(n as u8), span);
    }

    /// Swaps the `n`th word and the word on top of the stack
    ///
    /// The valid range for `n` is 1..=3
    pub fn swapw(mut self, n: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Swapw(n as u8), span);
    }

    /// Swaps the top 2 and bottom 2 words on the stack
    pub fn swapdw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Swapdw, span);
    }

    /// Moves the `n`th element to the top of the stack
    ///
    /// The valid range for `n` is 2..=15
    pub fn movup(mut self, idx: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Movup(idx as u8), span);
    }

    /// Moves the `n`th word to the top of the stack
    ///
    /// The valid range for `n` is 2..=3
    pub fn movupw(mut self, idx: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Movupw(idx as u8), span);
    }

    /// Moves the element on top of the stack, making it the `n`th element
    ///
    /// The valid range for `n` is 2..=15
    pub fn movdn(mut self, idx: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Movdn(idx as u8), span);
    }

    /// Moves the word on top of the stack, making it the `n`th word
    ///
    /// The valid range for `n` is 2..=3
    pub fn movdnw(mut self, idx: usize, span: SourceSpan) {
        self.build(self.ip, MasmOp::Movdnw(idx as u8), span);
    }

    /// Pops a boolean element off the stack, and swaps the top two elements
    /// on the stack if that boolean is true.
    ///
    /// Traps if the conditional is not 0 or 1.
    pub fn cswap(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Cswap, span);
    }

    /// Pops a boolean element off the stack, and swaps the top two words
    /// on the stack if that boolean is true.
    ///
    /// Traps if the conditional is not 0 or 1.
    pub fn cswapw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Cswapw, span);
    }

    /// Pops a boolean element off the stack, and drops the top element on the
    /// stack if the boolean is true, otherwise it drops the next element down.
    ///
    /// Traps if the conditional is not 0 or 1.
    pub fn cdrop(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Cdrop, span);
    }

    /// Pops a boolean element off the stack, and drops the top word on the
    /// stack if the boolean is true, otherwise it drops the next word down.
    ///
    /// Traps if the conditional is not 0 or 1.
    pub fn cdropw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Cdropw, span);
    }

    /// Pops the top element on the stack, and traps if that element is != 1.
    pub fn assert(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::AssertWithError).unwrap_or(MasmOp::Assert);
        self.build(self.ip, op, span);
    }

    /// Pops the top element on the stack, and traps if that element is != 0.
    pub fn assertz(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::AssertzWithError).unwrap_or(MasmOp::Assertz);
        self.build(self.ip, op, span);
    }

    /// Pops the top two elements on the stack, and traps if they are not equal.
    pub fn assert_eq(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::AssertEqWithError).unwrap_or(MasmOp::AssertEq);
        self.build(self.ip, op, span);
    }

    /// Pops the top two words on the stack, and traps if they are not equal.
    pub fn assert_eqw(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::AssertEqwWithError).unwrap_or(MasmOp::AssertEqw);
        self.build(self.ip, op, span);
    }

    /// Pops an element containing a memory address from the top of the stack,
    /// and loads the first element of the word at that address to the top of the stack.
    pub fn load(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemLoad, span);
    }

    /// Loads the first element of the word at the given address to the top of the stack.
    pub fn load_imm(mut self, addr: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemLoadImm(addr), span);
    }

    /// Pops an element containing a memory address from the top of the stack,
    /// and loads the word at that address to the top of the stack.
    pub fn loadw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemLoadw, span);
    }

    /// Loads the word at the given address to the top of the stack.
    pub fn loadw_imm(mut self, addr: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemLoadwImm(addr), span);
    }

    /// Pops two elements, the first containing a memory address from the top of the stack,
    /// the second the value to be stored as the first element of the word at that address.
    pub fn store(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemStore, span);
    }

    /// Pops an element from the top of the stack, and stores it as the first element of
    /// the word at the given address.
    pub fn store_imm(mut self, addr: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemStoreImm(addr), span);
    }

    /// Pops an element containing a memory address from the top of the stack,
    /// and then pops a word from the stack and stores it as the word at that address.
    pub fn storew(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemStorew, span);
    }

    /// Pops a word from the stack and stores it as the word at the given address.
    pub fn storew_imm(mut self, addr: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::MemStorewImm(addr), span);
    }

    /// Begins construction of a `if.true` statement.
    ///
    /// An `if.true` pops a boolean value off the stack, and uses it to choose between
    /// one of two branches. The "then" branch is taken if the conditional is true,
    /// the "else" branch otherwise.
    ///
    /// NOTE: This function will panic if the top of the operand stack is not of boolean type
    /// when called.
    ///
    /// You must ensure that both branches of the `if.true` statement leave the operand stack
    /// in the same abstract state, so that when control resumes after the `if.true`, the remaining
    /// program is well-formed. This will be validated automatically for you, but if validation
    /// fails, the builder will panic.
    pub fn if_true(self, span: SourceSpan) -> IfTrueBuilder<'a> {
        let cond = self.stack.pop().expect("operand stack is empty");
        assert_eq!(cond, Type::I1, "expected while.true condition to be a boolean value");
        let out_stack = self.stack.clone();
        IfTrueBuilder {
            dfg: self.dfg,
            asm: self.asm,
            in_stack: self.stack,
            out_stack,
            span,
            ip: self.ip,
            then_blk: None,
            else_blk: None,
        }
    }

    /// Begins construction of a `while.true` loop.
    ///
    /// A `while.true` pops a boolean value off the stack to use as the condition for
    /// entering the loop, and will then execute the loop body for as long as the value
    /// on top of the stack is a boolean and true. If the condition is not a boolean,
    /// execution traps.
    ///
    /// NOTE: This function will panic if the top of the operand stack is not of boolean type
    /// when called.
    ///
    /// Before finalizing construction of the loop body, you must ensure two things:
    ///
    /// 1. There is a value of boolean type on top of the operand stack
    /// 2. The abstract state of the operand stack, assuming the boolean just mentioned
    ///    has been popped, must be consistent with the state of the operand stack when the
    ///    loop was entered, as well as if the loop was skipped due to the conditional being
    ///    false. The abstract state referred to here is the number, and type, of the elements
    ///    on the operand stack.
    ///
    /// Both of these are validated by [LoopBuilder], and a panic is raised if validation fails.
    pub fn while_true(self, span: SourceSpan) -> LoopBuilder<'a> {
        let cond = self.stack.pop().expect("operand stack is empty");
        assert_eq!(cond, Type::I1, "expected while.true condition to be a boolean value");
        let out_stack = self.stack.clone();
        let body = self.asm.create_block();
        LoopBuilder {
            dfg: self.dfg,
            asm: self.asm,
            in_stack: self.stack,
            out_stack,
            span,
            ip: self.ip,
            body,
            style: LoopType::While,
        }
    }

    /// Begins construction of a `repeat` loop, with an iteration count of `n`.
    ///
    /// A `repeat` instruction requires no operands on the stack, and will execute the loop body `n`
    /// times.
    ///
    /// NOTE: The iteration count must be non-zero, or this function will panic.
    pub fn repeat(self, n: u16, span: SourceSpan) -> LoopBuilder<'a> {
        assert!(n > 0, "invalid iteration count for `repeat.n`, must be non-zero");
        let out_stack = self.stack.clone();
        let body = self.asm.create_block();
        LoopBuilder {
            dfg: self.dfg,
            asm: self.asm,
            in_stack: self.stack,
            out_stack,
            span,
            ip: self.ip,
            body,
            style: LoopType::Repeat(n),
        }
    }

    /// Executes the named procedure as a regular function.
    pub fn exec(mut self, id: FunctionIdent, span: SourceSpan) {
        self.build(self.ip, MasmOp::Exec(id), span);
    }

    /// Execute a procedure indirectly.
    ///
    /// Expects the hash of a function's MAST root on the stack, see `procref`
    pub fn dynexec(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::DynExec, span)
    }

    /// Call a procedure indirectly.
    ///
    /// Expects the hash of a function's MAST root on the stack, see `procref`
    pub fn dyncall(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::DynCall, span)
    }

    /// Executes the named procedure as a call.
    pub fn call(mut self, id: FunctionIdent, span: SourceSpan) {
        self.build(self.ip, MasmOp::Call(id), span);
    }

    /// Executes the named procedure as a syscall.
    pub fn syscall(mut self, id: FunctionIdent, span: SourceSpan) {
        self.build(self.ip, MasmOp::Syscall(id), span);
    }

    /// Push the hash of the named function on the stack for use with dyn(exec|call)
    pub fn procref(mut self, id: FunctionIdent, span: SourceSpan) {
        self.build(self.ip, MasmOp::ProcRef(id), span)
    }

    /// Pops two field elements from the stack, adds them, and places the result on the stack.
    pub fn add(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Add, span);
    }

    /// Pops a field element from the stack, adds the given value to it, and places the result on
    /// the stack.
    pub fn add_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::AddImm(imm), span);
    }

    /// Pops two field elements from the stack, subtracts the second from the first, and places the
    /// result on the stack.
    pub fn sub(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Sub, span);
    }

    /// Pops a field element from the stack, subtracts the given value from it, and places the
    /// result on the stack.
    pub fn sub_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::SubImm(imm), span);
    }

    /// Pops two field elements from the stack, multiplies them, and places the result on the stack.
    pub fn mul(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Mul, span);
    }

    /// Pops a field element from the stack, multiplies it by the given value, and places the result
    /// on the stack.
    pub fn mul_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::MulImm(imm), span);
    }

    /// Pops two field elements from the stack, divides the first by the second, and places the
    /// result on the stack.
    pub fn div(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Div, span);
    }

    /// Pops a field element from the stack, divides it by the given value, and places the result on
    /// the stack.
    pub fn div_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::DivImm(imm), span);
    }

    /// Negates the field element on top of the stack
    pub fn neg(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Neg, span);
    }

    /// Replaces the field element on top of the stack with it's multiplicative inverse, i.e. `a^-1
    /// mod p`
    pub fn inv(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Inv, span);
    }

    /// Increments the field element on top of the stack
    pub fn incr(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Incr, span);
    }

    /// Pops an element, `a`, from the top of the stack, and places the integral base 2 logarithm
    /// of that value on the stack.
    ///
    /// Traps if `a` is 0.
    pub fn ilog2(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Ilog2, span);
    }

    /// Pops an element, `a`, from the top of the stack, and places the result of `2^a` on the
    /// stack.
    ///
    /// Traps if `a` is not in the range 0..=63
    pub fn pow2(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Pow2, span);
    }

    /// Pops two elements from the stack, `b` and `a` respectively, and places the result of `a^b`
    /// on the stack.
    ///
    /// Traps if `b` is not in the range 0..=63
    pub fn exp(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Exp, span);
    }

    /// Pops an element from the stack, `a`, and places the result of `a^b` on the stack, where `b`
    /// is the given immediate value.
    ///
    /// Traps if `b` is not in the range 0..=63
    pub fn exp_imm(mut self, exponent: u8, span: SourceSpan) {
        self.build(self.ip, MasmOp::ExpImm(exponent), span);
    }

    /// Pops a value off the stack, and applies logical NOT, and places the result back on the
    /// stack.
    ///
    /// Traps if the value is not 0 or 1.
    pub fn not(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Not, span);
    }

    /// Pops two values off the stack, applies logical AND, and places the result back on the stack.
    ///
    /// Traps if either value is not 0 or 1.
    pub fn and(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::And, span);
    }

    /// Pops a value off the stack, applies logical AND with the given immediate, and places the
    /// result back on the stack.
    ///
    /// Traps if the value is not 0 or 1.
    pub fn and_imm(mut self, imm: bool, span: SourceSpan) {
        self.build(self.ip, MasmOp::AndImm(imm), span);
    }

    /// Pops two values off the stack, applies logical OR, and places the result back on the stack.
    ///
    /// Traps if either value is not 0 or 1.
    pub fn or(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Or, span);
    }

    /// Pops a value off the stack, applies logical OR with the given immediate, and places the
    /// result back on the stack.
    ///
    /// Traps if the value is not 0 or 1.
    pub fn or_imm(mut self, imm: bool, span: SourceSpan) {
        self.build(self.ip, MasmOp::OrImm(imm), span);
    }

    /// Pops two values off the stack, applies logical XOR, and places the result back on the stack.
    ///
    /// Traps if either value is not 0 or 1.
    pub fn xor(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Xor, span);
    }

    /// Pops a value off the stack, applies logical XOR with the given immediate, and places the
    /// result back on the stack.
    ///
    /// Traps if the value is not 0 or 1.
    pub fn xor_imm(mut self, imm: bool, span: SourceSpan) {
        self.build(self.ip, MasmOp::XorImm(imm), span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if they are equal, else 0.
    pub fn eq(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Eq, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value and the given
    /// immediate are equal, else 0.
    pub fn eq_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::EqImm(imm), span);
    }

    /// Pops two words off the stack, and pushes 1 on the stack if they are equal, else 0.
    pub fn eqw(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Eqw, span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if they are not equal, else 0.
    pub fn neq(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Neq, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value and the given
    /// immediate are not equal, else 0.
    pub fn neq_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::NeqImm(imm), span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if the first is greater than the
    /// second, else 0.
    pub fn gt(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Gt, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value is greater than the
    /// given immediate, else 0.
    pub fn gt_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::GtImm(imm), span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if the first is greater than or
    /// equal to the second, else 0.
    pub fn gte(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Gte, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value is greater than or
    /// equal to the given immediate, else 0.
    pub fn gte_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::GteImm(imm), span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if the first is less than the
    /// second, else 0.
    pub fn lt(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Lt, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value is less than the
    /// given immediate, else 0.
    pub fn lt_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::LtImm(imm), span);
    }

    /// Pops two elements off the stack, and pushes 1 on the stack if the first is less than or
    /// equal to the second, else 0.
    pub fn lte(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Lte, span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value is less than or equal
    /// to the given immediate, else 0.
    pub fn lte_imm(mut self, imm: Felt, span: SourceSpan) {
        self.build(self.ip, MasmOp::LteImm(imm), span);
    }

    /// Pops an element off the stack, and pushes 1 on the stack if that value is an odd number,
    /// else 0.
    pub fn is_odd(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::IsOdd, span);
    }

    /// Pushes the current value of the cycle counter (clock) on the stack
    pub fn clk(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Clk, span);
    }

    /// Pushes the hash of the caller on the stack
    pub fn caller(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Caller, span);
    }

    /// Pushes the current stack depth on the stack
    pub fn current_stack_depth(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::Sdepth, span);
    }

    /// Pushes 1 on the stack if the element on top of the stack is less than 2^32, else 0.
    pub fn test_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Test, span);
    }

    /// Pushes 1 on the stack if every element of the word on top of the stack is less than 2^32,
    /// else 0.
    pub fn testw_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Testw, span);
    }

    /// Traps if the element on top of the stack is greater than or equal to 2^32
    pub fn assert_u32(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::U32AssertWithError).unwrap_or(MasmOp::U32Assert);
        self.build(self.ip, op, span);
    }

    /// Traps if either of the first two elements on top of the stack are greater than or equal to
    /// 2^32
    pub fn assert2_u32(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::U32Assert2WithError).unwrap_or(MasmOp::U32Assert2);
        self.build(self.ip, op, span);
    }

    /// Traps if any element of the first word on the stack are greater than or equal to 2^32
    pub fn assertw_u32(mut self, error_code: Option<u32>, span: SourceSpan) {
        let op = error_code.map(MasmOp::U32AssertwWithError).unwrap_or(MasmOp::U32Assertw);
        self.build(self.ip, op, span);
    }

    /// Casts the element on top of the stack, `a`, to a valid u32 value, by computing `a mod 2^32`
    pub fn cast_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Cast, span);
    }

    /// Pops an element, `a`, from the stack, and splits it into two elements, `b` and `c`, each of
    /// which are a valid u32 value.
    ///
    /// The value for `b` is given by `a mod 2^32`, and the value for `c` by `a / 2^32`. They are
    /// pushed on the stack in that order, i.e. `c` will be on top of the stack afterwards.
    pub fn split_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Split, span);
    }

    /// Performs unsigned addition of the top two elements on the stack, `b` and `a` respectively,
    /// which are expected to be valid u32 values.
    ///
    /// See the [Overflow] enum for how `overflow` modifies the semantics of this instruction.
    pub fn add_u32(mut self, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked => MasmOp::Add,
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert2),
                        Span::new(span, MasmOp::Add),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingAdd,
            Overflow::Wrapping => MasmOp::U32WrappingAdd,
        };
        self.build(self.ip, op, span);
    }

    /// Same as above, but `a` is provided by the given immediate.
    pub fn add_imm_u32(mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked if imm == 1 => MasmOp::Incr,
            Overflow::Unchecked => MasmOp::AddImm(Felt::new(imm as u64)),
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert),
                        Span::new(span, MasmOp::AddImm(Felt::new(imm as u64))),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingAddImm(imm),
            Overflow::Wrapping => MasmOp::U32WrappingAddImm(imm),
        };
        self.build(self.ip, op, span);
    }

    /// Pops three elements from the stack, `c`, `b`, and `a`, and computes `a + b + c` using the
    /// overflowing semantics of `add_u32`. The first two elements on the stack after this
    /// instruction will be a boolean indicating whether addition overflowed, and the result
    /// itself, mod 2^32.
    pub fn add3_overflowing_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32OverflowingAdd3, span);
    }

    /// Pops three elements from the stack, `c`, `b`, and `a`, and computes `a + b + c` using the
    /// wrapping semantics of `add_u32`. The result will be on top of the stack afterwards, mod
    /// 2^32.
    pub fn add3_wrapping_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32WrappingAdd3, span);
    }

    /// Performs unsigned subtraction of the top two elements on the stack, `b` and `a`
    /// respectively, which are expected to be valid u32 values.
    ///
    /// See the [Overflow] enum for how `overflow` modifies the semantics of this instruction.
    pub fn sub_u32(mut self, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked => MasmOp::Sub,
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert2),
                        Span::new(span, MasmOp::Sub),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingSub,
            Overflow::Wrapping => MasmOp::U32WrappingSub,
        };
        self.build(self.ip, op, span);
    }

    /// Same as above, but `a` is provided by the given immediate.
    pub fn sub_imm_u32(mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked => MasmOp::SubImm(Felt::new(imm as u64)),
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert),
                        Span::new(span, MasmOp::SubImm(Felt::new(imm as u64))),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingSubImm(imm),
            Overflow::Wrapping => MasmOp::U32WrappingSubImm(imm),
        };
        self.build(self.ip, op, span);
    }

    /// Performs unsigned multiplication of the top two elements on the stack, `b` and `a`
    /// respectively, which are expected to be valid u32 values.
    ///
    /// See the [Overflow] enum for how `overflow` modifies the semantics of this instruction.
    pub fn mul_u32(mut self, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked => MasmOp::Mul,
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert2),
                        Span::new(span, MasmOp::Mul),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingMul,
            Overflow::Wrapping => MasmOp::U32WrappingMul,
        };
        self.build(self.ip, op, span);
    }

    /// Same as above, but `a` is provided by the given immediate.
    pub fn mul_imm_u32(mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        let op = match overflow {
            Overflow::Unchecked => MasmOp::MulImm(Felt::new(imm as u64)),
            Overflow::Checked => {
                return self.build_many(
                    self.ip,
                    [
                        Span::new(span, MasmOp::U32Assert),
                        Span::new(span, MasmOp::MulImm(Felt::new(imm as u64))),
                        Span::new(span, MasmOp::U32Assert),
                    ],
                );
            }
            Overflow::Overflowing => MasmOp::U32OverflowingMulImm(imm),
            Overflow::Wrapping => MasmOp::U32WrappingMulImm(imm),
        };
        self.build(self.ip, op, span);
    }

    /// Pops three elements from the stack, `b`, `a`, and `c`, and computes `a * b + c`, using
    /// overflowing semantics, i.e. the result is wrapped mod 2^32, and a flag is pushed on the
    /// stack if the result overflowed the u32 range.
    pub fn madd_overflowing_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32OverflowingMadd, span);
    }

    /// Pops three elements from the stack, `b`, `a`, and `c`, and computes `a * b + c`, using
    /// wrapping semantics, i.e. the result is wrapped mod 2^32.
    pub fn madd_wrapping_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32WrappingMadd, span);
    }

    /// Performs unsigned division of the top two elements on the stack, `b` and `a` respectively,
    /// which are expected to be valid u32 values.
    ///
    /// This operation is unchecked, so if either operand is >= 2^32, the result is undefined.
    ///
    /// Traps if `b` is 0.
    pub fn div_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Div, span);
    }

    /// Same as above, but `b` is provided by the given immediate
    pub fn div_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32DivImm(imm), span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and computes `a mod b`.
    ///
    /// This operation is unchecked, so if either operand is >= 2^32, the result is undefined.
    ///
    /// Traps if `b` is 0.
    pub fn mod_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Mod, span);
    }

    /// Same as above, but `b` is provided by the given immediate
    pub fn mod_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32ModImm(imm), span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and computes `a / b`, and `a mod
    /// b`, pushing the results of each on the stack in that order.
    ///
    /// This operation is unchecked, so if either operand is >= 2^32, the results are undefined.
    ///
    /// Traps if `b` is 0.
    pub fn divmod_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32DivMod, span);
    }

    /// Same as above, but `b` is provided by the given immediate
    pub fn divmod_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32DivModImm(imm), span);
    }

    /// Pops two elements off the stack, and computes the bitwise AND of those values, placing the
    /// result on the stack.
    ///
    /// Traps if either element is not a valid u32 value.
    pub fn band_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32And, span);
    }

    /// Pops two elements off the stack, and computes the bitwise OR of those values, placing the
    /// result on the stack.
    ///
    /// Traps if either element is not a valid u32 value.
    pub fn bor_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Or, span);
    }

    /// Pops two elements off the stack, and computes the bitwise XOR of those values, placing the
    /// result on the stack.
    ///
    /// Traps if either element is not a valid u32 value.
    pub fn bxor_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Xor, span);
    }

    /// Pops an element off the stack, and computes the bitwise NOT of that value, placing the
    /// result on the stack.
    ///
    /// Traps if the element is not a valid u32 value.
    pub fn bnot_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Not, span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and shifts `a` left by `b` bits.
    /// More precisely, the result is computed as `(a * 2^b) mod 2^32`.
    ///
    /// The result is undefined if `a` is not a valid u32, or `b` is > 31.
    pub fn shl_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Shl, span);
    }

    /// Same as `shl_u32`, but `b` is provided by immediate.
    pub fn shl_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32ShlImm(imm), span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and shifts `a` right by `b` bits.
    /// More precisely, the result is computed as `a / 2^b`.
    ///
    /// The result is undefined if `a` is not a valid u32, or `b` is > 31.
    pub fn shr_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Shr, span);
    }

    /// Same as `shr_u32`, but `b` is provided by immediate.
    pub fn shr_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32ShrImm(imm), span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and rotates the binary
    /// representation of `a` left by `b` bits.
    ///
    /// The result is undefined if `a` is not a valid u32, or `b` is > 31.
    pub fn rotl_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Rotl, span);
    }

    /// Same as `rotl_u32`, but `b` is provided by immediate.
    pub fn rotl_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32RotlImm(imm), span);
    }

    /// Pops two elements off the stack, `b` and `a` respectively, and rotates the binary
    /// representation of `a` right by `b` bits.
    ///
    /// The result is undefined if `a` is not a valid u32, or `b` is > 31.
    pub fn rotr_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Rotr, span);
    }

    /// Same as `rotr_u32`, but `b` is provided by immediate.
    pub fn rotr_imm_u32(mut self, imm: u32, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32RotrImm(imm), span);
    }

    /// Pops an element off the stack, and computes the number of set bits in its binary
    /// representation, i.e. its hamming weight, and places the result on the stack.
    ///
    /// The result is undefined if the input value is not a valid u32.
    pub fn popcnt_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Popcnt, span);
    }

    /// Pops an element off the stack, and computes the number of leading zeros in its binary
    /// representation, and places the result on the stack.
    ///
    /// The result is undefined if the input value is not a valid u32.
    pub fn clz_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Clz, span);
    }

    /// Pops an element off the stack, and computes the number of trailing zeros in its binary
    /// representation, and places the result on the stack.
    ///
    /// The result is undefined if the input value is not a valid u32.
    pub fn ctz_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Ctz, span);
    }

    /// Pops an element off the stack, and computes the number of leading ones in its binary
    /// representation, and places the result on the stack.
    ///
    /// The result is undefined if the input value is not a valid u32.
    pub fn clo_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Clo, span);
    }

    /// Pops an element off the stack, and computes the number of trailing ones in its binary
    /// representation, and places the result on the stack.
    ///
    /// The result is undefined if the input value is not a valid u32.
    pub fn cto_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Cto, span);
    }

    /// Pushes a boolean on the stack by computing `a < b` for `[b, a]`
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn lt_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Lt, span);
    }

    /// Pushes a boolean on the stack by computing `a <= b` for `[b, a]`
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn lte_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Lte, span);
    }

    /// Pushes a boolean on the stack by computing `a > b` for `[b, a]`
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn gt_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Gt, span);
    }

    /// Pushes a boolean on the stack by computing `a >= b` for `[b, a]`
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn gte_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Gte, span);
    }

    /// Computes the minimum of the two elements on top of the stack.
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn min_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Min, span);
    }

    /// Computes the maximum of the two elements on top of the stack.
    ///
    /// The result is undefined if either operand is not a valid u32 value.
    pub fn max_u32(mut self, span: SourceSpan) {
        self.build(self.ip, MasmOp::U32Max, span);
    }

    #[inline(never)]
    fn build(&mut self, ip: MasmBlockId, op: MasmOp, span: SourceSpan) {
        apply_op_stack_effects(&op, self.stack, self.dfg, self.asm);
        self.asm.push(ip, op, span);
    }

    #[inline(never)]
    fn build_many(&mut self, ip: MasmBlockId, ops: impl IntoIterator<Item = Span<MasmOp>>) {
        for op in ops.into_iter() {
            apply_op_stack_effects(&op, self.stack, self.dfg, self.asm);
            self.asm.push(ip, op.into_inner(), op.span());
        }
    }
}

#[doc(hidden)]
enum IfBranch {
    Then,
    Else,
}

/// This builder is used to construct an `if.true` instruction, while maintaining
/// the invariant that the operand stack has a uniform state upon exit from either
/// branch of the `if.true`, i.e. the number of elements, and their types, must
/// match.
///
/// We do this by snapshotting the state of the operand stack on entry, using it
/// when visiting each branch as the initial stack state, and then validating that
/// when both branches have been constructed, that the stack state on exit is the
/// same. The first branch to be completed defines the expected state of the stack
/// for the remaining branch.
///
/// # Example
///
/// The general usage here looks like this, where `masm_builder` is an instance of
/// [MasmBuilder]:
///
/// ```text,ignore
/// // If the current top of the stack is > 0, decrement the next stack element, which
/// is a counter, and then call a function, otherwise, pop the counter, push 0, and proceed.
/// masm_builder.ins().gt_imm(Felt::ZERO);
/// let if_builder = masm_builder.ins().if_true();
///
/// // Build the then branch
/// let then_b = if_builder.build_then();
/// then_b.ins().sub_imm(Felt::new(1 as u64));
/// then_b.ins().exec("do_some_stuff_and_return_a_boolean".parse().unwrap());
/// then_b.end();
///
/// // Build the else branch
/// let else_b = if_builder.build_else();
/// else_b.ins().pop();
/// else_b.ins().push(Felt::ZERO);
/// else_b.end();
///
/// // Finalize
/// if_builder.build();
/// ```
pub struct IfTrueBuilder<'a> {
    dfg: &'a mut DataFlowGraph,
    asm: &'a mut InlineAsm,
    /// This reference is to the operand stack in the parent [MasmOpBuilder],
    /// which represents the operand stack on entry to the `if.true`. Upon
    /// finalizatio of the `if.true`, we use update this operand stack to
    /// reflect the state upon exit from the `if.true`.
    ///
    /// In effect, when the call to `if_true` returns, the operand stack in the
    /// parent builder will look as if the `if.true` instruction has finished executing.
    in_stack: &'a mut OperandStack<Type>,
    /// This is set when the first branch is finished being constructed, and
    /// will be used as the expected state of the operand stack when we finish
    /// constructing the second branch and validate the `if.true`.
    out_stack: OperandStack<Type>,
    /// The source span associated with the conditional
    span: SourceSpan,
    /// This is the block to which the `if.true` will be appended
    ip: MasmBlockId,
    /// The block id for the then branch, unset until it has been finalized
    then_blk: Option<MasmBlockId>,
    /// The block id for the else branch, unset until it has been finalized
    else_blk: Option<MasmBlockId>,
}
impl<'f> IfTrueBuilder<'f> {
    /// Start constructing the then block for this `if.true` instruction
    ///
    /// NOTE: This function will panic if the then block has already been built
    pub fn build_then<'a: 'f, 'b: 'f + 'a>(&'b mut self) -> IfTrueBlockBuilder<'a> {
        assert!(self.then_blk.is_none(), "cannot build the 'then' branch twice");
        let then_blk = self.asm.create_block();
        let stack = self.in_stack.clone();
        IfTrueBlockBuilder {
            builder: self,
            stack,
            block: then_blk,
            branch: IfBranch::Then,
        }
    }

    /// Start constructing the else block for this `if.true` instruction
    ///
    /// NOTE: This function will panic if the else block has already been built
    pub fn build_else<'a: 'f, 'b: 'f + 'a>(&'b mut self) -> IfTrueBlockBuilder<'a> {
        assert!(self.else_blk.is_none(), "cannot build the 'else' branch twice");
        let else_blk = self.asm.create_block();
        let stack = self.in_stack.clone();
        IfTrueBlockBuilder {
            builder: self,
            stack,
            block: else_blk,
            branch: IfBranch::Else,
        }
    }

    /// Finalize this `if.true` instruction, inserting it into the block this
    /// builder was constructed from.
    pub fn build(mut self) {
        let then_blk = self.then_blk.expect("missing 'then' block");
        let else_blk = self.else_blk.expect("missing 'else' block");
        self.asm.push(self.ip, MasmOp::If(then_blk, else_blk), self.span);
        // Update the operand stack to represent the state after execution of the `if.true`
        let in_stack = self.in_stack.stack_mut();
        in_stack.clear();
        in_stack.append(self.out_stack.stack_mut());
    }
}

/// Used to construct a single branch of an `if.true` instruction
///
/// See [IfTrueBuilder] for usage.
pub struct IfTrueBlockBuilder<'a> {
    builder: &'a mut IfTrueBuilder<'a>,
    // The state of the operand stack in this block
    stack: OperandStack<Type>,
    // The block we're building
    block: MasmBlockId,
    branch: IfBranch,
}
impl IfTrueBlockBuilder<'_> {
    /// Construct a MASM instruction in this block
    pub fn ins<'a, 'b: 'a>(&'b mut self) -> MasmOpBuilder<'a> {
        MasmOpBuilder {
            dfg: self.builder.dfg,
            asm: self.builder.asm,
            stack: &mut self.stack,
            ip: self.block,
        }
    }

    /// Finalize this block, and release the builder
    pub fn end(self) {}
}
impl Drop for IfTrueBlockBuilder<'_> {
    fn drop(&mut self) {
        match self.branch {
            IfBranch::Then => {
                self.builder.then_blk = Some(self.block);
            }
            IfBranch::Else => {
                self.builder.else_blk = Some(self.block);
            }
        }

        // If the if.true instruction is complete, validate that the operand stack in
        // both branches is identical
        //
        // Otherwise, save the state of the stack here to be compared to the other
        // branch when it is constructed
        let is_complete = self.builder.then_blk.is_some() && self.builder.else_blk.is_some();
        if is_complete {
            assert_eq!(
                self.stack.stack(),
                self.builder.out_stack.stack(),
                "expected the operand stack to be in the same abstract state upon exit from \
                 either branch of this if.true instruction"
            );
        } else {
            core::mem::swap(&mut self.builder.out_stack, &mut self.stack);
        }
    }
}

#[doc(hidden)]
enum LoopType {
    While,
    Repeat(u16),
}

/// This builder is used to construct both `while.true` and `repeat.n` loops, enforcing
/// their individual invariants with regard to the operand stack.
///
/// In particular, this builder ensures that the body of a `while.true` loop is valid,
/// i.e. that when returning to the top of the loop to evaluate the conditional, that
/// there is a boolean value on top of the stack for that purpose. Similarly, it validates
/// that after the conditional has been evaluated, that the abstract state of the operand
/// stack is the same across iterations, and regardless of whether the loop is taken. The
/// abstract state in question is the number, and type, of the operands on the stack.
///
/// # Example
///
/// The general usage here looks like this, where `masm_builder` is an instance of
/// [MasmBuilder]:
///
/// ```text,ignore
/// // For our example here, we're generating inline assembly that performs
/// // the equivalent of `for (i = 0; i < len; i++) sum += array[i / 4][i % 4]`,
/// // where `array` is a pointer to words, and we're attempting to sum `len`
/// // field elements, across how ever many words that spans.
/// //
/// // We assume the operand stack is as follows (top to bottom):
/// //
/// //    [len, sum, array]
/// //
/// // First, build out the loop header
/// masm_builder.ins().push(Felt::ZERO); // [i, len, sum, array]
/// masm_builder.ins().dup(0);  // [i, i, len, sum, array]
/// masm_builder.ins().dup(2);  // [len, i, i, len, sum, array]
/// masm_builder.ins().lt();    // [i < len, i, len, sum, array]
///
/// // Now, build the loop body
/// //
/// // The state of the stack on entry is: [i, len, sum, array]
/// let mut lb = masm_builder.ins().while_true();
///
/// // Calculate `i / 4`
/// lb.ins().dup(0);     // [i, i, len, sum, array]
/// lb.ins().div_imm(4); // [word_offset, i, len, sum, array]
///
/// // Calculate the address for `array[i / 4]`
/// lb.ins().dup(4);     // [array, word_offset, ..]
/// lb.ins().add_u32(Overflow::Checked); // [array + word_offset, i, ..]
///
/// // Calculate the `i % 4`
/// lb.ins().dup(1);     // [i, array + word_offset, ..]
/// lb.ins().mod_imm_u32(4); // [element_offset, array + word_offset, ..]
///
/// // Precalculate what elements of the word to drop, so that
/// // we are only left with the specific element we wanted
/// lb.ins().dup(0);     // [element_offset, element_offset, ..]
/// lb.ins().lt_imm(Felt::new(3)); // [element_offset < 3, element_offset, ..]
/// lb.ins().dup(1);     // [element_offset, element_offset < 3, ..]
/// lb.ins().lt_imm(Felt::new(2)); // [element_offset < 2, element_offset < 3, ..]
/// lb.ins().dup(2);     // [element_offset, element_offset < 2, ..]
/// lb.ins().lt_imm(Felt::new(1)); // [element_offset < 1, element_offset < 2, ..]
///
/// // Load the word
/// lb.ins().dup(4);      // [array + word_offset, element_offset < 1]
/// lb.ins().loadw(); // [word[0], word[1], word[2], word[3], element_offset < 1]
///
/// // Select the element, `E`, that we want by conditionally dropping
/// // elements on the operand stack with a carefully chosen sequence
/// // of conditionals: E < N forall N in 0..=3
/// lb.ins().movup(4);   // [element_offset < 1, word[0], ..]
/// lb.ins().cdrop();    // [word[0 or 1], word[2], word[3], element_offset < 2]
/// lb.ins().movup(3);   // [element_offset < 2, word[0 or 1], ..]
/// lb.ins().cdrop();    // [word[0 or 1 or 2], word[3], element_offset < 3]
/// lb.ins().movup(2);   // [element_offset < 3, ..]
/// lb.ins().cdrop();    // [array[i], i, len, sum, array]
/// lb.ins().movup(3);   // [sum, array[i], i, len, array]
/// lb.ins().add();      // [sum + array[i], i, len, array]
/// lb.ins().movdn(2);   // [i, len, sum + array[i], array]
///
/// // We've reached the end of the loop, but we need a copy of the
/// // loop header here in order to use the expression `i < len` as
/// // the condition for the loop
/// lb.ins().dup(0);     // [i, i, len, ..]
/// lb.ins().dup(2);     // [len, i, i, len, ..]
/// lb.ins().lt();       // [i < len, i, len, sum, array]
///
/// // Finalize, it is at this point that validation will occur
/// lb.build();
/// ```
pub struct LoopBuilder<'a> {
    dfg: &'a mut DataFlowGraph,
    asm: &'a mut InlineAsm,
    /// This reference is to the operand stack in the parent [MasmOpBuilder],
    /// which represents the operand stack on entry to the loop. Upon finalization
    /// of the loop, we use update this operand stack to reflect the state upon
    /// exit from the loop.
    ///
    /// In effect, when the call to `while_true` or `repeat` returns, the operand
    /// stack in the parent builder will look as if the loop instruction has finished
    /// executing.
    in_stack: &'a mut OperandStack<Type>,
    /// This is the operand stack state within the loop.
    ///
    /// Upon finalization of the loop instruction, this state is used to validate
    /// the effect of the loop body on the operand stack. For `repeat`, which is
    /// unconditionally entered, no special validation is performed. However, for
    /// `while.true`, we must validate two things:
    ///
    /// 1. That the top of the stack holds a boolean value
    /// 2. That after popping the boolean, the output state of the operand stack matches the input
    ///    state in number and type of elements. This is required, as otherwise program behavior is
    ///    undefined based on whether the loop is entered or not.
    out_stack: OperandStack<Type>,
    /// The source span associated with the loop
    span: SourceSpan,
    /// The block to which the loop instruction will be appended
    ip: MasmBlockId,
    /// The top-level block for the loop
    body: MasmBlockId,
    /// The type of loop we're building
    style: LoopType,
}
impl LoopBuilder<'_> {
    /// Get a builder for a single MASM instruction
    pub fn ins<'a, 'b: 'a>(&'b mut self) -> MasmOpBuilder<'a> {
        MasmOpBuilder {
            dfg: self.dfg,
            asm: self.asm,
            stack: &mut self.out_stack,
            ip: self.body,
        }
    }

    /// Finalize construction of this loop, performing any final validation.
    pub fn build(mut self) {
        match self.style {
            LoopType::While => {
                // First, validate that the top of the stack holds a boolean
                let cond = self.out_stack.pop().expect("operand stack is empty");
                assert_eq!(
                    cond,
                    Type::I1,
                    "expected there to be a boolean on top of the stack at the end of the \
                     while.true body"
                );
                // Next, validate that the contents of the operand stack match
                // the input stack, in order to ensure that the operand stack
                // is consistent whether the loop is taken or not
                assert_eq!(
                    self.in_stack.stack(),
                    self.out_stack.stack(),
                    "expected the operand stack to be in the same abstract state whether the \
                     while.true loop is taken or skipped"
                );
                self.asm.push(self.ip, MasmOp::While(self.body), self.span);
            }
            LoopType::Repeat(1) => {
                // This is an edge case, but a single iteration `repeat` is no different than
                // inlining the loop body into the outer code block and eliding the `repeat`.
                //
                // Since that is the case, we literally do that transformation here, to simplify
                // the IR as much as possible during construction.
                let id = self.body;
                let mut block = core::mem::replace(
                    &mut self.asm.blocks[id],
                    MasmBlock {
                        id,
                        ops: smallvec![],
                    },
                );
                self.asm.blocks[self.ip].append(&mut block.ops);
            }
            LoopType::Repeat(n) => {
                // Apply the stack effects of the loop body `n` times, asserting if some operation
                // in the loop fails due to type mismatches. This is sufficient to validate
                // `repeat`, as it's iteration count is known statically, entry into
                // the loop is unconditional, and the only way to exit the loop is
                // to complete all `n` iterations.
                //
                // By validating in this way, we also implicitly validate the following:
                //
                // 1. If we were to translate this to SSA form, the resulting control flow graph
                //    would
                // have the same number and type of arguments passed to the loop header both on
                // entry and along the loopback edges.
                //
                // 2. If the body of the loop removes elements from the stack, we ensure that all
                //    `n`
                // iterations can be performed without exhausting the stack, or perform any other
                // invalid stack operation.
                let code = &self.asm.blocks[self.body];
                for _ in 1..n {
                    for op in code.ops.iter() {
                        apply_op_stack_effects(op, &mut self.out_stack, self.dfg, self.asm);
                    }
                }
                self.asm.push(self.ip, MasmOp::Repeat(n, self.body), self.span);
            }
        }

        // Update the operand stack to represent the state after execution of this loop
        let in_stack = self.in_stack.stack_mut();
        in_stack.clear();
        in_stack.append(self.out_stack.stack_mut());
    }
}

/// Asserts that the given value is an integer type which is compatible with u32 operations
macro_rules! assert_compatible_u32_operand {
    ($ty:ident) => {
        assert!(
            $ty.is_pointer() || Type::U32.is_compatible_operand(&$ty),
            "expected operand to be u32-compatible, got {}",
            $ty
        );
    };

    ($ty:ident, $op:expr) => {
        assert!(
            $ty.is_pointer() || Type::U32.is_compatible_operand(&$ty),
            "expected operand for {} to be u32-compatible, got {}",
            $op,
            $ty
        );
    };
}

/// Asserts that the given value is an integer type which is compatible with u32 operations
macro_rules! assert_compatible_u32_operands {
    ($lty:ident, $rty:ident) => {
        assert_matches!(
            $lty,
            Type::U8 | Type::U16 | Type::U32 | Type::Ptr(_),
            "expected controlling type to be u32-compatible, got {}",
            $lty
        );
        assert_compatible_operand_types!($lty, $rty);
    };

    ($lty:ident, $rty:ident, $op:expr) => {
        assert_matches!(
            $lty,
            Type::U8 | Type::U16 | Type::U32 | Type::Ptr(_),
            "expected controlling type for {} to be u32-compatible, got {}",
            $op,
            $lty
        );
        assert_compatible_operand_types!($lty, $rty, $op);
    };
}

/// Asserts that the given value is an integer or pointer type which is compatible with basic felt
/// arithmetic
macro_rules! assert_compatible_arithmetic_operand {
    ($ty:ident) => {
        assert!(
            $ty.is_pointer() || Type::Felt.is_compatible_operand(&$ty),
            "expected operand to be felt-compatible, got {}",
            $ty
        );
    };

    ($ty:ident, $op:expr) => {
        assert!(
            $ty.is_pointer() || Type::Felt.is_compatible_operand(&$ty),
            "expected operand for {} to be felt-compatible, got {}",
            $op,
            $ty
        );
    };
}

/// Asserts that the given value is an integer type which is compatible with felt operations
macro_rules! assert_compatible_felt_operand {
    ($ty:ident) => {
        assert!(
            Type::Felt.is_compatible_operand(&$ty),
            "expected operand to be felt-compatible, got {}",
            $ty
        );
    };

    ($ty:ident, $op:expr) => {
        assert!(
            Type::Felt.is_compatible_operand(&$ty),
            "expected operand for {} to be felt-compatible, got {}",
            $op,
            $ty
        );
    };
}

/// Asserts that the given value is an integer or pointer type which is compatible with basic
/// arithmetic operations as a felt
macro_rules! assert_compatible_arithmetic_operands {
    ($lty:ident, $rty:ident) => {
        assert_matches!(
            $lty,
            Type::U8
                | Type::I8
                | Type::U16
                | Type::I16
                | Type::U32
                | Type::I32
                | Type::Ptr(_)
                | Type::Felt,
            "expected controlling type to be felt-compatible, got {}",
            $lty
        );
        assert_compatible_operand_types!($lty, $rty);
    };

    ($lty:ident, $rty:ident, $op:expr) => {
        assert_matches!(
            $lty,
            Type::U8
                | Type::I8
                | Type::U16
                | Type::I16
                | Type::U32
                | Type::I32
                | Type::Ptr(_)
                | Type::Felt,
            "expected controlling type for {} to be felt-compatible, got {}",
            $op,
            $lty
        );
        assert_compatible_operand_types!($lty, $rty, $op);
    };
}

/// Asserts that the given value is an integer type which is compatible with felt operations
macro_rules! assert_compatible_felt_operands {
    ($lty:ident, $rty:ident) => {
        assert_matches!(
            $lty,
            Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32 | Type::Felt,
            "expected controlling type to be felt-compatible, got {}",
            $lty
        );
        assert_compatible_operand_types!($lty, $rty);
    };

    ($lty:ident, $rty:ident, $op:expr) => {
        assert_matches!(
            $lty,
            Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32 | Type::Felt,
            "expected controlling type for {} to be felt-compatible, got {}",
            $op,
            $lty
        );
        assert_compatible_operand_types!($lty, $rty, $op);
    };
}

/// Asserts that the two operands are of compatible types, where the first operand is assumed to
/// determine the controlling type
macro_rules! assert_compatible_operand_types {
    ($lty:ident, $rty:ident) => {
        assert!(
            $lty.is_compatible_operand(&$rty),
            "expected operands to be compatible types, the controlling type {} is not compatible \
             with {}",
            $lty,
            $rty
        );
    };

    ($lty:ident, $rty:ident, $op:expr) => {
        assert!(
            $lty.is_compatible_operand(&$rty),
            "expected operands for {} to be compatible types, the controlling type {} is not \
             compatible with {}",
            $op,
            $lty,
            $rty
        );
    };
}

fn apply_op_stack_effects(
    op: &MasmOp,
    stack: &mut OperandStack<Type>,
    dfg: &DataFlowGraph,
    asm: &InlineAsm,
) {
    match op {
        MasmOp::Padw => {
            stack.padw();
        }
        MasmOp::Push(_) => {
            stack.push(Type::Felt);
        }
        MasmOp::Push2(_) => {
            stack.push(Type::Felt);
            stack.push(Type::Felt);
        }
        MasmOp::Pushw(_) => {
            stack.padw();
        }
        MasmOp::PushU8(_) => {
            stack.push(Type::U8);
        }
        MasmOp::PushU16(_) => {
            stack.push(Type::U16);
        }
        MasmOp::PushU32(_) => {
            stack.push(Type::U32);
        }
        MasmOp::Drop => {
            stack.drop();
        }
        MasmOp::Dropw => {
            stack.dropw();
        }
        MasmOp::Dup(idx) => {
            stack.dup(*idx as usize);
        }
        MasmOp::Dupw(idx) => {
            stack.dupw(*idx as usize);
        }
        MasmOp::Swap(idx) => {
            stack.swap(*idx as usize);
        }
        MasmOp::Swapw(idx) => {
            stack.swapw(*idx as usize);
        }
        MasmOp::Swapdw => {
            stack.swapdw();
        }
        MasmOp::Movup(idx) => {
            stack.movup(*idx as usize);
        }
        MasmOp::Movupw(idx) => {
            stack.movupw(*idx as usize);
        }
        MasmOp::Movdn(idx) => {
            stack.movdn(*idx as usize);
        }
        MasmOp::Movdnw(idx) => {
            stack.movdnw(*idx as usize);
        }
        MasmOp::Cswap | MasmOp::Cswapw => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_eq!(ty, Type::I1, "expected boolean operand on top of the stack");
        }
        MasmOp::Cdrop => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_eq!(ty, Type::I1, "expected boolean operand on top of the stack");
            stack.drop();
        }
        MasmOp::Cdropw => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_eq!(ty, Type::I1, "expected boolean operand on top of the stack");
            stack.dropw();
        }
        MasmOp::Assert
        | MasmOp::Assertz
        | MasmOp::AssertWithError(_)
        | MasmOp::AssertzWithError(_) => {
            stack.drop();
        }
        MasmOp::AssertEq | MasmOp::AssertEqWithError(_) => {
            stack.dropn(2);
        }
        MasmOp::AssertEqw | MasmOp::AssertEqwWithError(_) => {
            stack.dropn(8);
        }
        MasmOp::LocAddr(_id)
        | MasmOp::LocLoad(_id)
        | MasmOp::LocLoadw(_id)
        | MasmOp::LocStore(_id)
        | MasmOp::LocStorew(_id) => unreachable!(),
        MasmOp::MemLoad => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_matches!(
                ty,
                Type::Ptr(_) | Type::NativePtr(_, _),
                "invalid load: expected pointer operand"
            );
            stack.push(ty.pointee().unwrap().clone());
        }
        MasmOp::MemLoadImm(_) => {
            // We don't know what we're loading, so fall back to the default type of field element
            stack.push(Type::Felt);
        }
        MasmOp::MemLoadw => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_matches!(
                ty,
                Type::Ptr(_) | Type::NativePtr(_, _),
                "invalid load: expected pointer operand"
            );
            // We're always loading a raw word with this op
            stack.padw();
        }
        MasmOp::MemLoadwImm(_) => {
            // We're always loading a raw word with this op
            stack.padw();
        }
        MasmOp::MemStore => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_matches!(
                ty,
                Type::Ptr(_) | Type::NativePtr(_, _),
                "invalid store: expected pointer operand"
            );
            stack.drop();
        }
        MasmOp::MemStoreImm(_) => {
            stack.drop();
        }
        MasmOp::MemStorew => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_matches!(
                ty,
                Type::Ptr(_) | Type::NativePtr(_, _),
                "invalid store: expected pointer operand"
            );
            stack.dropw();
        }
        MasmOp::MemStorewImm(_) => {
            stack.dropw();
        }
        MasmOp::MemStream => {
            // Read two sequential words from memory starting at `a`, overwriting the first two
            // words on the stack, and advancing `a` to the next address following the
            // two that were loaded [C, B, A, a] <- [*a, *(a + 1), A, a + 2]
            assert!(stack.len() > 12, "expected at least 13 elements on the stack for mem_stream");
            stack.dropw();
            stack.dropw();
            stack.padw();
            stack.padw();
        }
        MasmOp::AdvPipe => {
            // Pops the next two words from the advice stack, overwrites the
            // top of the operand stack with them, and also writes these words
            // into memory at `a` and `a + 1`
            //
            // [C, B, A, a] <- [*a, *(a + 1), A, a + 2]
            assert!(stack.len() > 12, "expected at least 13 elements on the stack for mem_stream");
            stack.dropw();
            stack.dropw();
            stack.padw();
            stack.padw();
        }
        MasmOp::AdvPush(n) => {
            for _ in 0..(*n) {
                stack.push(Type::Felt);
            }
        }
        MasmOp::AdvLoadw => {
            assert!(stack.len() > 3, "expected at least 4 elements on the stack for mem_stream");
            stack.dropw();
            stack.padw();
        }
        MasmOp::AdvInjectInsertMem
        | MasmOp::AdvInjectInsertHperm
        | MasmOp::AdvInjectInsertHdword
        | MasmOp::AdvInjectPushMapVal
        | MasmOp::AdvInjectPushMapValN
        | MasmOp::AdvInjectPushU64Div
        | MasmOp::AdvInjectPushMTreeNode
        | MasmOp::AdvInjectPushSignature(_) => (),
        MasmOp::Hash => {
            assert!(stack.len() > 3, "expected at least 4 elements on the stack for hash");
        }
        MasmOp::Hperm => {
            assert!(stack.len() >= 12, "expected at least 12 elements on the stack for hperm");
        }
        MasmOp::Hmerge => {
            assert!(stack.len() >= 8, "expected at least 8 elements on the stack for hmerge");
            stack.dropw();
        }
        MasmOp::MtreeGet => {
            assert!(stack.len() >= 6, "expected at least 6 elements on the stack for mtree_get");
            stack.dropn(2);
            stack.pushw([Type::Felt, Type::Felt, Type::Felt, Type::Felt]);
        }
        MasmOp::MtreeSet => {
            assert!(stack.len() >= 10, "expected at least 10 elements on the stack for mtree_set");
            stack.dropn(2);
            stack.swapw(1);
        }
        MasmOp::MtreeMerge => {
            assert!(stack.len() >= 8, "expected at least 8 elements on the stack for mtree_merge");
            stack.dropw();
        }
        MasmOp::MtreeVerify | MasmOp::MtreeVerifyWithError(_) => {
            assert!(
                stack.len() >= 10,
                "expected at least 10 elements on the stack for mtree_verify"
            );
        }
        MasmOp::RCombBase => {
            assert!(stack.len() >= 15, "expected at least 15 elements on the stack for rcomb_base");
        }
        MasmOp::FriExt2Fold4 => {
            assert!(
                stack.len() >= 16,
                "expected at least 16 elements on the stack for fri_ext2fold4"
            );
        }
        // This function is not called from [MasmOpBuilder] when building an `if.true` instruction,
        // instead, the only time we are evaluating this is when traversing the body of a `repeat.n`
        // instruction and applying the stack effects of instructions which have already been
        // inserted once.
        //
        // NOTE: We only apply the effects from a single branch, because we have already validated
        // that regardless of which branch is taken, the stack effects are the same.
        MasmOp::If(then_body, _else_body) => {
            let lty = stack.pop().expect("operand stack is empty");
            assert_eq!(lty, Type::I1, "expected boolean conditional");
            let body = asm.blocks[*then_body].ops.as_slice();
            for op in body.iter() {
                apply_op_stack_effects(op, stack, dfg, asm);
            }
        }
        // This function is not called from [MasmOpBuilder] when building an `while.true`
        // instruction, instead, the only time we are evaluating this is when traversing the
        // body of a `repeat.n` instruction and applying the stack effects of instructions
        // which have already been inserted once.
        //
        // NOTE: We don't need to traverse the body of the `while.true`, because we have already
        // validated that whether the loop is taken or not, the stack effects are the same
        MasmOp::While(_body) => {
            let lty = stack.pop().expect("operand stack is empty");
            assert_eq!(lty, Type::I1, "expected boolean conditional");
        }
        // This function is not called from [MasmOpBuilder] when building an `repeat.n` instruction,
        // instead, the only time we are evaluating this is when traversing the body of a `repeat.n`
        // instruction and applying the stack effects of instructions which have already been
        // inserted once.
        MasmOp::Repeat(n, body) => {
            let body = asm.blocks[*body].ops.as_slice();
            for _ in 0..*n {
                for op in body.iter() {
                    apply_op_stack_effects(op, stack, dfg, asm);
                }
            }
        }
        MasmOp::Exec(ref id) => {
            execute_call(id, false, stack, dfg);
        }
        MasmOp::Call(ref id) => {
            execute_call(id, false, stack, dfg);
        }
        MasmOp::Syscall(ref id) => {
            execute_call(id, true, stack, dfg);
        }
        MasmOp::DynExec | MasmOp::DynCall => {
            assert!(
                stack.len() > 3,
                "expected at least 4 elements on the stack for dynexec/dyncall"
            );
        }
        MasmOp::ProcRef(_) => {
            stack.pushw([Type::Felt, Type::Felt, Type::Felt, Type::Felt]);
        }
        MasmOp::Add | MasmOp::Sub => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_arithmetic_operands!(lty, rty, op);
            stack.push(lty);
        }
        MasmOp::Mul | MasmOp::Div => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_felt_operands!(lty, rty, op);
            stack.push(lty);
        }
        MasmOp::AddImm(_) | MasmOp::SubImm(_) | MasmOp::Incr => {
            let ty = stack.peek().expect("operand stack is empty");
            assert_compatible_arithmetic_operand!(ty, op);
        }
        MasmOp::MulImm(_)
        | MasmOp::DivImm(_)
        | MasmOp::Neg
        | MasmOp::Inv
        | MasmOp::Ilog2
        | MasmOp::Pow2
        | MasmOp::ExpImm(_)
        | MasmOp::ExpBitLength(_) => {
            let ty = stack.peek().expect("operand stack is empty");
            assert_compatible_felt_operand!(ty, op);
        }
        MasmOp::Exp => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_felt_operands!(lty, rty);
            stack.push(lty);
        }
        MasmOp::Not | MasmOp::AndImm(_) | MasmOp::OrImm(_) | MasmOp::XorImm(_) => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_eq!(ty, Type::I1, "expected boolean type");
            stack.push(ty);
        }
        MasmOp::And | MasmOp::Or | MasmOp::Xor => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_eq!(lty, rty, "expected operands for {} to be the same type", op);
            assert_eq!(lty, Type::I1, "expected boolean operands for {}", op);
            stack.push(lty);
        }
        MasmOp::Eq | MasmOp::Neq | MasmOp::Gt | MasmOp::Gte | MasmOp::Lt | MasmOp::Lte => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_felt_operands!(lty, rty, op);
            stack.push(Type::I1);
        }
        MasmOp::EqImm(_)
        | MasmOp::NeqImm(_)
        | MasmOp::GtImm(_)
        | MasmOp::GteImm(_)
        | MasmOp::LtImm(_)
        | MasmOp::LteImm(_)
        | MasmOp::IsOdd => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_compatible_felt_operand!(ty, op);
            stack.push(Type::I1);
        }
        MasmOp::Eqw => {
            stack.dropn(8);
            stack.push(Type::I1);
        }
        op @ (MasmOp::Ext2add | MasmOp::Ext2sub | MasmOp::Ext2mul | MasmOp::Ext2div) => {
            assert!(stack.len() > 3, "expected at least 4 elements on the operand stack for {op}");
            stack.dropn(2);
        }
        op @ (MasmOp::Ext2neg | MasmOp::Ext2inv) => {
            assert!(stack.len() > 1, "expected at least 2 elements on the operand stack for {op}");
        }
        MasmOp::Clk | MasmOp::Sdepth => {
            stack.push(Type::Felt);
        }
        MasmOp::Caller => {
            assert!(stack.len() > 3, "expected at least 4 elements on the operand stack");
            stack.popw();
            stack.pushw([Type::Felt, Type::Felt, Type::Felt, Type::Felt]);
        }
        MasmOp::U32Test => {
            assert!(!stack.is_empty());
            stack.push(Type::I1);
        }
        MasmOp::U32Testw => {
            assert!(stack.len() > 3, "expected at least 4 elements on the operand stack");
            stack.push(Type::I1);
        }
        MasmOp::U32Assert | MasmOp::U32AssertWithError(_) => {
            assert!(!stack.is_empty());
        }
        MasmOp::U32Assert2 | MasmOp::U32Assert2WithError(_) => {
            assert!(stack.len() > 1, "expected at least 2 elements on the operand stack");
        }
        MasmOp::U32Assertw | MasmOp::U32AssertwWithError(_) => {
            assert!(stack.len() > 3, "expected at least 4 elements on the operand stack");
        }
        MasmOp::U32Cast => {
            let lty = stack.pop().expect("operand stack is empty");
            assert_eq!(lty, Type::Felt, "expected felt operand");
            stack.push(Type::U32);
        }
        MasmOp::U32Split => {
            let lty = stack.pop().expect("operand stack is empty");
            assert_eq!(lty, Type::Felt, "expected felt operand");
            stack.push(Type::U32);
            stack.push(Type::U32);
        }
        MasmOp::U32Lt | MasmOp::U32Gt | MasmOp::U32Lte | MasmOp::U32Gte => {
            let rty = stack.pop().expect("failed to pop right operand: stack is empty");
            let lty = stack.pop().expect("failed to pop left operand: stack is empty");
            assert_compatible_u32_operands!(lty, rty, op);
            stack.push(Type::I1);
        }
        MasmOp::U32Div
        | MasmOp::U32Mod
        | MasmOp::U32DivMod
        | MasmOp::U32Shl
        | MasmOp::U32Shr
        | MasmOp::U32Rotl
        | MasmOp::U32Rotr
        | MasmOp::U32Min
        | MasmOp::U32Max
        | MasmOp::U32WrappingAdd
        | MasmOp::U32WrappingSub
        | MasmOp::U32WrappingMul
        | MasmOp::U32And
        | MasmOp::U32Or
        | MasmOp::U32Xor => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(lty, rty, op);
            stack.push(lty);
        }
        MasmOp::U32DivImm(_)
        | MasmOp::U32ModImm(_)
        | MasmOp::U32DivModImm(_)
        | MasmOp::U32ShlImm(_)
        | MasmOp::U32ShrImm(_)
        | MasmOp::U32RotlImm(_)
        | MasmOp::U32RotrImm(_)
        | MasmOp::U32Popcnt
        | MasmOp::U32Clz
        | MasmOp::U32Ctz
        | MasmOp::U32Clo
        | MasmOp::U32Cto
        | MasmOp::U32WrappingAddImm(_)
        | MasmOp::U32WrappingSubImm(_)
        | MasmOp::U32WrappingMulImm(_)
        | MasmOp::U32Not
        | MasmOp::U32LtImm(_)
        | MasmOp::U32LteImm(_)
        | MasmOp::U32GtImm(_)
        | MasmOp::U32GteImm(_)
        | MasmOp::U32MinImm(_)
        | MasmOp::U32MaxImm(_) => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operand!(ty, op);
            stack.push(ty);
        }
        MasmOp::U32OverflowingAdd | MasmOp::U32OverflowingSub | MasmOp::U32OverflowingMul => {
            let rty = stack.pop().expect("operand stack is empty");
            let lty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(lty, rty, op);
            stack.push(lty);
            stack.push(Type::I1);
        }
        MasmOp::U32OverflowingAddImm(_)
        | MasmOp::U32OverflowingSubImm(_)
        | MasmOp::U32OverflowingMulImm(_) => {
            let ty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operand!(ty, op);
            stack.push(ty);
            stack.push(Type::I1);
        }
        MasmOp::U32OverflowingAdd3 => {
            let cty = stack.pop().expect("operand stack is empty");
            let bty = stack.pop().expect("operand stack is empty");
            let aty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(aty, bty);
            assert_compatible_u32_operands!(aty, cty);
            stack.push(aty);
            stack.push(Type::U32);
        }
        MasmOp::U32OverflowingMadd => {
            let bty = stack.pop().expect("operand stack is empty");
            let aty = stack.pop().expect("operand stack is empty");
            let cty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(aty, bty);
            assert_compatible_u32_operands!(aty, cty);
            stack.push(aty);
            stack.push(Type::U32);
        }
        MasmOp::U32WrappingAdd3 => {
            let cty = stack.pop().expect("operand stack is empty");
            let bty = stack.pop().expect("operand stack is empty");
            let aty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(aty, bty);
            assert_compatible_u32_operands!(aty, cty);
            stack.push(aty);
        }
        MasmOp::U32WrappingMadd => {
            let bty = stack.pop().expect("operand stack is empty");
            let aty = stack.pop().expect("operand stack is empty");
            let cty = stack.pop().expect("operand stack is empty");
            assert_compatible_u32_operands!(aty, bty);
            assert_compatible_u32_operands!(aty, cty);
            stack.push(aty);
        }
        MasmOp::Emit(_) | MasmOp::Trace(_) | MasmOp::Breakpoint => (),
        MasmOp::DebugStack
        | MasmOp::DebugStackN(_)
        | MasmOp::DebugMemory
        | MasmOp::DebugMemoryAt(_)
        | MasmOp::DebugMemoryRange(..)
        | MasmOp::DebugFrame
        | MasmOp::DebugFrameAt(_)
        | MasmOp::DebugFrameRange(..)
        | MasmOp::Nop => (),
    }
}

/// Validate that a call to `id` is possible given the current state of the operand stack,
/// and if so, update the state of the operand stack to reflect the call.
fn execute_call(
    id: &FunctionIdent,
    is_syscall: bool,
    stack: &mut OperandStack<Type>,
    dfg: &DataFlowGraph,
) {
    let import = dfg.get_import(id).expect("unknown function, are you missing an import?");
    if is_syscall {
        assert_eq!(
            import.signature.cc,
            CallConv::Kernel,
            "cannot call a non-kernel function with the `syscall` instruction"
        );
    } else {
        assert_ne!(
            import.signature.cc,
            CallConv::Kernel,
            "`syscall` cannot be used to call non-kernel functions"
        );
    }
    match import.signature.cc {
        // For now, we're treating all calling conventions the same as SystemV
        CallConv::Fast
        | CallConv::SystemV
        | CallConv::Kernel
        | CallConv::CanonLift
        | CallConv::CanonLower => {
            // Visit the argument list in reverse (so that the top of the stack on entry
            // is the first argument), and allocate elements based on the argument types.
            let mut elements_needed = 0;
            for param in import.signature.params().iter().rev() {
                elements_needed += param.ty.size_in_felts();
            }

            // Verify that we have `elements_needed` values on the operand stack
            let elements_available = stack.len();
            assert!(
                elements_needed <= elements_available,
                "the operand stack does not contain enough values to call {} ({} exepected vs {} \
                 available)",
                id,
                elements_needed,
                elements_available
            );
            stack.dropn(elements_needed);

            // Update the operand stack to reflect the results
            for result in import.signature.results().iter().rev() {
                push_type_on_stack(result.ty.clone(), stack);
            }
        }
        CallConv::Wasm => unimplemented!("canonical abi support is not yet implemented"),
    }
}

fn push_type_on_stack(ty: Type, stack: &mut OperandStack<Type>) {
    let parts = ty.to_raw_parts().expect("invalid unknown type: cannot proceed");
    for part in parts.into_iter().rev() {
        stack.push(part);
    }
}
