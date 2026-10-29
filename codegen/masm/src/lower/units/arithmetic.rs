//! Source-checked constructions for arithmetic, conversions, assertions, and scalar values.
//!
//! Typed helpers construct each unit's local algorithm after its source types and attributes
//! have been validated. Their temporary stack is not the procedure's SSA stack. Input order,
//! unchanged-lane identities, helper ABIs, and effects come from closed source rules and are
//! retained separately for verification of the expanded instructions.
use alloc::{collections::BTreeSet, string::String, sync::Arc, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::{
    Immediate, Op, OpExt, Operation, Overflow, PointerType, Report, Spanned, SymbolTable, Type,
    Value,
};

use super::*;
use crate::{
    Operand, OperandStack, arith,
    emit::OpEmitter,
    hir,
    select::SelectEnv,
    ub,
    verify::{CallArgument, CallSite, LoopPosition, LoopSchema, UnitEvidence},
    wasm,
};

/// Registered arithmetic source forms; each kind has its own accepted type/attribute contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Add,
    AddOverflowing,
    Sub,
    SubOverflowing,
    Mul,
    MulOverflowing,
    Exp,
    Div,
    Mod,
    Divmod,
    And,
    Or,
    Xor,
    Band,
    Bor,
    Bxor,
    Shl,
    Shr,
    Rotl,
    Rotr,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Min,
    Max,
    Trunc,
    Zext,
    Sext,
    Incr,
    Neg,
    Inv,
    Ilog2,
    Pow2,
    Not,
    Bnot,
    IsOdd,
    Popcnt,
    Clz,
    Ctz,
    Clo,
    Cto,
    Ext2Add,
    Ext2Sub,
    Ext2Mul,
    Ext2Div,
    Ext2Neg,
    Ext2Inv,
    Cast,
    Assert,
    Assertz,
    AssertEq,
    AssertU32,
    Select,
    SignExtend,
    I32RemS,
    I64RemS,
    ConstantPointer,
    GlobalSymbol,
    Poison,
    Gep,
}

/// A snapshot of a source-checked, storage-free construction.
///
/// Build with `from_source`, then `bind` the selected atoms before using its evidence. The raw
/// body and relative lane relationships are fixed by source validation, independently of any
/// physical plan. No procedure SSA stack or allocator is retained.
pub(crate) struct Arithmetic {
    body: Vec<masm::Op>,
    /// Source operand indices in the order required by the unit's target stack convention.
    pub input_order: Vec<usize>,
    /// Source-fixed literal available for reconstruction of pointer/global/poison results.
    pub literal: Option<Immediate>,
    /// Source-derived aliases and exact helper-call contracts, completed by `bind`.
    pub proof: UnitEvidence,
    aliases: Vec<(usize, usize)>,
    call_inputs: Vec<Vec<(usize, usize)>>,
    publications: Vec<Vec<(usize, u32)>>,
    /// Source-authorized effects of this construction, including its closed helper calls.
    pub effects: LoweringEffects,
}

