//! A checked procedure records emission without allocating HIR operations or values.
//!
//! Value and block indices belong to one procedure. Only a successfully prepared procedure
//! reaches `emit`, so MASM validation and stack simulation run exactly once.
use midenc_hir::traits::{AnyInteger, AnyPointer, AnyUnsignedInteger, TypeConstraint};

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Value(usize);

#[derive(Clone, Copy)]
pub(super) struct Local(pub u16);

impl Local {
    pub fn ty(self) -> Type {
        Type::Felt
    }
}

pub(super) struct PreparedLoop {
    pub operation: usize,
    pub before: usize,
    pub before_args: Vec<StackValue>,
    pub after: usize,
    pub after_args: Vec<StackValue>,
    pub results: Vec<StackValue>,
}

type Action = Box<dyn FnOnce(&mut Emitter<'_, '_>) -> Result<()>>;

pub(super) struct PreparedProcedure {
    actions: Vec<Action>,
    num_values: usize,
    num_blocks: usize,
    num_operations: usize,
    num_locals: u16,
    callees: Vec<(GlobalItemIndex, SourceSpan)>,
}

impl PreparedProcedure {
    pub fn callees(&self) -> &[(GlobalItemIndex, SourceSpan)] {
        &self.callees
    }

    pub fn emit(
        self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        registry: &ModuleRegistry,
    ) -> Result<()> {
        let locals = (0..self.num_locals).map(|_| builder.alloc_local(Type::Felt)).collect();
        let mut emitter = Emitter {
            builder,
            registry,
            locals,
            values: Vec::with_capacity(self.num_values),
            blocks: Vec::with_capacity(self.num_blocks),
            operations: Vec::with_capacity(self.num_operations),
        };
        for action in self.actions {
            action(&mut emitter)?;
        }
        Ok(())
    }
}

struct Emitter<'a, 'b> {
    builder: &'a mut FunctionBuilder<'b, OpBuilder>,
    registry: &'a ModuleRegistry,
    locals: Vec<LocalVariable>,
    values: Vec<ValueRef>,
    blocks: Vec<BlockRef>,
    operations: Vec<OperationRef>,
}

impl Emitter<'_, '_> {
    fn value(&self, value: Value) -> ValueRef {
        self.values[value.0]
    }

    fn set_value(&mut self, id: Value, value: ValueRef) {
        assert_eq!(id.0, self.values.len(), "prepared value emission order");
        self.values.push(value);
    }

    fn values(&self, values: &[Value]) -> Vec<ValueRef> {
        values.iter().map(|v| self.value(*v)).collect()
    }
}

#[derive(Default)]
pub(super) struct PreparationBuilder {
    actions: Vec<Action>,
    types: Vec<Type>,
    callees: Vec<(GlobalItemIndex, SourceSpan)>,
    num_blocks: usize,
    num_operations: usize,
}

macro_rules! unary {
    ($constraint:ty; $($name:ident => $ty:expr),* $(,)?) => {$ (
        pub(super) fn $name(&mut self, value: Value, span: SourceSpan) -> Result<Value> {
            self.require_type::<$constraint>(self.ty(value), stringify!($name), span)?;
            let ty = ($ty)(self.ty(value));
            Ok(self.record(vec![value], vec![ty], move |b, v| Ok(vec![b.$name(v[0], span)?]))[0])
        }
    )*};
}
macro_rules! binary {
    ($($name:ident => $ty:expr),* $(,)?) => {$ (
        pub(super) fn $name(&mut self, lhs: Value, rhs: Value, span: SourceSpan) -> Result<Value> {
            let ty = ($ty)(self.ty(lhs));
            Ok(self.record(vec![lhs, rhs], vec![ty], move |b, v| Ok(vec![b.$name(v[0], v[1], span)?]))[0])
        }
    )*};
}
macro_rules! felt_window {
    ($($name:ident => $count:expr),* $(,)?) => {$ (
        pub(super) fn $name(&mut self, values: Vec<Value>, span: SourceSpan) -> Result<Vec<Value>> {
            Ok(self.record(values, vec![Type::Felt; $count], move |b, v| Ok(b.$name(v.iter().copied(), span)?.into_iter().collect())))
        }
    )*};
}

