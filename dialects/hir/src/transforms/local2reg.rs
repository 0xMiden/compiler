use alloc::rc::Rc;

use midenc_hir::{
    Backward, CallOpInterface, EntityMut, FxHashMap, Op, OperationName, OperationRef, ProgramPoint,
    RawWalk, RegionBranchOpInterface, Report, Rewriter, SmallVec, Symbol, TraceTarget, ValueRef,
    dialects::builtin::{Function, attributes::LocalVariable},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, TracingRewriterListener},
    traits::BranchOpInterface,
};

use crate::{LoadLocal, StoreLocal};

#[derive(Default)]
pub struct Local2Reg;

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

        if function.num_locals() == 0 {
            log::debug!(
                target: &trace_target,
                sym = trace_target.relevant_symbol();
                "function has no locals, nothing to do",
            );
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        let locals = SmallVec::<[_; 4]>::from_iter(function.iter_locals());
        let op = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        drop(function);

        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);

        let mut loaded = FxHashMap::<LocalVariable, SmallVec<[OperationRef; 2]>>::default();
        let mut stored =
            FxHashMap::<LocalVariable, SmallVec<[(OperationRef, ValueRef); 2]>>::default();
        op.raw_postwalk_all::<Backward, _>(|op: OperationRef| {
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
                let stored_value = store.value().as_value_ref();
                let local = *store.get_local();
                log::trace!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "found store of {stored_value} to local {local} @ {}",
                    ProgramPoint::before(op),
                );
                stored.entry(local).or_default().push((op, stored_value));
            }
        });

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
                // 2. There is no control flow between the store and load, including function calls
                let load = loads[0];
                let (store, stored_value) = stores.last().unwrap();

                // 1.
                if load.parent() != store.parent() {
                    log::trace!(
                        target: &trace_target,
                        sym = trace_target.relevant_symbol();
                        "ignoring {local}: load and store are in different blocks",
                    );
                    continue;
                }

                // 2.
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
                rewriter.erase_op(*store);
                rewriter.replace_all_op_uses_with_values(load, &[Some(*stored_value)]);
                rewriter.erase_op(load);
                changed = PostPassStatus::Changed;
            } else if let Some(stores) = stored.get(&local) {
                // We've found a local which is stored to, but never loaded - these stores are all
                // dead, and can be removed.
                //
                // We rely on region simplification/canonicalization to remove any ops/values made
                // dead by erasing these stores.
                for (store, _) in stores.iter() {
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

    use litcheck_filecheck::filecheck;
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        SourceSpan, Type, ValueRef,
        dialects::builtin::{BuiltinOpBuilder, Function},
        print::AsmPrinter,
        testing::Test,
    };

    use super::Local2Reg;
    use crate::HirOpBuilder;

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
