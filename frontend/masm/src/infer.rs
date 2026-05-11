use std::{cell::RefCell, ops::Range, rc::Rc};

use miden_assembly_syntax::{
    Path as MasmPath,
    ast::{Block, Immediate, Instruction, InvocationTarget, Op, Procedure},
    debuginfo::SourceSpan,
    parser::{IntValue, PushValue, WordValue},
};
use midenc_hir::{
    AddressSpace, ArrayType, CallConv, Context, PointerType, Type,
    dialects::builtin::attributes::Signature,
};
use rustc_hash::FxHashMap;

use crate::{
    Result, error,
    events::system_event_read_count,
    semantics::{self, InstructionSemantics},
    stack as masm_stack,
};

pub(crate) fn infer_signature(
    context: &Rc<Context>,
    procedure: &Procedure,
    signatures: &FxHashMap<String, Signature>,
    external_signatures: &FxHashMap<String, Signature>,
) -> Result<Signature> {
    let mut state = InferState::new(signatures, external_signatures);
    state.infer_block(procedure.body())?;

    let params = state.inputs.iter().map(AbstractValue::ty_or_felt);
    let results = state.stack.iter().rev().map(AbstractValue::ty_or_felt);

    Ok(Signature::with_convention(context, CallConv::Fast, params, results))
}

#[derive(Clone)]
struct AbstractValue(Rc<RefCell<AbstractValueState>>);

struct AbstractValueState {
    ty: Option<Type>,
    constraints: Vec<TypeConstraint>,
    #[allow(dead_code)]
    provenance: ValueProvenance,
}

#[allow(dead_code)]
#[derive(Clone)]
struct TypeConstraint {
    ty: Type,
    span: SourceSpan,
}

#[allow(dead_code)]
#[derive(Clone)]
enum ValueProvenance {
    Input {
        index: usize,
        span: SourceSpan,
    },
    Produced {
        span: SourceSpan,
    },
    BranchJoin {
        span: SourceSpan,
        lhs: Box<ValueProvenance>,
        rhs: Box<ValueProvenance>,
    },
}

impl AbstractValue {
    fn input(index: usize, span: SourceSpan) -> Self {
        Self::from_state(None, ValueProvenance::Input { index, span })
    }

    fn typed(ty: Type, span: SourceSpan) -> Self {
        let value = Self::from_state(None, ValueProvenance::Produced { span });
        value.constrain(ty, span);
        value
    }

    fn from_state(ty: Option<Type>, provenance: ValueProvenance) -> Self {
        Self(Rc::new(RefCell::new(AbstractValueState {
            ty,
            constraints: Vec::new(),
            provenance,
        })))
    }

    fn constrain(&self, ty: Type, span: SourceSpan) {
        let mut current = self.0.borrow_mut();
        current.ty = refine_type(current.ty.take(), ty.clone());
        current.constraints.push(TypeConstraint { ty, span });
    }

    fn ty(&self) -> Option<Type> {
        self.0.borrow().ty.clone()
    }

    fn ty_or_felt(&self) -> Type {
        self.ty().unwrap_or(Type::Felt)
    }

    fn branch_join(lhs: &Self, rhs: &Self, span: SourceSpan) -> Self {
        let lhs_state = lhs.0.borrow();
        let rhs_state = rhs.0.borrow();
        let ty = join_types(lhs_state.ty.clone(), rhs_state.ty.clone());
        let provenance = ValueProvenance::BranchJoin {
            span,
            lhs: Box::new(lhs_state.provenance.clone()),
            rhs: Box::new(rhs_state.provenance.clone()),
        };
        let constraints = lhs_state
            .constraints
            .iter()
            .chain(rhs_state.constraints.iter())
            .cloned()
            .collect();
        Self(Rc::new(RefCell::new(AbstractValueState {
            ty,
            constraints,
            provenance,
        })))
    }

    fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

struct InferState<'a> {
    stack: Vec<AbstractValue>,
    inputs: Vec<AbstractValue>,
    current_span: SourceSpan,
    signatures: &'a FxHashMap<String, Signature>,
    external_signatures: &'a FxHashMap<String, Signature>,
}

impl<'a> InferState<'a> {
    fn new(
        signatures: &'a FxHashMap<String, Signature>,
        external_signatures: &'a FxHashMap<String, Signature>,
    ) -> Self {
        Self {
            stack: Vec::new(),
            inputs: Vec::new(),
            current_span: SourceSpan::UNKNOWN,
            signatures,
            external_signatures,
        }
    }