impl PreparationBuilder {
    unary! { AnyInteger;
        bnot => Type::clone, neg => Type::clone, inv => Type::clone,
        ilog2 => Type::clone, incr => Type::clone, pow2 => Type::clone,
        not => |_| Type::I1, is_odd => |_| Type::I1,
        popcnt => |_| Type::U32, ctz => |_| Type::U32, clz => |_| Type::U32,
        clo => |_| Type::U32, cto => |_| Type::U32, assert_u32 => |_| Type::U32,
        emit_event => |_| Type::Felt,
    }

    unary! { AnyPointer; load => |_| Type::Felt }

    binary! {
        add => Type::clone, add_wrapping => Type::clone, sub_wrapping => Type::clone,
        mul => Type::clone, mul_wrapping => Type::clone, div => Type::clone,
        r#mod => Type::clone, band => Type::clone, bor => Type::clone, bxor => Type::clone,
        shr => Type::clone, shl => Type::clone, rotr => Type::clone, rotl => Type::clone,
        min => Type::clone, max => Type::clone, exp => Type::clone, exp_u32_exponent => Type::clone,
        and => |_| Type::I1, or => |_| Type::I1, xor => |_| Type::I1,
        eq => |_| Type::I1, neq => |_| Type::I1, lt => |_| Type::I1,
        lte => |_| Type::I1, gt => |_| Type::I1, gte => |_| Type::I1,
    }

    felt_window! {
        advice_pipe => 13, mem_stream => 13, mtree_get => 8, mtree_set => 8,
        mtree_merge => 4, mtree_verify => 10, crypto_stream => 14, fri_ext2fold4 => 16,
        horner_base => 16, horner_ext => 16, eval_circuit => 3, log_deferred => 12,
    }

    pub fn finish(self, num_locals: u16) -> PreparedProcedure {
        PreparedProcedure {
            actions: self.actions,
            num_values: self.types.len(),
            num_blocks: self.num_blocks,
            num_operations: self.num_operations,
            num_locals,
            callees: self.callees,
        }
    }

    pub fn ty(&self, value: Value) -> &Type {
        &self.types[value.0]
    }

    fn require_type<C: TypeConstraint>(
        &self,
        ty: &Type,
        operation: &str,
        span: SourceSpan,
    ) -> Result<()> {
        let constraint = C::get();
        if constraint.matches(ty) {
            Ok(())
        } else {
            Err(Report::msg(format!(
                "{operation} requires {} at {span:?}",
                constraint.description()
            )))
        }
    }

    fn allocate(&mut self, types: Vec<Type>) -> Vec<Value> {
        let start = self.types.len();
        self.types.extend(types);
        (start..self.types.len()).map(Value).collect()
    }