impl Arithmetic {
    /// Validate the registered source kind's types and attributes, then capture its algorithm.
    /// `kind` must come from this operation's registered selection rule. Unsupported tuples,
    /// conversions, overflow modes, or unresolved linked constants are reported as errors.
    pub fn from_source(kind: Kind, op: &Operation, env: &SelectEnv<'_>) -> Result<Self, Report> {
        use Kind::*;
        let inputs =
            op.operands().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let outputs =
            op.results().all().iter().map(|v| v.borrow().ty().clone()).collect::<Vec<_>>();
        let mut gep_offset_value = 0;
        let mut overflow = Overflow::Unchecked;
        let mut target = None;
        let mut source = None;
        let mut literal = None;
        let mut message: Arc<str> = "".into();
        let mut code = 0;
        macro_rules! overflow {
            ($ty:ty) => {{
                overflow = *op.downcast_ref::<$ty>().unwrap().get_overflow();
            }};
        }
        macro_rules! target {
            ($ty:ty) => {{
                target = Some(op.downcast_ref::<$ty>().unwrap().get_ty().clone());
            }};
        }
        macro_rules! assertion {
            ($ty:ty) => {{
                let a = op.downcast_ref::<$ty>().unwrap();
                message = a.get_message().as_str().into();
                code = *a.get_code();
            }};
        }
        match kind {
            Gep => {
                let gep = op.downcast_ref::<hir::GetElementPtr>().unwrap();
                gep_offset_value = gep_offset(gep)?;
                target = Some(outputs[0].clone());
            }
            Add => overflow!(arith::Add),
            Sub => overflow!(arith::Sub),
            Mul => overflow!(arith::Mul),
            AddOverflowing | SubOverflowing | MulOverflowing => overflow = Overflow::Overflowing,
            Trunc => target!(arith::Trunc),
            Zext => target!(arith::Zext),
            Sext => target!(arith::Sext),
            Cast => target!(hir::Cast),
            SignExtend => {
                let a = op.downcast_ref::<wasm::SignExtend>().unwrap();
                target = Some(a.get_dst_ty().clone());
                source = Some(a.get_src_ty().clone());
            }
            Assert => assertion!(hir::Assert),
            Assertz => assertion!(hir::Assertz),
            AssertEq => assertion!(hir::AssertEq),
            AssertU32 => assertion!(hir::AssertU32),
            ConstantPointer => {
                let a = op.downcast_ref::<hir::ConstantPointer>().unwrap();
                target = Some(Type::from(a.get_value().pointer_type().clone()));
                literal = Some(Immediate::U32(a.get_value().addr()));
            }
            Poison => {
                let a = op.downcast_ref::<ub::Poison>().unwrap();
                target = Some(a.get_value().clone());
                literal = Some(
                    a.value()
                        .as_immediate()
                        .ok_or_else(|| invalid("unsupported poison representation"))?,
                );
            }
            GlobalSymbol => {
                let a = op.downcast_ref::<midenc_hir::dialects::builtin::GlobalSymbol>().unwrap();
                let owner = a
                    .nearest_parent_op::<midenc_hir::dialects::builtin::Module>()
                    .ok_or_else(|| invalid("global symbol has no module"))?;
                let symbol = owner
                    .borrow()
                    .resolve(a.symbol().path())
                    .ok_or_else(|| invalid("unresolved global symbol"))?;
                let global = symbol.borrow();
                let global = global
                    .downcast_ref::<midenc_hir::dialects::builtin::GlobalVariable>()
                    .ok_or_else(|| invalid("global symbol is not a variable"))?;
                let global =
                    unsafe { midenc_hir::dialects::builtin::GlobalVariableRef::from_raw(global) };
                let address = env
                    .link_info
                    .ok_or_else(|| invalid("global symbol requires linked layout"))?
                    .globals_layout()
                    .get_computed_addr(global)
                    .ok_or_else(|| invalid("global symbol has no linked address"))?;
                let address = address
                    .checked_add_signed(*a.get_offset())
                    .ok_or_else(|| invalid("global symbol offset overflows"))?;
                target = Some(Type::from(PointerType::new(Type::U8)));
                literal = Some(Immediate::U32(address));
            }
            _ => {}
        }
        if matches!(kind, Add | Sub | Mul) && overflow.is_overflowing() {
            return Err(invalid("one-result arithmetic cannot carry overflowing mode").into());
        }
        let ty = inputs.first().cloned().unwrap_or(Type::Never);
        let integer = |t: &Type| t.is_integer() && *t != Type::U256;
        let same_binary = inputs.len() == 2 && inputs[0] == inputs[1];
        let unary = inputs.len() == 1;
        let unsigned_small = matches!(ty, Type::I1 | Type::U8 | Type::U16 | Type::U32);
        let ordinary = matches!(
            ty,
            Type::I1
                | Type::I8
                | Type::U8
                | Type::I16
                | Type::U16
                | Type::I32
                | Type::U32
                | Type::I64
                | Type::U64
                | Type::I128
                | Type::U128
                | Type::Felt
        );
        let mut order = (0..inputs.len()).collect::<Vec<_>>();
        let expected = match kind {
            Gep if unary => vec![target.clone().unwrap()],
            ConstantPointer | GlobalSymbol | Poison if inputs.is_empty() => {
                vec![target.clone().unwrap()]
            }
            Add | AddOverflowing | Sub | SubOverflowing | Mul | MulOverflowing
                if same_binary
                    && (matches!(
                        ty,
                        Type::Felt
                            | Type::U8
                            | Type::U16
                            | Type::U32
                            | Type::I32
                            | Type::U64
                            | Type::I64
                            | Type::U128
                            | Type::I128
                    ) || matches!(kind, Add | AddOverflowing | Sub | SubOverflowing)
                        && ty == Type::I1) =>
            {
                order.reverse();
                if overflow.is_overflowing() {
                    vec![Type::I1, ty.clone()]
                } else {
                    vec![ty.clone()]
                }
            }
            Eq | Neq if same_binary && (ordinary || ty.is_pointer()) => {
                order.reverse();
                vec![Type::I1]
            }
            Gt | Gte | Lt | Lte
                if same_binary
                    && (unsigned_small
                        || matches!(ty, Type::Felt | Type::U64 | Type::I64 | Type::I32)) =>
            {
                order.reverse();
                vec![Type::I1]
            }
            Min | Max
                if same_binary
                    && (unsigned_small || matches!(ty, Type::U64 | Type::I64 | Type::I32)) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Exp if same_binary && ty == Type::Felt => {
                order.reverse();
                vec![ty.clone()]
            }
            Div if same_binary
                && matches!(
                    ty,
                    Type::Felt
                        | Type::U8
                        | Type::U16
                        | Type::U32
                        | Type::I32
                        | Type::U64
                        | Type::I64
                ) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Mod if same_binary
                && matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::I32 | Type::U64) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Divmod if same_binary && matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64) => {
                order.reverse();
                vec![ty.clone(), ty.clone()]
            }
            And | Or | Xor if same_binary && ty == Type::I1 => {
                order.reverse();
                vec![Type::I1]
            }
            Band | Bor | Bxor if same_binary && ordinary && ty != Type::Felt => {
                order.reverse();
                vec![ty.clone()]
            }
            Shl if inputs.len() == 2
                && inputs[1] == Type::U32
                && matches!(
                    ty,
                    Type::I8
                        | Type::U8
                        | Type::I16
                        | Type::U16
                        | Type::I32
                        | Type::U32
                        | Type::I64
                        | Type::U64
                ) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Shr if inputs.len() == 2
                && inputs[1] == Type::U32
                && matches!(
                    ty,
                    Type::U8 | Type::U16 | Type::I32 | Type::U32 | Type::I64 | Type::U64
                ) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Rotl | Rotr
                if inputs.len() == 2
                    && inputs[1] == Type::U32
                    && matches!(ty, Type::I32 | Type::U32 | Type::I64 | Type::U64) =>
            {
                order.reverse();
                vec![ty.clone()]
            }
            Not if unary && ty == Type::I1 => vec![ty.clone()],
            Neg | Inv | Ilog2 if unary && ty == Type::Felt => vec![ty.clone()],
            IsOdd if unary && ordinary => vec![Type::I1],
            Bnot if unary && ordinary && ty != Type::Felt => vec![ty.clone()],
            Popcnt | Clz | Ctz | Clo | Cto if unary && ordinary && ty != Type::Felt => {
                vec![Type::U32]
            }
            Incr if unary
                && matches!(
                    ty,
                    Type::Felt
                        | Type::U8
                        | Type::I8
                        | Type::U16
                        | Type::I16
                        | Type::U32
                        | Type::I32
                        | Type::U64
                        | Type::I64
                ) =>
            {
                vec![ty.clone()]
            }
            Pow2 if unary
                && matches!(
                    ty,
                    Type::Felt
                        | Type::U8
                        | Type::U16
                        | Type::U32
                        | Type::I32
                        | Type::U64
                        | Type::I64
                ) =>
            {
                vec![ty.clone()]
            }
            Cast | Trunc | Zext | Sext
                if unary
                    && integer(&ty)
                    && target.as_ref().is_some_and(integer)
                    && conversion_supported(kind, &ty, target.as_ref().unwrap()) =>
            {
                vec![target.clone().unwrap()]
            }
            SignExtend
                if unary
                    && target.as_ref() == Some(&ty)
                    && matches!(
                        (&ty, source.as_ref()),
                        (Type::I32, Some(Type::I8 | Type::I16))
                            | (Type::I64, Some(Type::I8 | Type::I16 | Type::I32))
                    ) =>
            {
                vec![ty.clone()]
            }
            I32RemS if same_binary && ty == Type::I32 => {
                order.reverse();
                vec![ty.clone()]
            }
            I64RemS if same_binary && ty == Type::I64 => {
                order.reverse();
                vec![ty.clone()]
            }
            Assert | Assertz if unary && ordinary => vec![ty.clone()],
            AssertU32 if unary && integer(&ty) && Operand::from(ty.clone()).size() == 1 => {
                vec![Type::U32]
            }
            AssertEq if same_binary && ordinary => {
                order.reverse();
                vec![]
            }
            Select
                if inputs.len() == 3
                    && ty == Type::I1
                    && inputs[1] == inputs[2]
                    && integer(&inputs[1]) =>
            {
                vec![inputs[1].clone()]
            }
            Ext2Add | Ext2Sub | Ext2Mul | Ext2Div if inputs == vec![Type::Felt; 4] => {
                order.rotate_left(2);
                vec![Type::Felt; 2]
            }
            Ext2Neg | Ext2Inv if inputs == vec![Type::Felt; 2] => vec![Type::Felt; 2],
            _ => {
                return Err(invalid("unsupported arithmetic source type, arity or property").into());
            }
        };
        if outputs != expected {
            return Err(
                invalid("arithmetic results disagree with exact registered source rule").into()
            );
        }
        for t in &outputs {
            ValueLayout::new(t.clone())?;
        }
        let mut stack = OperandStack::new(op.context_rc());
        for &i in order.iter().rev() {
            stack.push(inputs[i].clone());
        }
        let mut invoked = BTreeSet::new();
        let mut body = vec![];
        let span = op.span();
        {
            let mut e = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            match kind {
                Gep => {
                    e.pop();
                    e.emit(
                        masm::Instruction::AddImm(Felt::new_unchecked(gep_offset_value).into()),
                        span,
                    );
                    e.push(target.clone().unwrap());
                }
                Add | AddOverflowing => e.add(overflow, span),
                Sub | SubOverflowing => e.sub(overflow, span),
                Mul | MulOverflowing => e.mul(overflow, span),
                Exp => {
                    if *op.downcast_ref::<arith::Exp>().unwrap().get_exponent_must_be_u32() {
                        e.exp_u32_exponent(span)
                    } else {
                        e.exp(span)
                    }
                }
                Div => e.checked_div(span),
                Mod => e.checked_mod(span),
                Divmod => e.checked_divmod(span),
                And => e.and(span),
                Or => e.or(span),
                Xor => e.xor(span),
                Band => e.band(span),
                Bor => e.bor(span),
                Bxor => e.bxor(span),
                Shl => e.shl(span),
                Shr => e.shr(span),
                Rotl => e.rotl(span),
                Rotr => e.rotr(span),
                Eq => e.eq(span),
                Neq => e.neq(span),
                Gt => e.gt(span),
                Gte => e.gte(span),
                Lt => e.lt(span),
                Lte => e.lte(span),
                Min => e.min(span),
                Max => e.max(span),
                Trunc => e.trunc(target.as_ref().unwrap(), span),
                Zext => e.zext(target.as_ref().unwrap(), span),
                Sext => e.sext(target.as_ref().unwrap(), span),
                Cast => e.cast(target.as_ref().unwrap(), span),
                SignExtend => {
                    e.trunc(source.as_ref().unwrap(), span);
                    e.sext(target.as_ref().unwrap(), span);
                }
                I32RemS | I64RemS => e.wrapping_mod(span),
                Incr => e.incr(span),
                Neg => e.neg(span),
                Inv => e.inv(span),
                Ilog2 => e.ilog2(span),
                Pow2 => e.pow2(span),
                Not => e.not(span),
                Bnot => e.bnot(span),
                IsOdd => e.is_odd(span),
                Popcnt => e.popcnt(span),
                Clz => e.clz(span),
                Ctz => e.ctz(span),
                Clo => e.clo(span),
                Cto => e.cto(span),
                Ext2Add => e.ext2add(span),
                Ext2Sub => e.ext2sub(span),
                Ext2Mul => e.ext2mul(span),
                Ext2Div => e.ext2div(span),
                Ext2Neg => e.ext2neg(span),
                Ext2Inv => e.ext2inv(span),
                Assert | Assertz => {
                    if kind == Assert {
                        e.assert(Some(code), Some(&message), span)
                    } else {
                        e.assertz(Some(code), Some(&message), span)
                    }
                    e.literal(small_literal(&ty, kind == Assert)?, span);
                }
                AssertEq => e.assert_eq(Some(code), Some(&message), span),
                AssertU32 => {
                    e.pop();
                    e.emit(
                        if message.is_empty() {
                            masm::Instruction::U32Assert
                        } else {
                            masm::Instruction::U32AssertWithError(message.clone().into())
                        },
                        span,
                    );
                    e.push(Type::U32);
                }
                Select => e.select(span),
                ConstantPointer | GlobalSymbol | Poison => e.literal(literal.unwrap(), span),
            }
        }
        if stack.len() != outputs.len()
            || outputs.iter().enumerate().any(|(i, t)| {
                if literal.is_some() {
                    stack[i].size() != Operand::from(t.clone()).size()
                } else {
                    stack[i].ty() != *t
                }
            })
        {
            return Err(invalid(
                "arithmetic construction did not complete its declared result tuple",
            )
            .into());
        }
        let specs = expected_calls(kind, &ty, overflow, target.as_ref());
        let actual = body
            .iter()
            .filter_map(|op| match op {
                masm::Op::Inst(inst) if matches!(inst.inner(), masm::Instruction::Exec(_)) => {
                    Some(inst.inner().clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if actual.len() != specs.len()
            || actual
                .iter()
                .zip(&specs)
                .any(|(i, s)| *i != masm::Instruction::Exec(super::super::raw::target(&s.0)))
        {
            return Err(invalid(
                "arithmetic invocation differs from registered opcode/type contract",
            )
            .into());
        }
        let mut proof = UnitEvidence::default();
        // The source tuple gate above determines which typed operations exist. Symmetry is
        // semantic even when the implementation contains helpers or checked overflow paths.
        if matches!(
            kind,
            Add | AddOverflowing
                | Mul
                | MulOverflowing
                | Eq
                | Neq
                | Min
                | Max
                | And
                | Or
                | Xor
                | Band
                | Bor
                | Bxor
        ) {
            proof.input_symmetry = InputSymmetry::CommutativePair {
                lanes_per_operand: ValueLayout::new(ty.clone())?.lanes(),
            };
        }

        if kind == Bnot && matches!(ty, Type::U64 | Type::I64) {
            proof.loops.insert(
                ControlId(0),
                LoopSchema {
                    stack: vec![LoopPosition::Carry(0), LoopPosition::Carry(1)],
                    cells: vec![],
                },
            );
        }
        for (instruction, (_, args, results)) in actual.into_iter().zip(specs) {
            let abi = ProcedureAbi::new(CallingConvention::SameContext, args, results)?;
            proof.calls.push(CallSite {
                instruction,
                abi,
                inputs: vec![CallArgument::Local; args as usize],
                path: vec![],
                truncate: false,
                outputs: vec![],
                publications: vec![],
                selector: None,
            });
        }
        let mut may_trap = !proof.calls.is_empty();
        may_trap |= raw_may_trap(&body)?;
        let aliases = alias_lanes(kind, &ty, target.as_ref(), &outputs);
        let call_inputs = proof
            .calls
            .iter()
            .enumerate()
            .map(|(site, call)| argument_lanes(kind, &ty, site, call.abi.arguments() as usize))
            .collect();
        if kind == Incr && matches!(ty, Type::U64 | Type::I64) {
            proof.calls[0].inputs[0] = CallArgument::Exact(StoredValue::Literal(Felt::ONE));
            proof.calls[0].inputs[1] = CallArgument::Exact(StoredValue::Literal(Felt::ZERO));
        }
        if kind == Cast && !proof.calls.is_empty() {
            let limit = if target.as_ref() == Some(&Type::I64) {
                i64::MAX as u64
            } else {
                0xffff_ffff_0000_0001
            };
            proof.calls[0].inputs[0] =
                CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(limit as u32 as u64)));
            proof.calls[0].inputs[1] =
                CallArgument::Exact(StoredValue::Literal(Felt::new_unchecked(limit >> 32)));
        }
        let publications = proof
            .calls
            .iter()
            .enumerate()
            .map(|(site, call)| {
                publication_lanes(kind, &ty, overflow, site, call.abi.results() as usize)
            })
            .collect();
        Ok(Self {
            body,
            input_order: order,
            literal,
            proof,
            aliases,
            call_inputs,
            publications,
            effects: LoweringEffects {
                may_trap,
                ..Default::default()
            },
        })
    }

    /// Capture a typed ABI argument extension without creating or mutating source HIR.
    /// The caller validates its parameter attributes; this shares the ordinary conversion
    /// tuple gate, instruction construction, and untouched-lane relations.
    pub fn extension(
        op: &Operation,
        source: Type,
        target: Type,
        extension: midenc_hir::dialects::builtin::attributes::ArgumentExtension,
    ) -> Result<Self, Report> {
        use midenc_hir::dialects::builtin::attributes::ArgumentExtension;
        let kind = match extension {
            ArgumentExtension::Zext => Kind::Zext,
            ArgumentExtension::Sext => Kind::Sext,
            ArgumentExtension::None => return Err(invalid("argument extension is absent").into()),
        };
        if !conversion_supported(kind, &source, &target) {
            return Err(invalid("unsupported argument extension tuple").into());
        }
        ValueLayout::new(source.clone())?;
        ValueLayout::new(target.clone())?;
        let mut stack = OperandStack::new(op.context_rc());
        stack.push(source.clone());
        let mut invoked = BTreeSet::new();
        let mut body = vec![];
        let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
        match kind {
            Kind::Zext => emitter.zext(&target, op.span()),
            Kind::Sext => emitter.sext(&target, op.span()),
            _ => unreachable!(),
        }
        let may_trap = raw_may_trap(&body)?;
        Ok(Self {
            aliases: alias_lanes(kind, &source, Some(&target), core::slice::from_ref(&target)),
            body,
            input_order: vec![0],
            literal: None,
            proof: UnitEvidence::default(),
            call_inputs: vec![],
            publications: vec![],
            effects: LoweringEffects {
                may_trap,
                ..Default::default()
            },
        })
    }

    /// Bind source atoms after the opcode/type rule has fixed all physical relationships.
    /// Inputs follow `input_order` and outputs use the source result lane order. Their lengths
    /// must match the previously validated source tuple; this step does not revalidate arity.
    pub fn bind(&mut self, inputs: &[Atom], outputs: &[Atom]) {
        self.proof.aliases =
            LaneAliases::new(self.aliases.iter().map(|&(o, i)| (outputs[o], inputs[i])).collect());
        for (call, lanes) in self.proof.calls.iter_mut().zip(&self.call_inputs) {
            for &(argument, source) in lanes {
                call.inputs[argument] = CallArgument::Exact(StoredValue::Atom(inputs[source]));
            }
        }
        for (call, lanes) in self.proof.calls.iter_mut().zip(&self.publications) {
            call.publications = lanes.iter().map(|&(o, r)| (outputs[o], r)).collect();
        }
    }

    /// Annotate the captured instructions for this unit without consulting physical locations.
    pub fn expand(&self, unit: &SelectedLowering) -> super::super::expand::AnnotatedMasmBody {
        super::super::expand::annotate_raw(&self.body, unit.id)
    }
}

