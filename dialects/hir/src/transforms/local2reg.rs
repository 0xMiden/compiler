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
