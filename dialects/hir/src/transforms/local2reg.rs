use alloc::rc::Rc;

use midenc_hir::{
    Backward, CallOpInterface, EntityMut, Forward, FxHashMap, FxHashSet, Op, OperationName,
    OperationRef, ProgramPoint, RawWalk, RegionBranchOpInterface, Report, Rewriter, SmallVec,
    Spanned, Symbol, TraceTarget, ValueRef,
    dialects::{
        builtin::{
            Function,
            attributes::{LocalVariable, LocalVariableAttr},
        },
        debuginfo::{
            DIBuilder, DebugDeclare, DebugValue,
            attributes::{ExpressionOp, FrameBase},
        },
    },
    effects::MemoryEffectOpInterface,
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, TracingRewriterListener},
    traits::BranchOpInterface,
};

use crate::{LoadLocal, LocalAddress, LocalByteAddress, StoreLocal};

/// The value `store` writes, read from the operation: a promotion earlier in the same run may have
/// replaced it, and the operation, not a copy taken during the scan, holds the current one.
fn stored_value(store: OperationRef) -> ValueRef {
    store
        .borrow()
        .downcast_ref::<StoreLocal>()
        .expect("only `hir.store_local` is recorded")
        .value()
        .as_value_ref()
}

/// Rewrite debug references to `local` so they follow the promoted SSA values.
///
/// When a local is promoted to an SSA value (or its dead stores are erased), the memory slot the
/// debug operation points at is no longer written, so a debugger following it would read stale or
/// uninitialized memory. Declarations become value records at each store, while existing value
/// records retain their SSA operand and drop the obsolete local-location expression. This mirrors
/// LLVM's mem2reg conversion of dbg.declare/dbg.value metadata.
///
/// Returns `false` when any debug reference to this local cannot be converted safely. In that case
/// the caller must preserve the stores.
fn convert_debug_references_for_local<R: Rewriter>(
    rewriter: &mut R,
    function_op: OperationRef,
    local: &LocalVariable,
    stores: &[OperationRef],
) -> bool {
    let local_index = local.as_usize() as u32;
    let mut declares = SmallVec::<[OperationRef; 2]>::new();
    let mut values = SmallVec::<[OperationRef; 2]>::new();
    function_op.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        let operation = op.borrow();
        let expression = if let Some(declare) = operation.downcast_ref::<DebugDeclare>() {
            Some((declare.expression(), &mut declares))
        } else {
            operation
                .downcast_ref::<DebugValue>()
                .map(|value| (value.expression(), &mut values))
        };
        if let Some((expr, references)) = expression
            && expr.as_value().operations.iter().any(|op| match op {
                ExpressionOp::LocalSlot(index) => *index == local_index,
                ExpressionOp::FrameBase {
                    base: FrameBase::LocalSlot(index),
                    ..
                } => *index == local_index,
                _ => false,
            })
        {
            references.push(op);
        }
    });

    if declares.is_empty() && values.is_empty() {
        return true;
    }

    let declares_are_safe = declares.iter().all(|declare_op| {
        matches!(
            declare_op
                .borrow()
                .downcast_ref::<DebugDeclare>()
                .unwrap()
                .expression()
                .as_value()
                .operations
                .as_slice(),
            [ExpressionOp::LocalSlot(index)] if *index == local_index
        )
    });
    let values_are_safe = values.iter().all(|value_op| {
        matches!(
            value_op
                .borrow()
                .downcast_ref::<DebugValue>()
                .unwrap()
                .expression()
                .as_value()
                .operations
                .as_slice(),
            [ExpressionOp::LocalSlot(index)] if *index == local_index
        )
    });
    if !declares_are_safe || !values_are_safe {
        return false;
    }

    for declare_op in declares {
        let variable = {
            let op = declare_op.borrow();
            op.downcast_ref::<DebugDeclare>().unwrap().variable().as_value().clone()
        };
        for store in stores {
            let span = store.borrow().span();
            rewriter.set_insertion_point_before(*store);
            let _ = rewriter.debug_value(stored_value(*store), variable.clone(), span);
        }
        rewriter.erase_op(declare_op);
    }

    for value_op in values {
        let (value, variable, span) = {
            let op = value_op.borrow();
            let debug_value = op.downcast_ref::<DebugValue>().unwrap();
            (
                debug_value.value().as_value_ref(),
                debug_value.variable().as_value().clone(),
                op.span(),
            )
        };
        rewriter.set_insertion_point_before(value_op);
        let _ = rewriter.debug_value(value, variable, span);
        rewriter.erase_op(value_op);
    }

    true
}