fn small_literal(ty: &Type, one: bool) -> Result<Immediate, PlanError> {
    Ok(match ty {
        Type::I1 => Immediate::I1(one),
        Type::U8 => Immediate::U8(one as u8),
        Type::I8 => Immediate::I8(one as i8),
        Type::U16 => Immediate::U16(one as u16),
        Type::I16 => Immediate::I16(one as i16),
        Type::U32 => Immediate::U32(one as u32),
        Type::I32 => Immediate::I32(one as i32),
        Type::U64 => Immediate::U64(one as u64),
        Type::I64 => Immediate::I64(one as i64),
        Type::U128 => Immediate::U128(one as u128),
        Type::I128 => Immediate::I128(one as i128),
        Type::Felt => Immediate::Felt(miden_core::Felt::new_unchecked(one as u64)),
        _ => return Err(invalid("unsupported assertion type")),
    })
}

fn conversion_supported(kind: Kind, src: &Type, dst: &Type) -> bool {
    use Type::*;
    let n = dst.size_in_bits();
    let m = src.size_in_bits();
    match kind {
        Kind::Trunc => {
            src == dst
                || matches!((src,dst),(Felt,_) if n<=32)
                || matches!(src, I128 | U128) && (matches!(dst, I128 | U128 | Felt) || n <= 64)
                || matches!(src, I64 | U64) && (matches!(dst, I64 | U64 | Felt) || n <= 32)
                || matches!(src, I32 | U32) && n <= 32
                || matches!(src, I16 | U16) && n <= 16
                || matches!(src, I8 | U8) && n <= 8
        }
        Kind::Zext => {
            m <= n
                && (src == dst
                    || matches!(
                        (src, dst),
                        (I1, U8 | U16)
                            | (U8, U16)
                            | (U64, U128 | I128)
                            | (Felt, U64 | U128 | I128)
                            | (U32, U64 | I64 | U128 | I128)
                            | (I1 | U8 | U16, U32 | I32 | U64 | I64 | U128 | I128)
                            | (I1 | U8 | U16 | U32, Felt)
                    ))
        }
        Kind::Sext => {
            m <= n
                && dst.is_signed_integer()
                && (src == dst
                    || matches!(
                        (src, dst),
                        (I1, I8 | I16)
                            | (I8 | U8, I16)
                            | (U64 | I64, I128)
                            | (Felt, I64 | I128)
                            | (I32 | U32, I64 | I128)
                            | (I1 | I8 | U8 | I16 | U16, I32 | I64 | I128)
                    ))
        }
        Kind::Cast => matches!(
            (src, dst),
            (U128, I128 | I64 | U64 | Felt | U32 | I32 | U16 | I16 | U8 | I8 | I1)
                | (I128, I64 | U64 | Felt | U32 | I32 | U16 | I16 | U8 | I8 | I1)
                | (I64, I128 | U128 | U64 | Felt | U32 | I32 | U16 | I16 | U8 | I8 | I1)
                | (U64, I128 | U128 | I64 | Felt | U32 | I32 | U16 | I16 | U8 | I8 | I1)
                | (Felt, I128 | U128 | I64 | U64 | U32 | I32 | U16 | I16 | U8 | I8 | I1)
                | (U32, Felt | I64 | U64 | I128 | I32 | U16 | I16 | U8 | I8 | I1)
                | (I32, I64 | I128 | U64 | U32 | U16 | I16 | U8 | I8 | I1)
                | (I8 | I16, I32 | I64 | I128 | U32 | U64)
                | (I16, U16 | U8 | I8 | I1)
                | (I8, U8 | I1)
                | (U8 | U16, Felt)
                | (I1, _)
        ),
        _ => false,
    }
}

