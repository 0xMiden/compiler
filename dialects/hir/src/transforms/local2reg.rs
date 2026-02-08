use alloc::rc::Rc;

use midenc_hir::{
    Backward, CallOpInterface, EntityMut, FxHashMap, Op, OperationName, OperationRef, ProgramPoint,
    RawWalk, RegionBranchOpInterface, Report, Rewriter, SmallVec, Symbol, TraceTarget, ValueRef,
    dialects::builtin::{Function, LocalVariable},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, TracingRewriterListener},
    traits::BranchOpInterface,
};

use crate::{LoadLocal, StoreLocal};
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

        let locals = SmallVec::<[_; 4]>::from_iter(function.iter_locals().map(|(l, _)| l));
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
                let local = *load.local();
                log::trace!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "found load from local {local} @ {}",
                    ProgramPoint::before(op),
                );
                loaded.entry(local).or_default().push(op);
            } else if let Some(store) = operation.downcast_ref::<StoreLocal>() {
                let stored_value = store.value().as_value_ref();
                let local = *store.local();
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
        'next_local: for local in locals.into_iter() {
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
    use alloc::{boxed::Box, format, rc::Rc, string::ToString};

    use litcheck_filecheck::filecheck;
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        AbiParam, Context, Ident, OpBuilder, OpPrinter, Report, Signature, SourceSpan, Type,
        ValueRef,
        dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder, FunctionRef},
        pass::PassManager,
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @promotes_redundant(v0: i32, v1: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32, v1: i32):
    hir.store_local v0 #[local = lv0];
    hir.store_local v1 #[local = lv1];
    v2 = hir.load_local : i32 #[local = lv0];
    v3 = hir.load_local : i32 #[local = lv1];
    // CHECK-NEXT: [[V4:v\d+]] = arith.add v0, v1 : i32 #[overflow = checked];
    v4 = arith.add v2, v3 : i32 #[overflow = checked]
    // CHECK-NEXT: builtin.ret [[V4]];
    builtin.ret v4;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @erases_dead_stores(v0: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32):
    hir.store_local v0 #[local = lv0];
    // CHECK-NEXT: builtin.ret v0;
    builtin.ret v0;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function @ignores_multiple_loads(v0: i32, v1: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32, v1: i32):
    hir.store_local v0 #[local = lv0];
    // CHECK-NEXT: hir.store_local v1 #[local = lv1]
    hir.store_local v1 #[local = lv1];
    v2 = hir.load_local #[local = lv0] : i32;
    // CHECK-NEXT: [[V3:v\d+]] = hir.load_local  : i32 #[local = lv1];
    // CHECK-NEXT: [[V4:v\d+]] = hir.load_local  : i32 #[local = lv1];
    v3 = hir.load_local : i32 #[local = lv1];
    v4 = hir.load_local : i32 #[local = lv1];
    // CHECK-NEXT: [[V5:v\d+]] = arith.add v0, [[V3]] : i32 #[overflow = checked];
    v5 = arith.add v2, v3 : i32 #[overflow = checked]
    // CHECK-NEXT: [[V6:v\d+]] = arith.add [[V5]], [[V4]] : i32 #[overflow = checked];
    v6 = arith.add v5, v4 : i32 #[overflow = checked]
    // CHECK-NEXT: builtin.ret [[V6]];
    builtin.ret v6;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function @ignores_multiple_stores(v0: i32, v1: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32, v1: i32):
    hir.store_local v0 #[local = lv0];
    // CHECK-NEXT: hir.store_local v1 #[local = lv1]
    hir.store_local v1 #[local = lv1];
    v2 = hir.load_local #[local = lv0] : i32;
    // CHECK-NEXT: [[V3:v\d+]] = hir.load_local  : i32 #[local = lv1];
    v3 = hir.load_local : i32 #[local = lv1];
    // CHECK-NEXT: hir.store_local v1 #[local = lv1]
    hir.store_local v1 #[local = lv1];
    // CHECK-NEXT: [[V4:v\d+]] = arith.add v0, [[V3]] : i32 #[overflow = checked];
    v4 = arith.add v2, v3 : i32 #[overflow = checked]
    // CHECK-NEXT: builtin.ret [[V4]];
    builtin.ret v4;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @ignores_poison_loads(v0: i32, v1: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32, v1: i32):
    // CHECK-NEXT: [[V2:v\d+]] = hir.load_local  : i32 #[local = lv0];
    v2 = hir.load_local  : i32 #[local = lv0];
    // CHECK-NEXT: [[V3:v\d+]] = arith.add v0, [[V2]] : i32 #[overflow = checked];
    v3 = arith.add v0, v2 : i32 #[overflow = checked]
    // CHECK-NEXT: builtin.ret [[V3]];
    builtin.ret v3;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @ignores_inter_block_candidates(v0: i32) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32):
    // CHECK-NEXT: hir.store_local v0 #[local = lv0];
    hir.store_local v0 #[local = lv0];
    // CHECK-NEXT: cf.br ^block1;
    cf.br ^block1