#[derive(Default)]
pub struct Local2Reg;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<Local2Reg>(
    "local2reg",
    "promote single-use locals to SSA registers"
));

impl Pass for Local2Reg {
    type Target = Function;

    fn name(&self) -> &'static str {
        "local2reg"
    }

    fn argument(&self) -> &'static str {
        "local2reg"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<midenc_hir::Context>) -> Result<(), Report> {
        context.get_or_register_dialect::<crate::HirDialect>();

        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let function = op.into_entity_ref();

        let trace_target = TraceTarget::category("pass")
            .with_topic(self.name())
            .with_relevant_symbol(function.name().as_str());

        log::debug!(
            target: &trace_target,
            sym = trace_target.relevant_symbol();
            "looking for locals to promote to SSA registers in: {}",
            function.as_operation()
        );

        if function.is_declaration() {
            log::debug!(
                target: &trace_target,
                sym = trace_target.relevant_symbol();
                "function has no body, nothing to do",
            );
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        let op = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        drop(function);

        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);

        let mut loaded = FxHashMap::<LocalVariable, SmallVec<[OperationRef; 2]>>::default();
        let mut stored = FxHashMap::<LocalVariable, SmallVec<[OperationRef; 2]>>::default();
        let mut pinned = FxHashSet::default();
        let function_op = op;
        op.raw_postwalk_all::<Backward, _>(|op: OperationRef| {
            // Function effects summarize the body; inspect the body's declarations directly.
            if op == function_op {
                return;
            }
            let operation = op.borrow();
            if let Some(load) = operation.downcast_ref::<LoadLocal>() {
                let local = *load.get_local();
                log::trace!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "found load from local {local} @ {}",
                    ProgramPoint::before(op),
                );
                loaded.entry(local).or_default().push(op);
            } else if let Some(store) = operation.downcast_ref::<StoreLocal>() {
                let value = store.value().as_value_ref();
                let local = *store.get_local();
                log::trace!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "found store of {value} to local {local} @ {}",
                    ProgramPoint::before(op),
                );
                stored.entry(local).or_default().push(op);
            } else if let Some(address) = operation.downcast_ref::<LocalAddress>() {
                pinned.insert(*address.get_local());
            } else if let Some(address) = operation.downcast_ref::<LocalByteAddress>() {
                pinned.insert(*address.get_local());
            } else if let Some(effects) = operation.as_trait::<dyn MemoryEffectOpInterface>() {
                for effect in effects.effects() {
                    if let Some(attribute) = effect.attribute()
                        && let Some(local) = attribute.borrow().downcast_ref::<LocalVariableAttr>()
                    {
                        pinned.insert(*local.as_value());
                    }
                }
            }
        });

        // The candidate set is derived from the load/store ops themselves rather than the
        // function's locals table: locals reconstructed from parsed HIR are not registered with
        // the function, and locals that are neither loaded nor stored have nothing to rewrite
        // anyway. Sort by index so rewrites are applied in a deterministic order.
        let mut locals = SmallVec::<[_; 4]>::from_iter(loaded.keys().chain(stored.keys()).copied());
        locals.sort_by_key(|local| local.as_usize());
        locals.dedup_by_key(|local| local.as_usize());
        // Address computations are pure, but publish pointers whose readers are invisible to
        // direct local loads. Other attribute-level memory effects also require the slot.
        locals.retain(|local| !pinned.contains(local));

        if locals.is_empty() {
            log::debug!(
                target: &trace_target,
                sym = trace_target.relevant_symbol();
                "function has no locals in use, nothing to do",
            );
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        let mut changed = PostPassStatus::Unchanged;
        'next_local: for local in locals {
            if let Some(loads) = loaded.get(&local) {
                if loads.len() > 1 {
                    // Ignore locals that are loaded multiple times for now
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: loaded more than once",
                    );
                    continue;
                }

                // If we reach here, we've found a local that is loaded only once - determine if
                // there is a corresponding store in the same block, with no intervening ops which
                // have side effects or regions.
                //
                // If there are no corresponding stores, the value being loaded is poison, but for
                // now we don't deal with that here, and instead just skip the op.
                let Some(stores) = stored.get(&local) else {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: never stored, should be poison",
                    );
                    continue;
                };

                // If the value is stored more than once, assume there is little benefit in
                // promoting it to a register
                if stores.len() > 1 {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: stored more than once",
                    );
                    continue;
                }

                // If we reach here, then we have a single load of a local stored just once - this
                // is almost certainly a case of something promotable to a SSA register. The only
                // caveat is when there is "distance" between the load and the store, where keeping
                // the load and store is actually better for codegen.
                //
                // Our heuristic then for choosing whether to promote or not is based on the
                // following criteria:
                //
                // 1. The load and store are in the same block
                // 2. The store precedes the load
                // 3. There is no control flow between the store and load, including function calls
                let load = loads[0];
                let store = *stores.last().unwrap();

                // 1.
                if load.parent() != store.parent() {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: load and store are in different blocks",
                    );
                    continue;
                }

                // 2. A load that precedes the store reads what the slot held before the store,
                // not the stored value
                if !store.borrow().is_before_in_block(&load) {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: loaded before it is stored",
                    );
                    continue;
                }

                // 3.
                let mut next_op = store.next();
                while let Some(current) = next_op.take() {
                    if current == load {
                        break;
                    }

                    next_op = current.next();

                    // Check if `curr` implements any control flow interfaces
                    let curr = current.borrow();
                    if curr.implements::<dyn BranchOpInterface>()
                        || curr.implements::<dyn RegionBranchOpInterface>()
                        || curr.implements::<dyn CallOpInterface>()
                    {
                        log::trace!(
                            target: &trace_target,
                            sym = trace_target.relevant_symbol();
                            "ignoring {local}: found control flow between load and store",
                        );
                        continue 'next_local;
                    }
                }

                log::trace!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "found promotable local {local}: erasing store and replacing load with stored value",
                );
                // If we reach here, then there is no control flow between the load and store, so
                // remove the store, and replace the load with the stored value.
                //
                // Any debug declaration pointing at the promoted slot must follow the value into
                // its SSA register, since the slot will no longer be written.
                if !convert_debug_references_for_local(&mut rewriter, op, &local, &[store]) {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: debug declarations cannot all be converted safely",
                    );
                    continue;
                }
                // Read before erasing: erasing an operation clears its operands
                let value = stored_value(store);
                rewriter.erase_op(store);
                rewriter.replace_all_op_uses_with_values(load, &[Some(value)]);
                rewriter.erase_op(load);
                changed = PostPassStatus::Changed;
            } else if let Some(stores) = stored.get(&local) {
                // We've found a local which is stored to, but never loaded - these stores are all
                // dead, and can be removed.
                //
                // We rely on region simplification/canonicalization to remove any ops/values made
                // dead by erasing these stores.
                if !convert_debug_references_for_local(&mut rewriter, op, &local, stores) {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "preserving dead stores for {local}: debug declarations cannot all be converted safely",
                    );
                    continue;
                }
                for store in stores.iter() {
                    changed = PostPassStatus::Changed;
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "found dead store for local {local}: erasing it",
                    );
                    rewriter.erase_op(*store);
                }
            } else {
                // This local is never loaded or stored - we should probably remove it, but this
                // will require visiting all of the local load/store ops and rewriting them. For
                // now we aren't doing this, but this note is left here for future reference.
            }
        }

        state.set_post_pass_status(changed);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString};

    use litcheck_filecheck::{filecheck, litcheck};
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        Op, SourceSpan, Type, ValueRef,
        dialects::builtin::{BuiltinOpBuilder, Function},
        print::AsmPrinter,
        testing::Test,
    };

    use super::Local2Reg;
    use crate::HirOpBuilder;

    #[test]
    fn preserves_stores_read_through_local_address() {
        let mut test = Test::new("address_taken", &[Type::I32], &[Type::I32]);
        let store;
        {
            let mut builder = test.function_builder();
            let local = builder.alloc_local(Type::I32);
            let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
            store = builder.store_local(local, input, SourceSpan::UNKNOWN).unwrap();
            let addr = builder.local_address(local, SourceSpan::UNKNOWN).unwrap();
            let loaded = builder.load(addr, SourceSpan::UNKNOWN).unwrap();
            builder.ret([loaded], SourceSpan::UNKNOWN).unwrap();
        }
        test.apply_pass::<Local2Reg>(true).unwrap();
        assert!(
            store.as_operation_ref().parent().is_some(),
            "pointer readers keep the store live"
        );
    }

    #[test]
    fn promotes_fpi_inputs_and_preserves_semantic_operand_order() {
        let mut test =
            Test::new("fpi_semantic_operands", &alloc::vec![Type::Felt; 7], &[Type::Felt]);
        let expected;
        let exec;
        {
            let mut builder = test.function_builder();
            expected = builder
                .entry_block()
                .borrow()
                .arguments()
                .iter()
                .map(|argument| *argument as ValueRef)
                .collect::<alloc::vec::Vec<_>>();
            let prefix = core::array::from_fn(|index| {
                let local = builder.alloc_local(Type::Felt);
                builder.store_local(local, expected[index], SourceSpan::UNKNOWN).unwrap();
                builder.load_local(local, SourceSpan::UNKNOWN).unwrap()
            });
            exec = builder.exec_fpi(prefix, [prefix[2], expected[6]], SourceSpan::UNKNOWN).unwrap();
            builder.ret([expected[6]], SourceSpan::UNKNOWN).unwrap();
        }
        test.apply_pass::<Local2Reg>(true).expect("FPI prefix locals promote normally");
        let op = exec.borrow();
        let values = |operands: midenc_hir::OpOperandRange<'_>| {
            operands
                .iter()
                .map(|operand| operand.borrow().as_value_ref())
                .collect::<alloc::vec::Vec<_>>()
        };
        assert_eq!(values(op.prefix()), expected[..6]);
        assert_eq!(values(op.inputs()), [expected[2], expected[6]]);
        test.function().borrow().as_operation().postwalk_all(|op| {
            assert!(
                op.downcast_ref::<crate::LoadLocal>().is_none()
                    && op.downcast_ref::<crate::StoreLocal>().is_none()
            );
        });
    }

    #[test]
    fn promotes_redundant_load_store_pairs() {
        let mut test = Test::new("promotes_redundant", &[Type::I32, Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let local1 = builder.alloc_local(Type::I32);
            let [v0, v1] = *builder.entry_block().borrow().arguments()[0..2].as_array().unwrap();
            let v0 = v0 as ValueRef;
            let v1 = v1 as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.load_local(local1, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @promotes_redundant(%0: i32, %1: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @promotes_redundant
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    %2 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    %3 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V4:%\d+]] = arith.add %0, %1 <{ overflow = #builtin.overflow<checked> }>;
    %4 = arith.add %2, %3 <{ overflow = #builtin.overflow<checked> }>;
    // CHECK-NEXT: builtin.ret [[V4]] : (i32);
    builtin.ret %4 : (i32);
};
            "#
        );
    }

    #[test]
    fn promotes_a_chain_of_locals_to_the_first_stored_value() {
        use alloc::vec::Vec;

        // Each chain lists, in program order, the slots the entry argument passes through: it is
        // stored to the first slot, loaded, the load stored to the next slot, and so on. Locals
        // are promoted in index order, so in every order but the descending one, a slot is
        // promoted after the slot whose load it stores: by then its store holds that load's
        // replacement, and that is the value the promotion forwards.
        for chain in [&[1, 0][..], &[0, 1], &[0, 2, 1]] {
            let mut test = Test::new("promotes_chain", &[Type::U32], &[Type::U32]);
            {
                let mut builder = test.function_builder();
                let locals =
                    chain.iter().map(|_| builder.alloc_local(Type::U32)).collect::<Vec<_>>();
                let mut value = builder.entry_block().borrow().arguments()[0] as ValueRef;
                for &slot in chain {
                    builder.store_local(locals[slot], value, SourceSpan::UNKNOWN).unwrap();
                    value = builder.load_local(locals[slot], SourceSpan::UNKNOWN).unwrap();
                }
                builder.ret([value], SourceSpan::UNKNOWN).unwrap();
            }

            test.apply_pass::<Local2Reg>(true).expect("invalid ir");

            let output = format!("{}", test.function().borrow().as_operation());
            std::println!("chain {chain:?}: {output}");
            filecheck!(
                output,
                r#"
// CHECK-LABEL: builtin.function public extern("C") @promotes_chain
// CHECK-NEXT: builtin.ret %0 : (u32);
                "#
            );
        }

        // One load stored to two slots, each loaded once
        let mut test = Test::new("promotes_fan_out", &[Type::U32], &[Type::U32]);
        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::U32);
            let local1 = builder.alloc_local(Type::U32);
            let local2 = builder.alloc_local(Type::U32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local2, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.load_local(local1, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.load_local(local2, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @promotes_fan_out
// CHECK-NEXT: [[V4:%\d+]] = arith.add %0, %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: builtin.ret [[V4]] : (u32);
            "#
        );
    }

    #[test]
    fn converts_debug_declares_for_promoted_locals() {
        use midenc_hir::{
            dialects::{
                builtin::BuiltinOpBuilder,
                debuginfo::{
                    DIBuilder, DebugInfoDialect,
                    attributes::{Expression, ExpressionOp, Variable},
                },
            },
            interner::Symbol,
        };

        let mut test = Test::new("promotes_with_declare", &[Type::I32], &[Type::I32]);
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            let variable = Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None);
            let expr = Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(0)]);
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_declare(variable, expr, SourceSpan::UNKNOWN)
                .unwrap();
            let v2 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.add(v2, v2, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v3], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @promotes_with_declare
// CHECK-NEXT: di.debug_value %0 <{ variable = #di.variable<{ name = "x", file = "test.rs", line = 1 }>, expression = #di.expression<[]> }> : (i32);
// CHECK-NEXT: [[V3:%\d+]] = arith.add %0, %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: builtin.ret [[V3]] : (i32);
// CHECK-NOT: di.debug_declare
// CHECK-NOT: hir.store_local
            "#
        );
    }

    #[test]
    fn converts_debug_declares_along_a_chain_of_locals() {
        use midenc_hir::{
            dialects::debuginfo::{
                DIBuilder, DebugInfoDialect,
                attributes::{Expression, ExpressionOp, Variable},
            },
            interner::Symbol,
        };

        // `y` is declared in slot 1, which stores slot 0's load, and promoting slot 0 replaces
        // that load. Slot 1 is promoted when it is loaded, and its store is dead when it is not;
        // either way the declaration follows the value slot 0 stored.
        for loaded in [true, false] {
            let mut test = Test::new("declares_chain", &[Type::U32], &[Type::U32]);
            test.context().get_or_register_dialect::<DebugInfoDialect>();
            {
                let mut builder = test.function_builder();
                let local0 = builder.alloc_local(Type::U32);
                let local1 = builder.alloc_local(Type::U32);
                let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
                builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
                let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
                builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
                let variable =
                    Variable::new(Symbol::intern("y"), Symbol::intern("test.rs"), 2, None);
                let expr = Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(1)]);
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_declare(variable, expr, SourceSpan::UNKNOWN)
                    .unwrap();
                let v2 = if loaded {
                    builder.load_local(local1, SourceSpan::UNKNOWN).unwrap()
                } else {
                    v0
                };
                let v3 = builder.add(v2, v0, SourceSpan::UNKNOWN).unwrap();
                builder.ret([v3], SourceSpan::UNKNOWN).unwrap();
            }

            test.apply_pass::<Local2Reg>(true).expect("invalid ir");

            let output = format!("{}", test.function().borrow().as_operation());
            std::println!("loaded = {loaded}: {output}");
            filecheck!(
                output,
                r#"
// CHECK-LABEL: builtin.function public extern("C") @declares_chain
// CHECK-NEXT: di.debug_value %0 <{ variable = #di.variable<{ name = "y", file = "test.rs", line = 2 }>, expression = #di.expression<[]> }> : (u32);
// CHECK-NEXT: [[V3:%\d+]] = arith.add %0, %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: builtin.ret [[V3]] : (u32);
// CHECK-NOT: di.debug_declare
// CHECK-NOT: hir.store_local
                "#
            );
        }
    }

    #[test]
    fn rewrites_debug_values_for_promoted_locals() {
        use midenc_hir::{
            dialects::debuginfo::{
                DIBuilder, DebugInfoDialect,
                attributes::{Expression, ExpressionOp, Variable},
            },
            interner::Symbol,
        };

        let mut test = Test::new("promotes_with_debug_value", &[Type::I32], &[Type::I32]);
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, value, SourceSpan::UNKNOWN).unwrap();
            let variable = Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None);
            let expression = Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(0)]);
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value_with_expr(value, variable, Some(expression), SourceSpan::UNKNOWN)
                .unwrap();
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @promotes_with_debug_value
// CHECK-NEXT: di.debug_value %0 <{ variable = #di.variable<{ name = "x", file = "test.rs", line = 1 }>, expression = #di.expression<[]> }> : (i32);
// CHECK-NEXT: builtin.ret %0 : (i32);
// CHECK-NOT: hir.store_local
// CHECK-NOT: DI_OP_local_slot
            "#
        );
    }

    #[test]
    fn preserves_stores_for_transformed_debug_values() {
        use midenc_hir::{
            dialects::debuginfo::{
                DIBuilder, DebugInfoDialect,
                attributes::{Expression, ExpressionOp, Variable},
            },
            interner::Symbol,
        };

        let mut test = Test::new("preserves_transformed_debug_value", &[Type::I32], &[Type::I32]);
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, value, SourceSpan::UNKNOWN).unwrap();
            let variable = Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None);
            let expression =
                Expression::with_ops(alloc::vec![ExpressionOp::LocalSlot(0), ExpressionOp::Deref,]);
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value_with_expr(value, variable, Some(expression), SourceSpan::UNKNOWN)
                .unwrap();
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @preserves_transformed_debug_value
// CHECK-NEXT: hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
// CHECK-NEXT: di.debug_value %0 <{ variable = #di.variable<{ name = "x", file = "test.rs", line = 1 }>, expression = #di.expression<[DI_OP_local_slot(0), DW_OP_deref]> }> : (i32);
            "#
        );
    }

    #[test]
    fn erases_dead_stores() {
        let mut test = Test::new("erases_dead_stores", &[Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v0], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @erases_dead_stores(%0: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @erases_dead_stores
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    // CHECK-NEXT: builtin.ret %0 : (i32);
    builtin.ret %0 : (i32);
};
            "#
        );
    }

    #[test]
    fn does_not_promote_multiply_loaded_locals() {
        let mut test = Test::new("ignores_multiple_loads", &[Type::I32, Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let local1 = builder.alloc_local(Type::I32);
            let [v0, v1] = *builder.entry_block().borrow().arguments()[0..2].as_array().unwrap();
            let v0 = v0 as ValueRef;
            let v1 = v1 as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.load_local(local1, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.load_local(local1, SourceSpan::UNKNOWN).unwrap();
            let v5 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            let v6 = builder.add(v5, v4, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v6], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @ignores_multiple_loads(%0: i32, %1: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @ignores_multiple_loads
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    // CHECK-NEXT: hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    %2 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V3:%\d+]] = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    // CHECK-NEXT: [[V4:%\d+]] = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    %3 = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    %4 = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    // CHECK-NEXT: [[V5:%\d+]] = arith.add %0, [[V3]] <{ overflow = #builtin.overflow<checked> }>;
    %5 = arith.add %2, %3 <{ overflow = #builtin.overflow<checked> }>
    // CHECK-NEXT: [[V6:%\d+]] = arith.add [[V5]], [[V4]] <{ overflow = #builtin.overflow<checked> }>
    %6 = arith.add %5, %4 <{ overflow = #builtin.overflow<checked> }>
    // CHECK-NEXT: builtin.ret [[V6]] : (i32);
    builtin.ret %6 : (i32);
};
            "#
        );
    }

    #[test]
    fn does_not_promote_multiply_stored_locals() {
        let mut test = Test::new("ignores_multiple_stores", &[Type::I32, Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let local1 = builder.alloc_local(Type::I32);
            let [v0, v1] = *builder.entry_block().borrow().arguments()[0..2].as_array().unwrap();
            let v0 = v0 as ValueRef;
            let v1 = v1 as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.load_local(local1, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local1, v1, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @ignores_multiple_stores(%0: i32, %1: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @ignores_multiple_stores
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    // CHECK-NEXT: hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    %2 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V3:%\d+]] = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    %3 = hir.load_local <{ local = #builtin.local_variable<1, i32> }>;
    // CHECK-NEXT: hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    hir.store_local %1 <{ local = #builtin.local_variable<1, i32> }> : (i32);
    // CHECK-NEXT: [[V4:%\d+]] = arith.add %0, [[V3]] <{ overflow = #builtin.overflow<checked> }>;
    %4 = arith.add %2, %3 <{ overflow = #builtin.overflow<checked> }>;
    // CHECK-NEXT: builtin.ret [[V4]] : (i32);
    builtin.ret %4 : (i32);
};
            "#
        );
    }

    #[test]
    fn does_not_promote_poison_loads() {
        let mut test = Test::new("ignores_poison_loads", &[Type::I32, Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v2 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.add(v0, v2, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v3], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @ignores_poison_loads(%0: i32, %1: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @ignores_poison_loads
    // CHECK-NEXT: [[V2:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    %2 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V3:%\d+]] = arith.add %0, [[V2]] <{ overflow = #builtin.overflow<checked> }>;
    %3 = arith.add %0, %2 <{ overflow = #builtin.overflow<checked> }>;
    // CHECK-NEXT: builtin.ret [[V3]] : (i32);
    builtin.ret %3 : (i32);
};
            "#
        );
    }

    #[test]
    fn does_not_promote_a_load_that_precedes_its_store() {
        let mut test = Test::new("ignores_load_before_store", &[Type::I32], &[Type::I32]);

        // The load reads the slot before the store writes it, and its use follows the store
        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.add(v0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local0, v2, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.mul(v1, v0, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v3, v2, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @ignores_load_before_store
// CHECK-NEXT: [[V1:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
// CHECK-NEXT: [[V2:%\d+]] = arith.add %0, %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: hir.store_local [[V2]] <{ local = #builtin.local_variable<0, i32> }> : (i32);
// CHECK-NEXT: [[V3:%\d+]] = arith.mul [[V1]], %0 <{ overflow = #builtin.overflow<checked> }>;
            "#
        );
    }

    #[test]
    fn does_not_promote_a_load_used_before_its_store() {
        let mut test = Test::new("ignores_use_before_store", &[Type::I32], &[Type::I32]);

        // The load and its use both precede the store
        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.mul(v1, v0, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.add(v0, v0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local0, v3, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @ignores_use_before_store
// CHECK-NEXT: [[V1:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
// CHECK-NEXT: [[V2:%\d+]] = arith.mul [[V1]], %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: [[V3:%\d+]] = arith.add %0, %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: hir.store_local [[V3]] <{ local = #builtin.local_variable<0, i32> }> : (i32);
            "#
        );
    }

    #[test]
    fn does_not_promote_a_load_stored_back_to_its_local() {
        let mut test = Test::new("ignores_self_store", &[Type::I32], &[Type::I32]);

        // The only store writes back the value the only load read
        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            builder.store_local(local0, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.add(v1, v0, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v2], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output = format!("{}", test.function().borrow().as_operation());
        std::println!("{output}");
        filecheck!(
            output,
            r#"
// CHECK-LABEL: builtin.function public extern("C") @ignores_self_store
// CHECK-NEXT: [[V1:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
// CHECK-NEXT: hir.store_local [[V1]] <{ local = #builtin.local_variable<0, i32> }> : (i32);
// CHECK-NEXT: [[V2:%\d+]] = arith.add [[V1]], %0 <{ overflow = #builtin.overflow<checked> }>;
// CHECK-NEXT: builtin.ret [[V2]] : (i32);
            "#
        );
    }

    #[test]
    fn does_not_promote_across_blocks() {
        use midenc_dialect_cf::ControlFlowOpBuilder;

        let mut test = Test::new("ignores_inter_block_candidates", &[Type::I32], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();

            let block1 = builder.create_block();
            builder.br(block1, None, SourceSpan::UNKNOWN).unwrap();

            builder.switch_to_block(block1);

            let v1 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v2], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(test.function().borrow());
        let output = format!("{}", printer.finish());
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @ignores_inter_block_candidates(%0: i32) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @ignores_inter_block_candidates
    // CHECK-NEXT: hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    // CHECK-NEXT: cf.br ^block1;
    cf.br ^block1
// CHECK-LABEL: ^block1:
^block1:
    // CHECK-NEXT: [[V1:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    v1 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V2:%\d+]] = arith.add %0, [[V1]] <{ overflow = #builtin.overflow<checked> }>;
    v2 = arith.add %0, %1 <{ overflow = #builtin.overflow<checked> }>;
    // CHECK-NEXT: builtin.ret [[V2]] : (i32);
    builtin.ret %2 : (i32);
};
            "#
        );
    }

    #[test]
    fn does_not_promote_across_region_control_flow() {
        use midenc_dialect_scf::StructuredControlFlowOpBuilder;
        use midenc_hir::Op;

        let mut test = Test::new("ignores_intervening_scf", &[Type::I32, Type::I1], &[Type::I32]);

        {
            let mut builder = test.function_builder();
            let local0 = builder.alloc_local(Type::I32);
            let [v0, v1] = *builder.entry_block().borrow().arguments()[0..2].as_array().unwrap();
            let v0 = v0 as ValueRef;
            let v1 = v1 as ValueRef;
            builder.store_local(local0, v0, SourceSpan::UNKNOWN).unwrap();

            let if_op = builder.r#if(v1, &[Type::I32], SourceSpan::UNKNOWN).unwrap();
            let v2 = if_op.borrow().results()[0] as ValueRef;
            let entry_block = builder.current_block();

            {
                let then_region = if_op.borrow().then_body().as_region_ref();
                let then_block = builder.create_block_in_region(then_region);
                builder.switch_to_block(then_block);
                let v3 = builder.i32(1, SourceSpan::UNKNOWN);
                builder.r#yield(Some(v3), SourceSpan::UNKNOWN).unwrap();

                let else_region = if_op.borrow().else_body().as_region_ref();
                let else_block = builder.create_block_in_region(else_region);
                builder.switch_to_block(else_block);
                let v4 = builder.i32(2, SourceSpan::UNKNOWN);
                builder.r#yield(Some(v4), SourceSpan::UNKNOWN).unwrap();
            }
            builder.switch_to_block(entry_block);

            let v5 = builder.load_local(local0, SourceSpan::UNKNOWN).unwrap();
            let v6 = builder.add(v5, v2, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v6], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<Local2Reg>(true).expect("invalid ir");

        let output =
            format!("{}", <Function as Op>::print(&test.function().borrow(), &Default::default()));
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function public extern("C") @ignores_intervening_scf(%0: i32, %1: i1) -> i32 {
// CHECK-LABEL: builtin.function public extern("C") @ignores_intervening_scf
    // CHECK-NEXT: hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    hir.store_local %0 <{ local = #builtin.local_variable<0, i32> }> : (i32);
    // CHECK-NEXT: [[V2:%\d+]] = scf.if %1 then {
    %2 = scf.if %1 then {
        // CHECK-NEXT: [[V3:%\d+]] = arith.constant 1 : i32;
        %3 = arith.constant 1 : i32;
        // CHECK-NEXT: scf.yield [[V3]] : (i32);
        scf.yield v3 : (i32);
    // CHECK-NEXT: } else {
    } else {
        // CHECK-NEXT: [[V4:%\d+]] = arith.constant 2 : i32;
        %4 = arith.constant 2 : i32;
        // CHECK-NEXT: scf.yield [[V4]] : (i32);
        scf.yield %4 : (i32);
    // CHECK-NEXT: } : (i1) -> (i32);
    } : (i1) -> (i32);
    // CHECK-NEXT: [[V5:%\d+]] = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    %5 = hir.load_local <{ local = #builtin.local_variable<0, i32> }>;
    // CHECK-NEXT: [[V6:%\d+]] = arith.add [[V5]], [[V2]] <{ overflow = #builtin.overflow<checked> }>;
    %6 = arith.add %5, %2 <{ overflow = #builtin.overflow<checked> }>;
    // CHECK-NEXT: builtin.ret [[V6]] : (i32);
    builtin.ret %6 : (i32);
};
            "#
        );
    }
}