/// Exact intrinsic targets are selected by opcode and semantic type, never by candidate MASM.
fn expected_calls(
    kind: Kind,
    ty: &Type,
    overflow: Overflow,
    dst: Option<&Type>,
) -> Vec<(String, u32, u32)> {
    use Kind::*;
    use Type::*;
    let mut calls = vec![];
    let mut add =
        |module: &str, name: &str, a, r| calls.push((format!("::{module}::{name}"), a, r));
    let mode = if overflow.is_overflowing() {
        "overflowing"
    } else if overflow.is_checked() {
        "checked"
    } else {
        "wrapping"
    };
    match kind {
        Add | AddOverflowing | Sub | SubOverflowing | Mul | MulOverflowing => {
            let name = match kind {
                Add | AddOverflowing => "add",
                Sub | SubOverflowing => "sub",
                _ => "mul",
            };
            match ty {
                Felt if name == "mul" && (overflow.is_checked() || overflow.is_overflowing()) => {
                    add("miden::core::math::u64", "widening_mul", 4, 4)
                }
                U128 => add(
                    "miden::core::math::u128",
                    &format!(
                        "{}_{}",
                        if overflow.is_checked() || overflow.is_overflowing() {
                            "overflowing"
                        } else {
                            "wrapping"
                        },
                        name
                    ),
                    8,
                    if overflow.is_checked() || overflow.is_overflowing() {
                        5
                    } else {
                        4
                    },
                ),
                I128 if overflow.is_checked() || overflow.is_overflowing() => {
                    add("intrinsics::i128", &format!("overflowing_{name}"), 8, 5)
                }
                I128 => add("miden::core::math::u128", &format!("wrapping_{name}"), 8, 4),
                U64 => add(
                    "miden::core::math::u64",
                    &format!(
                        "{}_{}",
                        if name == "mul" && (overflow.is_checked() || overflow.is_overflowing()) {
                            "widening"
                        } else if overflow.is_checked() || overflow.is_overflowing() {
                            "overflowing"
                        } else {
                            "wrapping"
                        },
                        name
                    ),
                    4,
                    if name == "mul" && (overflow.is_checked() || overflow.is_overflowing()) {
                        4
                    } else if overflow.is_checked() || overflow.is_overflowing() {
                        3
                    } else {
                        2
                    },
                ),
                I64 if !overflow.is_checked() && !overflow.is_overflowing() && name != "mul" => {
                    add("miden::core::math::u64", &format!("wrapping_{name}"), 4, 2)
                }
                I64 => add(
                    "intrinsics::i64",
                    &format!("{mode}_{name}"),
                    4,
                    if overflow.is_overflowing() { 3 } else { 2 },
                ),
                I32 if overflow.is_checked() || overflow.is_overflowing() || name == "mul" => add(
                    "intrinsics::i32",
                    &format!("{mode}_{name}"),
                    2,
                    if overflow.is_overflowing() { 2 } else { 1 },
                ),
                _ => {}
            }
        }
        Eq | Neq if matches!(ty, U64 | I64) => {
            add("miden::core::math::u64", if kind == Eq { "eq" } else { "neq" }, 4, 1)
        }
        Gt | Gte | Lt | Lte | Min | Max => {
            let name = match kind {
                Gt => "gt",
                Gte => "gte",
                Lt => "lt",
                Lte => "lte",
                Min => "min",
                _ => "max",
            };
            let result = if matches!(kind, Min | Max) { 2 } else { 1 };
            match ty {
                U64 => add("miden::core::math::u64", name, 4, result),
                I64 => add("intrinsics::i64", name, 4, result),
                I32 => add(
                    "intrinsics::i32",
                    &if matches!(kind, Min | Max) {
                        name.into()
                    } else {
                        format!("is_{name}")
                    },
                    2,
                    1,
                ),
                _ => {}
            }
        }
        Div | Mod | Divmod | I32RemS | I64RemS => {
            let name = match kind {
                Div => "div",
                Mod => "mod",
                Divmod => "divmod",
                _ => "wrapping_mod",
            };
            match ty {
                U64 => add("miden::core::math::u64", name, 4, if kind == Divmod { 4 } else { 2 }),
                I64 => add(
                    "intrinsics::i64",
                    &if kind == I64RemS {
                        name.into()
                    } else {
                        format!("checked_{name}")
                    },
                    4,
                    2,
                ),
                I32 => add(
                    "intrinsics::i32",
                    &if kind == I32RemS {
                        name.into()
                    } else {
                        format!("checked_{name}")
                    },
                    2,
                    1,
                ),
                _ => {}
            }
        }
        Band | Bor | Bxor if matches!(ty, U64 | I64 | U128 | I128) => {
            let name = match kind {
                Band => "and",
                Bor => "or",
                _ => "xor",
            };
            add("miden::core::math::u64", name, 4, 2);
            if matches!(ty, U128 | I128) {
                add("miden::core::math::u64", name, 4, 2);
            }
        }
        Shl | Shr | Rotl | Rotr if matches!(ty, U64 | I64) => {
            let name = match kind {
                Shl => "shl",
                Shr => "shr",
                Rotl => "rotl",
                _ => "rotr",
            };
            if kind == Shr && *ty == I64 {
                add("intrinsics::i64", "checked_shr", 3, 2)
            } else {
                add("miden::core::math::u64", name, 3, 2)
            }
        }
        Shr if *ty == I32 => add("intrinsics::i32", "checked_shr", 2, 1),
        Incr if matches!(ty, U64 | I64) => add("miden::core::math::u64", "wrapping_add", 4, 2),
        Pow2 if *ty == I64 => add("intrinsics::i64", "pow2", 2, 2),
        Pow2 if *ty == I32 => add("intrinsics::i32", "pow2", 1, 1),
        Clz | Ctz | Clo | Cto if matches!(ty, U64 | I64 | U128 | I128) => {
            let name = match kind {
                Clz => "clz",
                Ctz => "ctz",
                Clo => "clo",
                _ => "cto",
            };
            add(
                if matches!(ty, U128 | I128) {
                    "miden::core::math::u128"
                } else {
                    "miden::core::math::u64"
                },
                name,
                if matches!(ty, U128 | I128) { 4 } else { 2 },
                1,
            );
        }
        Cast if *ty == U64 && dst == Some(&I64) => add("miden::core::math::u64", "lte", 4, 1),
        Cast if matches!(ty, U64 | U128 | I128) && dst == Some(&Felt) => {
            add("miden::core::math::u64", "lt", 4, 1)
        }
        _ => {}
    }
    calls
}