    fn branch_from(&self) -> Self {
        Self {
            stack: self.stack.clone(),
            inputs: Vec::new(),
            current_span: self.current_span,
            signatures: self.signatures,
            external_signatures: self.external_signatures,
        }
    }

    fn infer_block(&mut self, block: &Block) -> Result<()> {
        for op in block.iter() {
            match op {
                Op::Inst(inst) => self.infer_instruction(inst.inner(), inst.span())?,
                Op::If {
                    span,
                    then_blk,
                    else_blk,
                } => self.infer_if(then_blk, else_blk, *span)?,
                Op::While { span, body } => self.infer_while(body, *span)?,
                Op::Repeat { count, body, .. } => {
                    let count = immediate_u32(count)?;
                    for _ in 0..count {
                        self.infer_block(body)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn infer_instruction(&mut self, inst: &Instruction, span: SourceSpan) -> Result<()> {
        use Instruction::*;

        let previous_span = self.current_span;
        self.current_span = span;
        let result = match inst {
            Nop => Ok(()),
            Drop => self.drop_n(1, span),
            DropW => self.drop_n(4, span),
            PadW => {
                for _ in 0..4 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            Dup0 => self.dup(0, span),
            Dup1 => self.dup(1, span),
            Dup2 => self.dup(2, span),
            Dup3 => self.dup(3, span),
            Dup4 => self.dup(4, span),
            Dup5 => self.dup(5, span),
            Dup6 => self.dup(6, span),
            Dup7 => self.dup(7, span),
            Dup8 => self.dup(8, span),
            Dup9 => self.dup(9, span),
            Dup10 => self.dup(10, span),
            Dup11 => self.dup(11, span),
            Dup12 => self.dup(12, span),
            Dup13 => self.dup(13, span),
            Dup14 => self.dup(14, span),
            Dup15 => self.dup(15, span),
            DupW0 => self.dup_word(0, span),
            DupW1 => self.dup_word(1, span),
            DupW2 => self.dup_word(2, span),
            DupW3 => self.dup_word(3, span),
            Swap1 => self.swap(1, span),
            Swap2 => self.swap(2, span),
            Swap3 => self.swap(3, span),
            Swap4 => self.swap(4, span),
            Swap5 => self.swap(5, span),
            Swap6 => self.swap(6, span),
            Swap7 => self.swap(7, span),
            Swap8 => self.swap(8, span),
            Swap9 => self.swap(9, span),
            Swap10 => self.swap(10, span),
            Swap11 => self.swap(11, span),
            Swap12 => self.swap(12, span),
            Swap13 => self.swap(13, span),
            Swap14 => self.swap(14, span),
            Swap15 => self.swap(15, span),
            SwapW1 => self.swap_word(1, span),
            SwapW2 => self.swap_word(2, span),
            SwapW3 => self.swap_word(3, span),
            SwapDw => self.swap_chunks(8, 1, span),
            MovUp2 => self.movup(2, span),
            MovUp3 => self.movup(3, span),
            MovUp4 => self.movup(4, span),
            MovUp5 => self.movup(5, span),
            MovUp6 => self.movup(6, span),
            MovUp7 => self.movup(7, span),
            MovUp8 => self.movup(8, span),
            MovUp9 => self.movup(9, span),
            MovUp10 => self.movup(10, span),
            MovUp11 => self.movup(11, span),
            MovUp12 => self.movup(12, span),
            MovUp13 => self.movup(13, span),
            MovUp14 => self.movup(14, span),
            MovUp15 => self.movup(15, span),
            MovUpW2 => self.move_chunk_to_top(4, 2, span),
            MovUpW3 => self.move_chunk_to_top(4, 3, span),
            MovDn2 => self.movdn(2, span),
            MovDn3 => self.movdn(3, span),
            MovDn4 => self.movdn(4, span),
            MovDn5 => self.movdn(5, span),
            MovDn6 => self.movdn(6, span),
            MovDn7 => self.movdn(7, span),
            MovDn8 => self.movdn(8, span),
            MovDn9 => self.movdn(9, span),
            MovDn10 => self.movdn(10, span),
            MovDn11 => self.movdn(11, span),
            MovDn12 => self.movdn(12, span),
            MovDn13 => self.movdn(13, span),
            MovDn14 => self.movdn(14, span),
            MovDn15 => self.movdn(15, span),
            MovDnW2 => self.move_top_chunk_down(4, 2, span),
            MovDnW3 => self.move_top_chunk_down(4, 3, span),
            Reversew => self.reverse_n(4, span),
            Reversedw => self.reverse_n(8, span),
            Push(value) => self.push_immediate(value, span),
            PushSlice(value, range) => self.push_word_slice(value, range, span),
            PushFeltList(values) => {
                for _ in values {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            Sdepth => {
                self.push(Type::Felt);
                Ok(())
            }
            Add | Sub | Mul | Div | Neg | ILog2 | Inv | Incr | Pow2 => {
                self.unary_or_binary_felt(inst, span)
            }
            Ext2Add | Ext2Sub | Ext2Mul | Ext2Div => self.ext2_binary(span),
            Ext2Neg | Ext2Inv => self.ext2_unary(span),
            AddImm(_) | SubImm(_) | MulImm(_) | DivImm(_) | ExpImm(_) => {
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::Felt);
                Ok(())
            }
            Exp => {
                self.pop_with_type(Type::Felt, span)?;
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::Felt);
                Ok(())
            }
            Not => {
                self.pop_with_type(Type::I1, span)?;
                self.push(Type::I1);
                Ok(())
            }
            And | Or | Xor => {
                self.pop_with_type(Type::I1, span)?;
                self.pop_with_type(Type::I1, span)?;
                self.push(Type::I1);
                Ok(())
            }
            Eq | Neq | Lt | Lte | Gt | Gte => {
                self.pop_with_type(Type::Felt, span)?;
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::I1);
                Ok(())
            }
            EqImm(_) | NeqImm(_) | IsOdd => {
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::I1);
                Ok(())
            }
            Eqw => {
                self.pop_word_with_type(Type::Felt, span)?;
                self.pop_word_with_type(Type::Felt, span)?;
                self.push(Type::I1);
                Ok(())
            }
            U32WrappingAdd | U32WrappingSub | U32WrappingMul | U32Div | U32Mod | U32And | U32Or
            | U32Xor | U32Shr | U32Shl | U32Rotr | U32Rotl | U32Min | U32Max => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                Ok(())
            }
            U32WrappingAddImm(_) | U32WrappingSubImm(_) | U32WrappingMulImm(_) | U32DivImm(_)
            | U32ModImm(_) | U32ShrImm(_) | U32ShlImm(_) | U32RotrImm(_) | U32RotlImm(_) => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                Ok(())
            }
            U32OverflowingAdd | U32OverflowingSub => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::I1);
                Ok(())
            }
            U32OverflowingAddImm(_) | U32OverflowingSubImm(_) => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::I1);
                Ok(())
            }
            U32WideningAdd | U32WideningMul => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32WideningAddImm(_) | U32WideningMulImm(_) => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32WideningAdd3 | U32OverflowingAdd3 => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32WideningMadd => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32WrappingAdd3 | U32WrappingMadd => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                Ok(())
            }
            U32DivMod => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32DivModImm(_) => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            U32Not | U32Popcnt | U32Ctz | U32Clz | U32Clo | U32Cto => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::U32);
                Ok(())
            }
            U32Lt | U32Lte | U32Gt | U32Gte => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::I1);
                Ok(())
            }
            U32Cast => {
                self.pop_any(span)?;
                self.push(Type::U32);
                Ok(())
            }
            U32Assert | U32AssertWithError(_) => self.constrain_top_n(1, Type::U32, span),
            U32Assert2 | U32Assert2WithError(_) => self.constrain_top_n(2, Type::U32, span),
            U32AssertW | U32AssertWWithError(_) => self.constrain_top_n(4, Type::U32, span),
            U32Test => {
                self.constrain_top_n(1, Type::Felt, span)?;
                self.push(Type::I1);
                Ok(())
            }
            U32TestW => {
                self.constrain_top_n(4, Type::Felt, span)?;
                self.push(Type::I1);
                Ok(())
            }
            U32Split => {
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::U32);
                self.push(Type::U32);
                Ok(())
            }
            CSwap => self.conditional_swap(1, span),
            CSwapW => self.conditional_swap(4, span),
            CDrop => self.conditional_drop(1, span),
            CDropW => self.conditional_drop(4, span),
            Assert | AssertWithError(_) | Assertz | AssertzWithError(_) => {
                self.pop_any(span)?;
                Ok(())
            }
            AssertEq | AssertEqWithError(_) => {
                self.pop_any(span)?;
                self.pop_any(span)?;
                Ok(())
            }
            AssertEqw | AssertEqwWithError(_) => {
                self.pop_word_with_type(Type::Felt, span)?;
                self.pop_word_with_type(Type::Felt, span)?;
                Ok(())
            }
            LocLoad(_) => {
                self.push(Type::Felt);
                Ok(())
            }
            Locaddr(_) => {
                self.push(felt_memory_pointer_type());
                Ok(())
            }
            LocLoadWBe(_) | LocLoadWLe(_) => {
                for _ in 0..4 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            LocStore(_) => {
                self.pop_with_type(Type::Felt, span)?;
                Ok(())
            }
            LocStoreWBe(_) | LocStoreWLe(_) => self.constrain_top_n(4, Type::Felt, span),
            MemLoad => {
                self.pop_with_type(Type::U32, span)?;
                self.push(Type::Felt);
                Ok(())
            }
            MemLoadImm(_) => {
                self.push(Type::Felt);
                Ok(())
            }
            MemLoadWBe | MemLoadWLe => self.load_memory_word(true, span),
            MemLoadWBeImm(addr) | MemLoadWLeImm(addr) => {
                validate_memory_word_address(immediate_value(addr)?, span)?;
                self.load_memory_word(false, span)
            }
            MemStore => {
                self.pop_with_type(Type::U32, span)?;
                self.pop_with_type(Type::Felt, span)?;
                Ok(())
            }
            MemStoreImm(_) => {
                self.pop_with_type(Type::Felt, span)?;
                Ok(())
            }
            MemStoreWBe | MemStoreWLe => self.store_memory_word(true, span),
            MemStoreWBeImm(addr) | MemStoreWLeImm(addr) => {
                validate_memory_word_address(immediate_value(addr)?, span)?;
                self.store_memory_word(false, span)
            }
            Caller => {
                self.push(Type::from(ArrayType::new(Type::Felt, 4)));
                Ok(())
            }
            ProcRef(_) => {
                self.push(Type::from(ArrayType::new(Type::Felt, 4)));
                Ok(())
            }
            Clk => {
                self.push(Type::Felt);
                Ok(())
            }
            AdvPush(count) => {
                let count = immediate_value(count)?;
                validate_advice_read_count(count, span)?;
                for _ in 0..count {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            AdvLoadW => {
                self.drop_n(4, span)?;
                for _ in 0..4 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            Emit => {
                self.pop_with_type(Type::Felt, span)?;
                self.push(Type::Felt);
                Ok(())
            }
            EmitImm(_) => Ok(()),
            SysEvent(event) => {
                self.constrain_top_n(system_event_read_count(event), Type::Felt, span)
            }
            Hash => self.constrain_top_n(4, Type::Felt, span),
            HMerge => {
                for _ in 0..8 {
                    self.pop_with_type(Type::Felt, span)?;
                }
                for _ in 0..4 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            HPerm => self.constrain_top_n(12, Type::Felt, span),
            MTreeGet => {
                for _ in 0..6 {
                    self.pop_with_type(Type::Felt, span)?;
                }
                for _ in 0..8 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            MTreeSet => {
                for _ in 0..10 {
                    self.pop_with_type(Type::Felt, span)?;
                }
                for _ in 0..8 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            MTreeMerge => {
                for _ in 0..8 {
                    self.pop_with_type(Type::Felt, span)?;
                }
                for _ in 0..4 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            MTreeVerify | MTreeVerifyWithError(_) => self.constrain_top_n(10, Type::Felt, span),
            CryptoStream => self.constrain_top_n(14, Type::Felt, span),
            MemStream | AdvPipe => self.constrain_top_n(13, Type::Felt, span),
            FriExt2Fold4 => {
                for _ in 0..17 {
                    self.pop_with_type(Type::Felt, span)?;
                }
                for _ in 0..16 {
                    self.push(Type::Felt);
                }
                Ok(())
            }
            HornerBase | HornerExt => self.constrain_top_n(16, Type::Felt, span),
            EvalCircuit => self.constrain_top_n(3, Type::Felt, span),
            LogPrecompile => self.constrain_top_n(12, Type::Felt, span),
            Exec(target) | Call(target) | SysCall(target) => self.invoke(target, span),
            Debug(_) | DebugVar(_) | Trace(_) => Ok(()),
            _ => unsupported_instruction(inst, span),
        };
        self.current_span = previous_span;
        result
    }

    fn infer_if(&mut self, then_blk: &Block, else_blk: &Block, span: SourceSpan) -> Result<()> {
        self.pop_with_type(Type::I1, span)?;

        let mut then_state = self.branch_from();
        then_state.infer_block(then_blk)?;

        let mut else_state = self.branch_from();
        else_state.infer_block(else_blk)?;

        let inputs = merge_branch_inputs(&then_state.inputs, &else_state.inputs, span);
        self.inputs.extend(inputs.iter().cloned());

        then_state.normalize_local_inputs(&inputs);
        else_state.normalize_local_inputs(&inputs);
        then_state.prepend_missing_inputs(&inputs);
        else_state.prepend_missing_inputs(&inputs);

        if then_state.stack.len() != else_state.stack.len() {
            return Err(error::error(format!(
                "if branches leave different inferred stack depths at {span:?}: then={}, else={}",
                then_state.stack.len(),
                else_state.stack.len()
            )));
        }

        self.stack = merge_stacks(then_state.stack, else_state.stack, span);
        Ok(())
    }

    fn infer_while(&mut self, body: &Block, span: SourceSpan) -> Result<()> {
        self.pop_with_type(Type::I1, span)?;
        let base_stack = self.stack.clone();

        let mut body_state = self.branch_from();
        body_state.stack = base_stack.clone();
        body_state.infer_block(body)?;

        let inputs = body_state.inputs.clone();
        self.inputs.extend(inputs.iter().cloned());
        body_state.normalize_local_inputs(&inputs);

        let expected = inputs.len() + base_stack.len() + 1;
        if body_state.stack.len() != expected {
            return Err(error::error(format!(
                "while body must leave {expected} inferred value(s) for the next iteration at \
                 {span:?}, but left {}",
                body_state.stack.len()
            )));
        }

        let next_condition = body_state.stack.pop().unwrap();
        next_condition.constrain(Type::I1, span);
        self.stack = body_state.stack;
        Ok(())
    }

    fn unary_or_binary_felt(&mut self, inst: &Instruction, span: SourceSpan) -> Result<()> {
        use Instruction::*;

        let arity = match inst {
            Neg | ILog2 | Inv | Incr | Pow2 => 1,
            Add | Sub | Mul | Div => 2,
            _ => unreachable!("invalid felt arithmetic instruction"),
        };
        for _ in 0..arity {
            self.pop_with_type(Type::Felt, span)?;
        }
        self.push(Type::Felt);
        Ok(())
    }

    fn ext2_binary(&mut self, span: SourceSpan) -> Result<()> {
        for _ in 0..4 {
            self.pop_with_type(Type::Felt, span)?;
        }
        self.push(Type::Felt);
        self.push(Type::Felt);
        Ok(())
    }

    fn ext2_unary(&mut self, span: SourceSpan) -> Result<()> {
        for _ in 0..2 {
            self.pop_with_type(Type::Felt, span)?;
        }
        self.push(Type::Felt);
        self.push(Type::Felt);
        Ok(())
    }

    fn push_immediate(&mut self, value: &Immediate<PushValue>, _span: SourceSpan) -> Result<()> {
        let value = immediate_value(value)?;
        match value {
            PushValue::Int(IntValue::U8(_)) => self.push(Type::U8),
            PushValue::Int(IntValue::U16(_)) => self.push(Type::U16),
            PushValue::Int(IntValue::U32(_)) => self.push(Type::U32),
            PushValue::Int(IntValue::Felt(_)) | PushValue::Word(_) => self.push_word_or_felt(value),
        }
        Ok(())
    }

    fn push_word_or_felt(&mut self, value: PushValue) {
        match value {
            PushValue::Int(IntValue::Felt(_)) => self.push(Type::Felt),
            PushValue::Word(WordValue(values)) => {
                for _ in values {
                    self.push(Type::Felt);
                }
            }
            PushValue::Int(IntValue::U8(_))
            | PushValue::Int(IntValue::U16(_))
            | PushValue::Int(IntValue::U32(_)) => unreachable!("integer immediates handled above"),
        }
    }

    fn push_word_slice(
        &mut self,
        value: &Immediate<WordValue>,
        range: &Range<usize>,
        span: SourceSpan,
    ) -> Result<()> {
        let value = immediate_value(value)?;
        let Some(values) = value.0.get(range.clone()) else {
            return Err(error::error(format!(
                "invalid push word slice range {:?} at {span:?}",
                range
            )));
        };
        if values.is_empty() {
            return Err(error::error(format!(
                "empty push word slice range {:?} at {span:?}",
                range
            )));
        }
        for _ in values {
            self.push(Type::Felt);
        }
        Ok(())
    }

    fn conditional_drop(&mut self, chunk_len: usize, span: SourceSpan) -> Result<()> {
        self.pop_with_type(Type::I1, span)?;
        let if_true = self.pop_chunk(chunk_len, span);
        let if_false = self.pop_chunk(chunk_len, span);
        for (if_false, if_true) in if_false.into_iter().zip(if_true.into_iter()) {
            self.stack.push(merge_values(if_false, if_true, span));
        }
        Ok(())
    }

    fn conditional_swap(&mut self, chunk_len: usize, span: SourceSpan) -> Result<()> {
        self.pop_with_type(Type::I1, span)?;
        let if_true = self.pop_chunk(chunk_len, span);
        let if_false = self.pop_chunk(chunk_len, span);
        let mut lower = Vec::with_capacity(chunk_len);
        let mut upper = Vec::with_capacity(chunk_len);
        for (if_false, if_true) in if_false.into_iter().zip(if_true.into_iter()) {
            lower.push(merge_values(if_false.clone(), if_true.clone(), span));
            upper.push(merge_values(if_true, if_false, span));
        }
        self.stack.extend(lower);
        self.stack.extend(upper);
        Ok(())
    }

    fn load_memory_word(&mut self, pop_address: bool, span: SourceSpan) -> Result<()> {
        if pop_address {
            self.pop_with_type(Type::U32, span)?;
        }
        self.drop_n(4, span)?;
        for _ in 0..4 {
            self.push(Type::Felt);
        }
        Ok(())
    }

    fn store_memory_word(&mut self, pop_address: bool, span: SourceSpan) -> Result<()> {
        if pop_address {
            self.pop_with_type(Type::U32, span)?;
        }
        let values = self.pop_chunk(4, span);
        for value in &values {
            value.constrain(Type::Felt, span);
        }
        self.stack.extend(values);
        Ok(())
    }

    fn invoke(&mut self, target: &InvocationTarget, span: SourceSpan) -> Result<()> {
        let signature = match target {
            InvocationTarget::Symbol(name) => {
                self.signatures.get(name.as_str()).ok_or_else(|| {
                    error::error(format!(
                        "signature inference could not resolve local callee '{name}' at {span:?}"
                    ))
                })?
            }
            InvocationTarget::Path(path) => {
                let key = invocation_path_key(path.inner());
                self.external_signatures.get(&key).ok_or_else(|| {
                    error::error(format!(
                        "signature inference could not resolve external callee '{}' at {span:?}; \
                         external signature metadata is missing{}",
                        path.inner(),
                        external_signature_metadata_hint(self.external_signatures)
                    ))
                })?
            }
            InvocationTarget::MastRoot(_) => {
                return Err(error::error(format!(
                    "signature inference does not support MAST root invoke targets at {span:?}"
                )));
            }
        };

        for param in signature.params() {
            self.pop_with_type(param.ty.clone(), span)?;
        }
        for result in signature.results().iter().rev() {
            self.push(result.ty.clone());
        }
        Ok(())
    }

    fn push(&mut self, ty: Type) {
        self.stack.push(AbstractValue::typed(ty, self.current_span));
    }

    fn pop_any(&mut self, span: SourceSpan) -> Result<AbstractValue> {
        self.ensure_depth(0, span);
        Ok(self.stack.pop().unwrap())
    }

    fn pop_with_type(&mut self, ty: Type, span: SourceSpan) -> Result<AbstractValue> {
        let value = self.pop_any(span)?;
        value.constrain(ty, span);
        Ok(value)
    }

    fn pop_word_with_type(&mut self, ty: Type, span: SourceSpan) -> Result<()> {
        for _ in 0..4 {
            self.pop_with_type(ty.clone(), span)?;
        }
        Ok(())
    }

    fn pop_chunk(&mut self, chunk_len: usize, span: SourceSpan) -> Vec<AbstractValue> {
        self.ensure_depth(chunk_len - 1, span);
        masm_stack::pop_chunk(&mut self.stack, chunk_len)
            .expect("inference stack depth was extended before popping")
    }

    fn constrain_top_n(&mut self, n: usize, ty: Type, span: SourceSpan) -> Result<()> {
        self.ensure_depth(n.saturating_sub(1), span);
        let start = self.stack.len() - n;
        for value in &self.stack[start..] {
            value.constrain(ty.clone(), span);
        }
        Ok(())
    }

    fn drop_n(&mut self, n: usize, span: SourceSpan) -> Result<()> {
        for _ in 0..n {
            self.pop_any(span)?;
        }
        Ok(())
    }

    fn dup(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(depth, span);
        masm_stack::dup(&mut self.stack, depth)
            .expect("inference stack depth was extended before dup");
        Ok(())
    }

    fn dup_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(depth * 4 + 3, span);
        masm_stack::dup_word(&mut self.stack, depth)
            .expect("inference stack depth was extended before dupw");
        Ok(())
    }

    fn swap(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(depth, span);
        masm_stack::swap(&mut self.stack, depth)
            .expect("inference stack depth was extended before swap");
        Ok(())
    }

    fn swap_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.swap_chunks(4, depth, span)
    }

    fn swap_chunks(&mut self, chunk_len: usize, depth: usize, span: SourceSpan) -> Result<()> {
        let total = chunk_len * (depth + 1);
        self.ensure_depth(total - 1, span);
        masm_stack::swap_chunks(&mut self.stack, chunk_len, depth)
            .expect("inference stack depth was extended before chunk swap");
        Ok(())
    }

    fn movup(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(depth, span);
        masm_stack::movup(&mut self.stack, depth)
            .expect("inference stack depth was extended before movup");
        Ok(())
    }

    fn move_chunk_to_top(
        &mut self,
        chunk_len: usize,
        depth: usize,
        span: SourceSpan,
    ) -> Result<()> {
        let total = chunk_len * (depth + 1);
        self.ensure_depth(total - 1, span);
        masm_stack::move_chunk_to_top(&mut self.stack, chunk_len, depth)
            .expect("inference stack depth was extended before chunk movup");
        Ok(())
    }

    fn movdn(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(depth, span);
        masm_stack::movdn(&mut self.stack, depth)
            .expect("inference stack depth was extended before movdn");
        Ok(())
    }

    fn move_top_chunk_down(
        &mut self,
        chunk_len: usize,
        depth: usize,
        span: SourceSpan,
    ) -> Result<()> {
        self.ensure_depth(chunk_len * (depth + 1) - 1, span);
        masm_stack::move_top_chunk_down(&mut self.stack, chunk_len, depth)
            .expect("inference stack depth was extended before chunk movdn");
        Ok(())
    }

    fn reverse_n(&mut self, n: usize, span: SourceSpan) -> Result<()> {
        self.ensure_depth(n - 1, span);
        masm_stack::reverse_n(&mut self.stack, n)
            .expect("inference stack depth was extended before reverse");
        Ok(())
    }

    fn ensure_depth(&mut self, depth: usize, span: SourceSpan) {
        while self.stack.len() <= depth {
            let input = AbstractValue::input(self.inputs.len(), span);
            self.inputs.push(input.clone());
            self.stack.insert(0, input);
        }
    }

    fn normalize_local_inputs(&mut self, replacement_inputs: &[AbstractValue]) {
        for value in &mut self.stack {
            for (local, replacement) in self.inputs.iter().zip(replacement_inputs.iter()) {
                if value.ptr_eq(local) {
                    // Preserve a path-local refinement on stack values. The merged replacement
                    // still records the procedure input requirement, but replacing the stack value
                    // would erase facts proven by operations such as `u32assert` in one branch.
                    let local_ty = local.ty();
                    if local_ty.is_some() && local_ty != replacement.ty() {
                        break;
                    }
                    *value = replacement.clone();
                    break;
                }
            }
        }
    }

    fn prepend_missing_inputs(&mut self, replacement_inputs: &[AbstractValue]) {
        let missing = &replacement_inputs[self.inputs.len()..];
        for input in missing.iter().rev() {
            self.stack.insert(0, input.clone());
        }
    }
}

fn unsupported_instruction(inst: &Instruction, span: SourceSpan) -> Result<()> {
    debug_assert_eq!(
        semantics::instruction_semantics(inst),
        InstructionSemantics::Unsupported,
        "instruction classified as inferable reached the inference unsupported fallback: {inst:?}"
    );
    Err(error::error(format!(
        "signature inference is not implemented for MASM instruction {inst:?} at {span:?}"
    )))
}

fn refine_type(current: Option<Type>, constraint: Type) -> Option<Type> {
    match current {
        Some(current) => Some(meet_types(current, constraint)),
        None => Some(constraint),
    }
}

fn join_types(lhs: Option<Type>, rhs: Option<Type>) -> Option<Type> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(join_type(lhs, rhs)),
        (None, None) => None,
        // A value which is unconstrained on one alternative path cannot be narrowed globally by
        // the other path without inventing a path-sensitive procedure signature.
        (Some(_), None) | (None, Some(_)) => Some(Type::Felt),
    }
}

fn meet_types(lhs: Type, rhs: Type) -> Type {
    if lhs == rhs {
        return lhs;
    }

    match (masm_scalar_rank(&lhs), masm_scalar_rank(&rhs)) {
        (Some(lhs_rank), Some(rhs_rank)) => {
            if lhs_rank <= rhs_rank {
                lhs
            } else {
                rhs
            }
        }
        (Some(_), None) if rhs == Type::Unknown => lhs,
        (None, Some(_)) if lhs == Type::Unknown => rhs,
        _ => Type::Felt,
    }
}

fn join_type(lhs: Type, rhs: Type) -> Type {
    if lhs == rhs {
        return lhs;
    }

    match (masm_scalar_rank(&lhs), masm_scalar_rank(&rhs)) {
        (Some(lhs_rank), Some(rhs_rank)) => {
            if lhs_rank >= rhs_rank {
                lhs
            } else {
                rhs
            }
        }
        (Some(_), None) if rhs == Type::Unknown => lhs,
        (None, Some(_)) if lhs == Type::Unknown => rhs,
        _ => Type::Felt,
    }
}

fn masm_scalar_rank(ty: &Type) -> Option<u8> {
    match ty {
        Type::I1 => Some(0),
        Type::U8 => Some(1),
        Type::U16 => Some(2),
        Type::U32 => Some(3),
        Type::U64 => Some(4),
        Type::U128 => Some(5),
        Type::U256 => Some(6),
        Type::Felt => Some(7),
        _ => None,
    }
}

fn invocation_path_key(path: &MasmPath) -> String {
    path.to_absolute().to_string()
}

fn external_signature_metadata_hint(external_signatures: &FxHashMap<String, Signature>) -> String {
    if external_signatures.is_empty() {
        return "; no external signature metadata is available".to_string();
    }

    let mut paths = external_signatures.keys().cloned().collect::<Vec<_>>();
    paths.sort();
    let omitted = paths.len().saturating_sub(8);
    paths.truncate(8);
    let mut hint = format!("; available external signatures: {}", paths.join(", "));
    if omitted > 0 {
        hint.push_str(&format!(" (+{omitted} more)"));
    }
    hint
}

fn merge_branch_inputs(
    lhs: &[AbstractValue],
    rhs: &[AbstractValue],
    span: SourceSpan,
) -> Vec<AbstractValue> {
    let max_len = lhs.len().max(rhs.len());
    let mut merged = Vec::with_capacity(max_len);
    for index in 0..max_len {
        let value = match (lhs.get(index), rhs.get(index)) {
            (Some(lhs), Some(rhs)) => AbstractValue::branch_join(lhs, rhs, span),
            (Some(value), None) | (None, Some(value)) => value.clone(),
            (None, None) => unreachable!("index is bounded by max branch input length"),
        };
        merged.push(value);
    }
    merged
}

fn merge_stacks(
    lhs: Vec<AbstractValue>,
    rhs: Vec<AbstractValue>,
    span: SourceSpan,
) -> Vec<AbstractValue> {
    lhs.into_iter()
        .zip(rhs)
        .map(|(lhs, rhs)| AbstractValue::branch_join(&lhs, &rhs, span))
        .collect()
}

fn merge_values(lhs: AbstractValue, rhs: AbstractValue, span: SourceSpan) -> AbstractValue {
    AbstractValue::branch_join(&lhs, &rhs, span)
}

fn immediate_u32(immediate: &Immediate<u32>) -> Result<u32> {
    match immediate {
        Immediate::Value(value) => Ok(value.into_inner()),
        Immediate::Constant(name) => Err(error::error(format!(
            "unresolved repeat count constant '{name}' is not supported during signature inference"
        ))),
    }
}

fn immediate_value<T: Copy>(immediate: &Immediate<T>) -> Result<T> {
    match immediate {
        Immediate::Value(value) => Ok(value.into_inner()),
        Immediate::Constant(name) => Err(error::error(format!(
            "unresolved immediate constant '{name}' is not supported during signature inference"
        ))),
    }
}

fn validate_memory_word_address(addr: u32, span: SourceSpan) -> Result<()> {
    if addr % 4 != 0 {
        return Err(error::error(format!(
            "memory word address {addr} is not word-aligned at {span:?}"
        )));
    }
    Ok(())
}

fn validate_advice_read_count(count: u8, span: SourceSpan) -> Result<()> {
    if !(1..=16).contains(&count) {
        return Err(error::error(format!(
            "advice read count {count} is out of range at {span:?}; expected 1..=16"
        )));
    }
    Ok(())
}

fn felt_memory_pointer_type() -> Type {
    Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element))
}