    fn record(
        &mut self,
        inputs: Vec<Value>,
        types: Vec<Type>,
        emit: impl FnOnce(&mut FunctionBuilder<'_, OpBuilder>, &[ValueRef]) -> Result<Vec<ValueRef>>
        + 'static,
    ) -> Vec<Value> {
        let outputs = self.allocate(types.clone());
        let result_ids = outputs.clone();
        self.actions.push(Box::new(move |e| {
            let inputs = e.values(&inputs);
            let results = emit(e.builder, &inputs)?;
            assert_eq!(results.len(), result_ids.len(), "prepared result count");
            for ((id, result), ty) in result_ids.into_iter().zip(results).zip(types) {
                debug_assert_eq!(result.borrow().ty(), &ty, "prepared result type");
                e.set_value(id, result);
            }
            Ok(())
        }));
        outputs
    }

    pub fn arguments(&mut self, signature: &Signature, span: SourceSpan) -> Vec<StackValue> {
        let args = self.allocate(signature.params().iter().map(|p| p.ty.clone()).collect());
        let ids = args.clone();
        self.actions.push(Box::new(move |e| {
            let entry = e.builder.entry_block();
            for (id, arg) in ids.into_iter().zip(entry.borrow().arguments()) {
                e.set_value(id, *arg as ValueRef);
            }
            Ok(())
        }));
        args.into_iter().rev().map(|value| StackValue { value, span }).collect()
    }

    pub(super) fn felt(&mut self, value: Felt, span: SourceSpan) -> Value {
        self.record(vec![], vec![Type::Felt], move |b, _| Ok(vec![b.felt(value, span)]))[0]
    }

    pub(super) fn u8(&mut self, value: u8, span: SourceSpan) -> Value {
        self.record(vec![], vec![Type::U8], move |b, _| Ok(vec![b.u8(value, span)]))[0]
    }

    pub(super) fn u16(&mut self, value: u16, span: SourceSpan) -> Value {
        self.record(vec![], vec![Type::U16], move |b, _| Ok(vec![b.u16(value, span)]))[0]
    }

    pub(super) fn u32(&mut self, value: u32, span: SourceSpan) -> Value {
        self.record(vec![], vec![Type::U32], move |b, _| Ok(vec![b.u32(value, span)]))[0]
    }

    pub(super) fn cast(&mut self, value: Value, ty: Type, span: SourceSpan) -> Result<Value> {
        self.require_type::<AnyInteger>(self.ty(value), "cast", span)?;
        self.require_type::<AnyInteger>(&ty, "cast", span)?;
        Ok(self
            .record(vec![value], vec![ty.clone()], move |b, v| Ok(vec![b.cast(v[0], ty, span)?]))
            [0])
    }

    pub(super) fn trunc(&mut self, value: Value, ty: Type, span: SourceSpan) -> Result<Value> {
        self.require_type::<AnyInteger>(self.ty(value), "trunc", span)?;
        self.require_type::<AnyInteger>(&ty, "trunc", span)?;
        Ok(self
            .record(vec![value], vec![ty.clone()], move |b, v| Ok(vec![b.trunc(v[0], ty, span)?]))
            [0])
    }

    pub(super) fn zext(&mut self, value: Value, ty: Type, span: SourceSpan) -> Result<Value> {
        self.require_type::<AnyUnsignedInteger>(self.ty(value), "zext", span)?;
        self.require_type::<AnyUnsignedInteger>(&ty, "zext", span)?;
        Ok(self
            .record(vec![value], vec![ty.clone()], move |b, v| Ok(vec![b.zext(v[0], ty, span)?]))
            [0])
    }

    pub(super) fn inttoptr(&mut self, value: Value, ty: Type, span: SourceSpan) -> Result<Value> {
        self.require_type::<AnyInteger>(self.ty(value), "inttoptr", span)?;
        self.require_type::<AnyPointer>(&ty, "inttoptr", span)?;
        Ok(self.record(vec![value], vec![ty.clone()], move |b, v| {
            Ok(vec![b.inttoptr(v[0], ty, span)?])
        })[0])
    }

    pub(super) fn unrealized_conversion_cast(
        &mut self,
        value: Value,
        ty: Type,
        span: SourceSpan,
    ) -> Result<Value> {
        Ok(self.record(vec![value], vec![ty.clone()], move |b, v| {
            Ok(vec![b.unrealized_conversion_cast(v[0], ty, span)?])
        })[0])
    }

    pub(super) fn add_overflowing(
        &mut self,
        lhs: Value,
        rhs: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let types = vec![Type::I1, self.ty(lhs).clone()];
        let results = self.record(vec![lhs, rhs], types, move |b, v| {
            let (a, b) = b.add_overflowing(v[0], v[1], span)?;
            Ok(vec![a, b])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn sub_overflowing(
        &mut self,
        lhs: Value,
        rhs: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let types = vec![Type::I1, self.ty(lhs).clone()];
        let results = self.record(vec![lhs, rhs], types, move |b, v| {
            let (a, b) = b.sub_overflowing(v[0], v[1], span)?;
            Ok(vec![a, b])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn divmod(
        &mut self,
        lhs: Value,
        rhs: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let types = vec![self.ty(lhs).clone(); 2];
        let results = self.record(vec![lhs, rhs], types, move |b, v| {
            let (a, b) = b.divmod(v[0], v[1], span)?;
            Ok(vec![a, b])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2add(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2add(v[0], v[1], v[2], v[3], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2sub(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2sub(v[0], v[1], v[2], v[3], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2mul(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2mul(v[0], v[1], v[2], v[3], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2div(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2div(v[0], v[1], v[2], v[3], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2neg(
        &mut self,
        v0: Value,
        v1: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2neg(v[0], v[1], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn ext2inv(
        &mut self,
        v0: Value,
        v1: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![v0, v1], vec![Type::Felt; 2], move |b, v| {
            let (r0, r1) = b.ext2inv(v[0], v[1], span)?;
            Ok(vec![r0, r1])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn advice_load_word(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<(Value, Value, Value, Value)> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 4], move |b, v| {
            let (r0, r1, r2, r3) = b.advice_load_word(v[0], v[1], v[2], v[3], span)?;
            Ok(vec![r0, r1, r2, r3])
        });
        Ok((results[0], results[1], results[2], results[3]))
    }

    pub(super) fn hash(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        span: SourceSpan,
    ) -> Result<Vec<Value>> {
        let results = self.record(vec![v0, v1, v2, v3], vec![Type::Felt; 4], move |b, v| {
            Ok(b.hash(v[0], v[1], v[2], v[3], span)?.into_iter().collect())
        });
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn hmerge(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        v4: Value,
        v5: Value,
        v6: Value,
        v7: Value,
        span: SourceSpan,
    ) -> Result<Vec<Value>> {
        let results =
            self.record(vec![v0, v1, v2, v3, v4, v5, v6, v7], vec![Type::Felt; 4], move |b, v| {
                Ok(b.hmerge(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], span)?
                    .into_iter()
                    .collect())
            });
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn hperm(
        &mut self,
        v0: Value,
        v1: Value,
        v2: Value,
        v3: Value,
        v4: Value,
        v5: Value,
        v6: Value,
        v7: Value,
        v8: Value,
        v9: Value,
        v10: Value,
        v11: Value,
        span: SourceSpan,
    ) -> Result<Vec<Value>> {
        let results = self.record(
            vec![v0, v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11],
            vec![Type::Felt; 12],
            move |b, v| {
                Ok(b.hperm(
                    v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11], span,
                )?
                .into_iter()
                .collect())
            },
        );
        Ok(results)
    }

    pub(super) fn caller(&mut self, span: SourceSpan) -> Result<Value> {
        Ok(self.record(
            vec![],
            vec![Type::from(midenc_hir::ArrayType::new(Type::Felt, 4))],
            move |b, _| Ok(vec![b.caller(span)?]),
        )[0])
    }

    pub(super) fn clk(&mut self, span: SourceSpan) -> Result<Value> {
        Ok(self.record(vec![], vec![Type::Felt], move |b, _| Ok(vec![b.clk(span)?]))[0])
    }

    pub(super) fn advice_pop(&mut self, span: SourceSpan) -> Result<Value> {
        Ok(self.record(vec![], vec![Type::Felt], move |b, _| Ok(vec![b.advice_pop(span)?]))[0])
    }

    pub(super) fn assert(&mut self, v0: Value, span: SourceSpan) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assert", span)?;
        self.record(vec![v0], vec![], move |b, v| {
            b.assert(v[0], span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn assertz(&mut self, v0: Value, span: SourceSpan) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assertz", span)?;
        self.record(vec![v0], vec![], move |b, v| {
            b.assertz(v[0], span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn assert_eq(&mut self, v0: Value, v1: Value, span: SourceSpan) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assert_eq", span)?;
        self.require_type::<AnyInteger>(self.ty(v1), "assert_eq", span)?;
        self.record(vec![v0, v1], vec![], move |b, v| {
            b.assert_eq(v[0], v[1], span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn assert_with_message(
        &mut self,
        v0: Value,
        message: CompactString,
        span: SourceSpan,
    ) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assert", span)?;
        self.record(vec![v0], vec![], move |b, v| {
            b.assert_with_message(v[0], message, span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn assertz_with_message(
        &mut self,
        v0: Value,
        message: CompactString,
        span: SourceSpan,
    ) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assertz", span)?;
        self.record(vec![v0], vec![], move |b, v| {
            b.assertz_with_message(v[0], message, span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn assert_eq_with_message(
        &mut self,
        v0: Value,
        v1: Value,
        message: CompactString,
        span: SourceSpan,
    ) -> Result<()> {
        self.require_type::<AnyInteger>(self.ty(v0), "assert_eq", span)?;
        self.require_type::<AnyInteger>(self.ty(v1), "assert_eq", span)?;
        self.record(vec![v0, v1], vec![], move |b, v| {
            b.assert_eq_with_message(v[0], v[1], message, span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn store(&mut self, v0: Value, v1: Value, span: SourceSpan) -> Result<()> {
        self.record(vec![v0, v1], vec![], move |b, v| {
            b.store(v[0], v[1], span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn split2(
        &mut self,
        value: Value,
        ty: Type,
        span: SourceSpan,
    ) -> Result<(Value, Value)> {
        let results = self.record(vec![value], vec![ty.clone(); 2], move |b, v| {
            let (high, low) = b.split2(v[0], ty, span)?;
            Ok(vec![high, low])
        });
        Ok((results[0], results[1]))
    }

    pub(super) fn select(
        &mut self,
        cond: Value,
        lhs: Value,
        rhs: Value,
        span: SourceSpan,
    ) -> Result<Value> {
        let ty = self.ty(lhs).clone();
        Ok(self.record(vec![cond, lhs, rhs], vec![ty], move |b, v| {
            Ok(vec![b.select(v[0], v[1], v[2], span)?])
        })[0])
    }

    pub(super) fn assert_u32_with_message(
        &mut self,
        value: Value,
        message: CompactString,
        span: SourceSpan,
    ) -> Result<Value> {
        self.require_type::<AnyInteger>(self.ty(value), "assert_u32", span)?;
        Ok(self.record(vec![value], vec![Type::U32], move |b, v| {
            Ok(vec![b.assert_u32_with_message(v[0], message, span)?])
        })[0])
    }

    pub(super) fn system_event(
        &mut self,
        values: Vec<Value>,
        event_id: Felt,
        span: SourceSpan,
    ) -> Result<Vec<Value>> {
        let count = values.len();
        Ok(self.record(values, vec![Type::Felt; count], move |b, v| {
            Ok(b.system_event(v.iter().copied(), event_id, span)?.into_iter().collect())
        }))
    }

    pub(super) fn emit_event_imm(&mut self, event_id: Felt, span: SourceSpan) -> Result<()> {
        self.record(vec![], vec![], move |b, _| {
            b.emit_event_imm(event_id, span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn mtree_verify_with_message(
        &mut self,
        values: Vec<Value>,
        message: CompactString,
        span: SourceSpan,
    ) -> Result<Vec<Value>> {
        Ok(self.record(values, vec![Type::Felt; 10], move |b, v| {
            Ok(b.mtree_verify_with_message(v.iter().copied(), message, span)?
                .into_iter()
                .collect())
        }))
    }

    pub(super) fn load_local(&mut self, local: Local, span: SourceSpan) -> Result<Value> {
        let result = self.allocate(vec![Type::Felt])[0];
        self.actions.push(Box::new(move |e| {
            let value = e.builder.load_local(e.locals[local.0 as usize], span)?;
            e.set_value(result, value);
            Ok(())
        }));
        Ok(result)
    }

    pub(super) fn local_address(&mut self, local: Local, span: SourceSpan) -> Result<Value> {
        let result = self.allocate(vec![felt_memory_pointer_type()])[0];
        self.actions.push(Box::new(move |e| {
            let value = e.builder.local_address(e.locals[local.0 as usize], span)?;
            e.set_value(result, value);
            Ok(())
        }));
        Ok(result)
    }

    pub(super) fn store_local(
        &mut self,
        local: Local,
        value: Value,
        span: SourceSpan,
    ) -> Result<()> {
        self.actions.push(Box::new(move |e| {
            e.builder.store_local(e.locals[local.0 as usize], e.value(value), span)?;
            Ok(())
        }));
        Ok(())
    }

    pub(super) fn invoke(
        &mut self,
        callee: GlobalItemIndex,
        kind: ast::InvokeKind,
        signature: Signature,
        args: Vec<Value>,
        span: SourceSpan,
    ) -> Vec<Value> {
        self.callees.push((callee, span));
        let results = self.allocate(signature.results().iter().map(|r| r.ty.clone()).collect());
        let ids = results.clone();
        self.actions.push(Box::new(move |e| {
            let function = e.registry.functions[&callee];
            let args = e.values(&args);
            let op = match kind {
                ast::InvokeKind::Exec => {
                    e.builder.exec(function, signature, args, span)?.as_operation_ref()
                }
                ast::InvokeKind::Call => {
                    e.builder.call(function, signature, args, span)?.as_operation_ref()
                }
                ast::InvokeKind::SysCall => {
                    e.builder.syscall(function, signature, args, span)?.as_operation_ref()
                }
                ast::InvokeKind::ProcRef => unreachable!("procedure references cannot be prepared"),
            };
            for (id, result) in ids.into_iter().zip(op.borrow().results().all().iter()) {
                e.set_value(id, result.borrow().as_value_ref());
            }
            Ok(())
        }));
        results
    }

    pub(super) fn ret(&mut self, values: Vec<Value>, span: SourceSpan) -> Result<()> {
        self.record(values, vec![], move |b, v| {
            b.ret(v.iter().copied(), span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn r#yield(&mut self, values: Vec<Value>, span: SourceSpan) -> Result<()> {
        self.record(values, vec![], move |b, v| {
            b.r#yield(v.iter().copied(), span)?;
            Ok(vec![])
        });
        Ok(())
    }

    pub(super) fn condition(
        &mut self,
        cond: Value,
        values: Vec<Value>,
        span: SourceSpan,
    ) -> Result<()> {
        self.actions.push(Box::new(move |e| {
            let values = e.values(&values);
            e.builder.condition(e.value(cond), values, span)?;
            Ok(())
        }));
        Ok(())
    }

    fn new_operation(&mut self) -> usize {
        let id = self.num_operations;
        self.num_operations += 1;
        id
    }

    fn new_block(&mut self) -> usize {
        let id = self.num_blocks;
        self.num_blocks += 1;
        id
    }

    pub fn begin_if(&mut self, cond: Value, span: SourceSpan) -> (usize, usize, usize) {
        let op = self.new_operation();
        let then_block = self.new_block();
        let else_block = self.new_block();
        self.actions.push(Box::new(move |e| {
            let if_op = e.builder.r#if(e.value(cond), &[], span)?;
            let then_region = if_op.borrow().then_body().as_region_ref();
            let else_region = if_op.borrow().else_body().as_region_ref();
            assert_eq!(op, e.operations.len());
            e.operations.push(if_op.as_operation_ref());
            assert_eq!(then_block, e.blocks.len());
            e.blocks.push(e.builder.create_block_in_region(then_region));
            assert_eq!(else_block, e.blocks.len());
            e.blocks.push(e.builder.create_block_in_region(else_region));
            Ok(())
        }));
        (op, then_block, else_block)
    }

    pub fn switch_to_block(&mut self, block: usize) {
        self.actions.push(Box::new(move |e| {
            e.builder.switch_to_block(e.blocks[block]);
            Ok(())
        }));
    }

    pub fn after(&mut self, op: usize) {
        self.actions.push(Box::new(move |e| {
            e.builder.builder_mut().set_insertion_point_after(e.operations[op]);
            Ok(())
        }));
    }

    pub fn append_results(
        &mut self,
        op: usize,
        types: &[Type],
        span: SourceSpan,
    ) -> Result<Vec<StackValue>> {
        if types.len() > u8::MAX as usize {
            return Err(Report::msg(format!(
                "control flow returns {} values, exceeding the HIR operand limit at {span:?}",
                types.len()
            )));
        }
        let types = types.to_vec();
        let results = self.allocate(types.clone());
        let ids = results.clone();
        self.actions.push(Box::new(move |e| {
            let mut owner = e.operations[op];
            for (index, (id, ty)) in ids.into_iter().zip(types).enumerate() {
                let result =
                    e.builder.builder().context().make_result(span, ty, owner, index as u8);
                owner.borrow_mut().results_mut().push(result);
                e.set_value(id, result.borrow().as_value_ref());
            }
            Ok(())
        }));
        Ok(results.into_iter().map(|value| StackValue { value, span }).collect())
    }

    pub fn begin_while(
        &mut self,
        inits: Vec<Value>,
        result_types: Vec<Type>,
        span: SourceSpan,
    ) -> Result<PreparedLoop> {
        if inits.len() > u8::MAX as usize {
            return Err(Report::msg(format!(
                "loop has {} inputs, exceeding the HIR operand limit at {span:?}",
                inits.len()
            )));
        }
        let init_types = inits.iter().map(|v| self.ty(*v).clone()).collect();
        let before_args = self.allocate(init_types);
        let after_args = self.allocate(result_types.clone());
        let results = self.allocate(result_types.clone());
        let op = self.new_operation();
        let before = self.new_block();
        let after = self.new_block();
        let before_ids = before_args.clone();
        let after_ids = after_args.clone();
        let result_ids = results.clone();
        self.actions.push(Box::new(move |e| {
            let inits = e.values(&inits);
            let while_op = e.builder.r#while(inits, &result_types, span)?;
            let before_block =
                while_op.borrow().before().entry_block_ref().expect("while before block");
            let after_block =
                while_op.borrow().after().entry_block_ref().expect("while after block");
            assert_eq!(op, e.operations.len());
            e.operations.push(while_op.as_operation_ref());
            assert_eq!(before, e.blocks.len());
            e.blocks.push(before_block);
            assert_eq!(after, e.blocks.len());
            e.blocks.push(after_block);
            for (ids, block) in [(before_ids, before_block), (after_ids, after_block)] {
                for (id, arg) in ids.into_iter().zip(block.borrow().arguments()) {
                    e.set_value(id, *arg as ValueRef);
                }
            }
            for (id, result) in result_ids.into_iter().zip(while_op.borrow().results().iter()) {
                e.set_value(id, result.borrow().as_value_ref());
            }
            Ok(())
        }));
        let stack = |values: Vec<Value>| {
            values.into_iter().map(|value| StackValue { value, span }).collect()
        };
        Ok(PreparedLoop {
            operation: op,
            before,
            before_args: stack(before_args),
            after,
            after_args: stack(after_args),
            results: stack(results),
        })
    }
}