/// Only the native instructions in this family's immutable construction contribute effects.
/// Fixed repeats retain their actual body; open loops and branches require another family rule.
fn raw_may_trap(body: &[masm::Op]) -> Result<bool, PlanError> {
    let mut may_trap = false;
    for op in body {
        may_trap |= match op {
            masm::Op::Inst(i) if matches!(i.inner(), masm::Instruction::Exec(_)) => false,
            masm::Op::Inst(i) => TargetEffects::instruction(i.inner())?.may_trap,
            masm::Op::Repeat { body, .. } => {
                raw_may_trap(&body.iter().cloned().collect::<Vec<_>>())?
            }
            _ => return Err(invalid("unexpected structured arithmetic construction")),
        };
    }
    Ok(may_trap)
}

/// Complete untouched raw lanes, established by the conversion's source type rule.
fn alias_lanes(
    kind: Kind,
    src: &Type,
    dst: Option<&Type>,
    outputs: &[Type],
) -> Vec<(usize, usize)> {
    if !matches!(kind, Kind::Trunc | Kind::Zext | Kind::Sext | Kind::Cast | Kind::AssertU32) {
        return vec![];
    }
    let dst = dst.unwrap_or_else(|| outputs.first().unwrap_or(src));
    let n = Operand::from(src.clone()).size();
    let m = Operand::from(dst.clone()).size();
    let retain = match kind {
        Kind::AssertU32 => 1,
        Kind::Trunc
            if src == dst
                || src.size_in_bits() == dst.size_in_bits()
                    && *src != Type::Felt
                    && *dst != Type::Felt =>
        {
            n
        }
        Kind::Trunc if *src != Type::Felt && *dst != Type::Felt && dst.size_in_bits() >= 32 => m,
        Kind::Zext if src == dst || *src != Type::Felt => n,
        Kind::Sext if src == dst || *src != Type::Felt && src.size_in_bits() >= 32 => n,
        Kind::Cast
            if *src != Type::Felt
                && *dst != Type::Felt
                && dst.size_in_bits() >= 32
                && (src.size_in_bits() >= 32
                    || src.is_unsigned_integer()
                    || dst.is_unsigned_integer()) =>
        {
            n.min(m)
        }
        Kind::Cast
            if matches!(src, Type::I1 | Type::U8 | Type::U16 | Type::U32) && *dst == Type::Felt =>
        {
            1
        }
        _ => 0,
    };
    (0..retain).map(|i| (i, i)).collect()
}