// CHECK-LABEL: ^block1:
^block1:
    // CHECK-NEXT: [[V1:v\d+]] = hir.load_local  : i32 #[local = lv0];
    v1 = hir.load_local : i32 #[local = lv0];
    // CHECK-NEXT: [[V2:v\d+]] = arith.add v0, [[V1]] : i32 #[overflow = checked];
    v2 = arith.add v0, v1 : i32 #[overflow = checked]
    // CHECK-NEXT: builtin.ret [[V2]];
    builtin.ret v2;
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

        test.run_local2reg(true).expect("invalid ir");

        let output =
            format!("{}", <Function as Op>::print(&test.function().borrow(), &Default::default()));
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function @ignores_intervening_scf(v0: i32, v1: i1) -> i32 {
// CHECK-LABEL: ^block0
^block0(v0: i32, v1: i1):
    // CHECK-NEXT: hir.store_local v0 #[local = lv0];
    hir.store_local v0 #[local = lv0];
    // CHECK-NEXT: [[V2:v\d+]] = scf.if v1 : i32 {
    // CHECK-NEXT: ^block{{\d+}}:
    v2 = scf.if v1 : i32 {
    ^block1:
        // CHECK-NEXT: [[V3:v\d+]] = arith.constant 1 : i32;
        v3 = arith.constant 1 : i32;
        // CHECK-NEXT: scf.yield [[V3]];
        scf.yield v3;
    // CHECK-NEXT: } else {
    // CHECK-NEXT: ^block2:
    } else {
    ^block2:
        // CHECK-NEXT: [[V4:v\d+]] = arith.constant 2 : i32;
        v4 = arith.constant 2 : i32;
        // CHECK-NEXT: scf.yield [[V4]];
        scf.yield v4;
    // CHECK-NEXT: };
    };
    // CHECK-NEXT: [[V5:v\d+]] = hir.load_local  : i32 #[local = lv0];
    v5 = hir.load_local : i32 #[local = lv0];
    // CHECK-NEXT: [[V6:v\d+]] = arith.add [[V5]], [[V2]] : i32 #[overflow = checked];
    v6 = arith.add v5, v2 : i32 #[overflow = checked];
    // CHECK-NEXT: builtin.ret [[V6]];
    builtin.ret v6;
};
            "#
        );
    }

    fn enable_compiler_instrumentation() {
        let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();
    }

    struct Test {
        context: Rc<Context>,
        builder: OpBuilder,
        function: FunctionRef,
    }

    impl Test {
        pub fn new(name: &'static str, params: &[Type], results: &[Type]) -> Self {
            enable_compiler_instrumentation();

            let context = Rc::new(Context::default());
            let mut builder = OpBuilder::new(context.clone());
            let function = builder
                .create_function(
                    Ident::with_empty_span(name.into()),
                    Signature::new(
                        params.iter().cloned().map(AbiParam::new),
                        results.iter().cloned().map(AbiParam::new),
                    ),
                )
                .unwrap();

            Self {
                context,
                builder,
                function,
            }
        }

        pub fn function(&self) -> FunctionRef {
            self.function
        }

        pub fn function_builder(&mut self) -> FunctionBuilder<'_, OpBuilder> {
            FunctionBuilder::new(self.function, &mut self.builder)
        }

        pub fn run_local2reg(&self, verify: bool) -> Result<(), Report> {
            let mut pm = PassManager::on::<Function>(
                self.context.clone(),
                midenc_hir::pass::Nesting::Explicit,
            );
            pm.add_pass(Box::new(Local2Reg));
            pm.enable_verifier(verify);
            pm.run(self.function.as_operation_ref())
        }
    }
}