/// ABI results retained verbatim at publication. Derived flags and field reductions stay local.
fn publication_lanes(
    kind: Kind,
    ty: &Type,
    overflow: Overflow,
    site: usize,
    results: usize,
) -> Vec<(usize, u32)> {
    use Kind::*;
    if matches!(kind, Cast) || *ty == Type::Felt {
        return vec![];
    }
    if matches!(kind, Band | Bor | Bxor) && matches!(ty, Type::I128 | Type::U128) {
        return (0..2).map(|i| (site * 2 + i, i as u32)).collect();
    }
    if matches!(kind, Add | AddOverflowing | Sub | SubOverflowing | Mul | MulOverflowing) {
        if *ty == Type::U64
            && matches!(kind, Mul | MulOverflowing)
            && (overflow.is_checked() || overflow.is_overflowing())
        {
            let offset = usize::from(overflow.is_overflowing());
            return (0..2).map(|i| (i + offset, i as u32)).collect();
        }
        if overflow.is_checked() && matches!(ty, Type::U64 | Type::I128 | Type::U128) {
            return (1..results).map(|r| (r - 1, r as u32)).collect();
        }
    }
    (0..results).map(|i| (i, i as u32)).collect()
}

/// Source lanes exposed by each closed call, in the helper's physical argument order.
fn argument_lanes(kind: Kind, ty: &Type, site: usize, arguments: usize) -> Vec<(usize, usize)> {
    use Kind::*;
    match kind {
        // The widening call receives split local integers; the retained native field result is
        // separately published at the unit boundary.
        Mul | MulOverflowing if *ty == Type::Felt => vec![],
        Band | Bor | Bxor if matches!(ty, Type::I128 | Type::U128) => {
            let sources = if site == 0 {
                [4, 5, 0, 1]
            } else {
                [2, 3, 6, 7]
            };
            sources.into_iter().enumerate().collect()
        }
        Incr => vec![(2, 0), (3, 1)],
        Cast => vec![(2, 0), (3, 1)],
        _ => (0..arguments).map(|i| (i, i)).collect(),
    }
}

/// Validate both nonzero arithmetic and zero-offset aliases against one source contract.
pub(crate) fn gep_offset(gep: &hir::GetElementPtr) -> Result<u64, Report> {
    let op = gep.as_operation();
    if op.num_operands() != 1 || op.num_results() != 1 {
        return Err(invalid("GEP requires one pointer operand and one result").into());
    }
    let Type::Ptr(pointer) = gep.addr().ty() else {
        return Err(invalid("GEP operand is not a pointer").into());
    };
    let (bytes, pointee) = gep.get_element()?;
    let expected = Type::from(PointerType::new_with_address_space(pointee, pointer.addrspace()));
    if gep.result().ty() != &expected {
        return Err(invalid("GEP result disagrees with selected pointee or address space").into());
    }
    let offset = if pointer.is_byte_pointer() {
        bytes
    } else {
        if !bytes.is_multiple_of(4) {
            return Err(
                invalid("element-pointer GEP requires a byte offset divisible by four").into()
            );
        }
        bytes / 4
    };
    let offset = u64::try_from(offset).map_err(|_| invalid("GEP offset exceeds address domain"))?;
    let limit = if pointer.is_byte_pointer() {
        (1u64 << 34) - 1
    } else {
        u32::MAX as u64
    };
    if offset > limit {
        return Err(invalid("GEP offset exceeds address domain").into());
    }
    Ok(offset)
}
