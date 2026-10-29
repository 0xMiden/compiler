use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    Builder, BuilderExt, EntityMut, Forward, Op, OperationName, OperationRef, ProgramPoint,
    RawWalk, Region, Report, Rewriter, Spanned, ValueRef,
    dialects::{
        builtin::Function,
        debuginfo::{
            DIBuilder, DebugDeclare, DebugInfoMutation, DebugInfoUpdate, DebugKill, DebugValue,
            attributes::{Expression, ExpressionOp},
        },
    },
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{NoopRewriterListener, RegionSimplificationLevel, RewriterExt, RewriterImpl},
};

/// Deliberately correct and incorrect mutations for testing the pass-manager debug contract.
#[derive(Default)]
struct TestDebugInfoMutation {
    action: String,
}

midenc_hir::inventory::submit!(midenc_hir::pass::registry::PassInfo::new::<TestDebugInfoMutation>(
    "test-debug-info-mutation",
    "exercise checked and unchecked debug-info mutations"
));

impl Pass for TestDebugInfoMutation {
    type Target = Function;

    fn name(&self) -> &'static str {
        "test-debug-info-mutation"
    }

    fn argument(&self) -> &'static str {
        self.name()
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        self.action = options
            .strip_prefix("action=")
            .ok_or_else(|| Report::msg("test-debug-info-mutation requires action=NAME"))?
            .to_owned();
        Ok(())
    }

    fn run_on_operation(
        &mut self,
        function: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        let entry = function.entry_block();
        let body = function.body().as_region_ref();
        drop(function);
        if self.action == "simplify-aggressive" {
            let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
            Region::simplify_all(&[body], &mut rewriter, RegionSimplificationLevel::Aggressive)
                .map_err(|_| Report::msg("test requires an aggressively simplifiable region"))?;
            state.set_post_pass_status(PostPassStatus::Changed);
            return Ok(());
        }
        let arguments = entry.borrow().arguments().to_vec();
        let replacement = arguments[1] as ValueRef;
        let mut record = None;
        let mut last_kill = None;
        let mut store = None;
        root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
            let operation = op.borrow();
            if record.is_none() && (operation.is::<DebugValue>() || operation.is::<DebugDeclare>())
            {
                record = Some(op);
            }
            if operation.is::<DebugKill>() {
                last_kill = Some(op);
            }
            if operation.is::<midenc_dialect_hir::StoreLocal>() {
                store = Some(op);
            }
        });
        let mut record = record.ok_or_else(|| Report::msg("test requires a debug record"))?;
        let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
        match self.action.as_str() {
            "erase-record" => record.borrow_mut().erase(),
            "move-record" | "preserve-all" | "move-record-checked" => {
                let _debug_update = (self.action == "move-record-checked")
                    .then(|| DebugInfoMutation::new(record, DebugInfoUpdate::Preserve));
                record.borrow_mut().move_to(ProgramPoint::after(last_kill.unwrap()));
            }
            "equivalent-value" => {
                let value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                let definition = value.borrow().get_defining_op().unwrap();
                if !definition.borrow().is::<midenc_dialect_arith::Bxor>() {
                    return Err(Report::msg("equivalent-value test requires a bxor producer"));
                }
                let operands = definition
                    .borrow()
                    .operands()
                    .iter()
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<Vec<_>>();
                rewriter.set_insertion_point_before(definition);
                let equivalent = rewriter.bxor(operands[0], operands[1], definition.span())?;
                rewriter.replace_op_with_values_and_debug(
                    definition,
                    &[Some(equivalent)],
                    DebugInfoUpdate::Equivalent,
                );
            }
            "replace-value" | "late-edit" | "retroactive-equivalent" => {
                let mut value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                if matches!(self.action.as_str(), "replace-value" | "retroactive-equivalent") {
                    value.borrow_mut().replace_all_uses_with(replacement);
                } else {
                    rewriter.replace_value_with_debug(
                        value,
                        replacement,
                        DebugInfoUpdate::Equivalent,
                    );
                }
                if self.action == "late-edit" {
                    let expression = Expression::with_ops(vec![ExpressionOp::PlusUConst(1)]);
                    record
                        .borrow_mut()
                        .downcast_mut::<DebugValue>()
                        .unwrap()
                        .set_expression(expression);
                }
                if self.action == "retroactive-equivalent" {
                    let _update = DebugInfoMutation::new(record, DebugInfoUpdate::Equivalent);
                }
            }
            "erase-producer"
            | "invalidate-producer"
            | "rewrite-producer"
            | "erase-producer-equivalent" => {
                let value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                let mut definition = value.borrow().get_defining_op().unwrap();
                match self.action.as_str() {
                    "erase-producer" => definition.borrow_mut().erase(),
                    "invalidate-producer" => {
                        rewriter.erase_op_with_debug(definition, DebugInfoUpdate::Invalidate)
                    }
                    "erase-producer-equivalent" => {
                        rewriter.erase_op_with_debug(definition, DebugInfoUpdate::Equivalent)
                    }
                    _ => {
                        let mut operand = definition.borrow().operands()[1];
                        operand.borrow_mut().set(arguments[0] as ValueRef);
                    }
                }
            }
            "detached-declare-new-store"
            | "detached-declare-edit-store"
            | "detached-declare-preserve"
            | "detached-declare-prebuilt-store" => {
                let mut source = store.unwrap();
                let (local, span) = {
                    let op = source.borrow();
                    (
                        *op.downcast_ref::<midenc_dialect_hir::StoreLocal>()
                            .unwrap()
                            .local()
                            .as_value(),
                        op.span(),
                    )
                };
                let mut detached = rewriter.create_block(body, None, &[]);
                detached.borrow_mut().erase();
                if self.action == "detached-declare-prebuilt-store" {
                    source.borrow_mut().move_to(ProgramPoint::at_end_of(detached));
                }
                rewriter.set_insertion_point_to_end(detached);
                let variable = record
                    .borrow()
                    .downcast_ref::<DebugDeclare>()
                    .unwrap()
                    .variable()
                    .as_value()
                    .clone();
                rewriter.debug_declare(
                    variable,
                    Expression::with_ops(vec![ExpressionOp::LocalSlot(local.as_usize() as u32)]),
                    span,
                )?;
                let mut region = body;
                region.borrow_mut().body_mut().push_back(detached);
                if self.action == "detached-declare-new-store" {
                    rewriter.set_insertion_point_to_end(detached);
                    rewriter.create::<midenc_dialect_hir::StoreLocal, (_, _)>(span)(
                        local,
                        replacement,
                    )?;
                } else if self.action == "detached-declare-edit-store" {
                    let mut operand = source.borrow().operands()[0];
                    operand.borrow_mut().set(replacement);
                }
            }
            "retarget-local-store-salvage" => {
                let mut source = store.unwrap();
                let mut local = None;
                root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
                    if op != source
                        && let Some(store) =
                            op.borrow().downcast_ref::<midenc_dialect_hir::StoreLocal>()
                    {
                        local = Some(*store.local().as_value());
                    }
                });
                let _update = DebugInfoMutation::new(source, DebugInfoUpdate::Salvage);
                source
                    .borrow_mut()
                    .downcast_mut::<midenc_dialect_hir::StoreLocal>()
                    .unwrap()
                    .set_local(local.unwrap());
            }
            "erase-local-store" => store.unwrap().borrow_mut().erase(),
            "invalidate-local-store-without-kills" => {
                rewriter.erase_op_with_debug(store.unwrap(), DebugInfoUpdate::Invalidate);
            }
            "rewrite-local-store" => {
                let mut operand = store.unwrap().borrow().operands()[0];
                operand.borrow_mut().set(replacement);
            }
            "insert-local-store" | "insert-local-store-preserve" | "insert-local-store-salvage" => {
                let _update = match self.action.as_str() {
                    "insert-local-store-preserve" => {
                        Some(DebugInfoMutation::new(record, DebugInfoUpdate::Preserve))
                    }
                    "insert-local-store-salvage" => {
                        Some(DebugInfoMutation::new(record, DebugInfoUpdate::Salvage))
                    }
                    _ => None,
                };
                let store = store.unwrap();
                let (local, span) = {
                    let operation = store.borrow();
                    (
                        *operation
                            .downcast_ref::<midenc_dialect_hir::StoreLocal>()
                            .unwrap()
                            .local()
                            .as_value(),
                        operation.span(),
                    )
                };
                rewriter.set_insertion_point_before(record);
                rewriter.create::<midenc_dialect_hir::StoreLocal, (_, _)>(span)(
                    local,
                    replacement,
                )?;
            }
            "discard-record" => {
                let _update = DebugInfoMutation::new(record, DebugInfoUpdate::DiscardUnreachable);
                record.borrow_mut().erase();
            }
            "discard-record-erase-neighbor" => {
                let _update = DebugInfoMutation::new(record, DebugInfoUpdate::DiscardUnreachable);
                rewriter.erase_op(record.prev().unwrap());
            }
            "discard-region-move"
            | "discard-region-roundtrip"
            | "discard-block-move"
            | "discard-block-roundtrip"
            | "relocate-region"
            | "relocate-block" => {
                let mut owner = record.parent_op().unwrap();
                let update = if self.action.starts_with("relocate") {
                    DebugInfoUpdate::Relocate
                } else {
                    DebugInfoUpdate::DiscardUnreachable
                };
                let _update = DebugInfoMutation::new(owner, update);
                let iterations = if self.action.ends_with("roundtrip") {
                    2
                } else {
                    1
                };
                for _ in 0..iterations {
                    let mut owner = owner.borrow_mut();
                    let regions = owner.regions_mut();
                    if self.action.contains("region") {
                        let mut detached = regions.take().into_iter();
                        let first = detached.next().unwrap();
                        for region in detached {
                            regions.push_back(region);
                        }
                        regions.push_back(first);
                    } else {
                        let mut first = regions.front().as_pointer().unwrap();
                        let mut second = regions.back().as_pointer().unwrap();
                        let mut first = first.borrow_mut();
                        let mut second = second.borrow_mut();
                        let first_block = first.body_mut().pop_front().unwrap();
                        let second_block = second.body_mut().pop_front().unwrap();
                        first.body_mut().push_back(second_block);
                        second.body_mut().push_back(first_block);
                    }
                }
            }
            "discard-record-move"
            | "discard-record-roundtrip"
            | "discard-dependency-move"
            | "discard-dependency-roundtrip"
            | "discard-container-move"
            | "discard-container-roundtrip"
            | "relocate-record"
            | "relocate-container" => {
                let mut moved = if self.action.contains("container") {
                    record.parent_op().unwrap()
                } else {
                    record
                };
                let root = if self.action.contains("dependency") {
                    store.unwrap()
                } else {
                    moved
                };
                let original = moved
                    .prev()
                    .map(ProgramPoint::after)
                    .unwrap_or_else(|| ProgramPoint::at_start_of(moved.parent().unwrap()));
                let update = if self.action.starts_with("relocate") {
                    DebugInfoUpdate::Relocate
                } else {
                    DebugInfoUpdate::DiscardUnreachable
                };
                let _update = DebugInfoMutation::new(root, update);
                moved.borrow_mut().move_to(ProgramPoint::after(last_kill.unwrap()));
                if self.action.ends_with("roundtrip") {
                    moved.borrow_mut().move_to(original);
                }
            }
            "discard-dependency-preserve"
            | "discard-dependency-rewrite"
            | "discard-dependency-erase" => {
                let _update =
                    DebugInfoMutation::new(store.unwrap(), DebugInfoUpdate::DiscardUnreachable);
                if self.action == "discard-dependency-rewrite" {
                    record
                        .borrow_mut()
                        .downcast_mut::<DebugDeclare>()
                        .unwrap()
                        .set_expression(Expression::with_ops(vec![ExpressionOp::LocalSlot(1)]));
                } else if self.action == "discard-dependency-erase" {
                    record.borrow_mut().erase();
                }
            }
            "salvage-unavailable" => {
                let value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                midenc_hir::dialects::debuginfo::transform::salvage_debug_info(
                    &value,
                    &midenc_hir::dialects::debuginfo::transform::SalvageAction::Undef,
                    &mut rewriter,
                );
            }
            "false-preserve" => {
                let _update = DebugInfoMutation::new(record, DebugInfoUpdate::Preserve);
                let mut value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                value.borrow_mut().replace_all_uses_with(replacement);
            }
            "invalidate-without-kill" => {
                let _update = DebugInfoMutation::new(record, DebugInfoUpdate::Invalidate);
                record.borrow_mut().erase();
            }
            "invalidate-wrong-point" => {
                let _update = DebugInfoMutation::new(record, DebugInfoUpdate::Invalidate);
                let (variable, span) = {
                    let operation = record.borrow();
                    let debug = operation.downcast_ref::<DebugValue>().unwrap();
                    (debug.variable().as_value().clone(), operation.span())
                };
                rewriter.set_insertion_point_after(last_kill.unwrap());
                rewriter.debug_kill(variable, span)?;
                record.borrow_mut().erase();
            }
            "preserve-owner-roundtrip" | "equivalent-owner-roundtrip" => {
                let mut owner = record.parent_op().unwrap();
                let next = owner.next().unwrap();
                let policy = if self.action == "preserve-owner-roundtrip" {
                    DebugInfoUpdate::Preserve
                } else {
                    DebugInfoUpdate::Equivalent
                };
                let _update = DebugInfoMutation::new(owner, policy);
                owner.borrow_mut().move_to(ProgramPoint::after(next));
                owner.borrow_mut().move_to(ProgramPoint::before(next));
            }
            "invalidate-owner-anchor"
            | "salvage-owner-anchor"
            | "invalidate-unrelated-anchor"
            | "invalidate-moved-owner-anchor"
            | "salvage-moved-owner-anchor"
            | "invalidate-roundtrip-owner-anchor"
            | "salvage-roundtrip-owner-anchor"
            | "invalidate-relocated-owner-anchor"
            | "salvage-relocated-owner-anchor" => {
                let mut owner = record.parent_op().unwrap();
                let next = owner.next().unwrap();
                if self.action.contains("-relocated-") {
                    let _relocation = DebugInfoMutation::new(owner, DebugInfoUpdate::Relocate);
                    owner.borrow_mut().move_to(ProgramPoint::after(next));
                }
                let policy = if self.action.starts_with("salvage-") {
                    DebugInfoUpdate::Salvage
                } else {
                    DebugInfoUpdate::Invalidate
                };
                let mut update = DebugInfoMutation::new(owner, policy);
                if self.action.contains("-moved-") || self.action.contains("-roundtrip-") {
                    owner.borrow_mut().move_to(ProgramPoint::after(next));
                    if self.action.contains("-roundtrip-") {
                        owner.borrow_mut().move_to(ProgramPoint::before(next));
                    }
                }
                let (variable, span) = {
                    let operation = record.borrow();
                    (
                        operation
                            .downcast_ref::<DebugValue>()
                            .unwrap()
                            .variable()
                            .as_value()
                            .clone(),
                        operation.span(),
                    )
                };
                let anchor = if self.action == "invalidate-unrelated-anchor" {
                    root
                } else {
                    owner
                };
                let replacement = {
                    let mut scope = rewriter.with_inline_call_chain(record);
                    scope.set_insertion_point_before(anchor);
                    scope.debug_kill(variable, span)?.as_operation_ref()
                };
                update.replace_capture(record, replacement, anchor)?;
                owner.borrow_mut().erase();
            }
            "raw-insert" | "raw-replace-checked" => {
                let mut update = (self.action == "raw-replace-checked")
                    .then(|| DebugInfoMutation::new(record, DebugInfoUpdate::Salvage));
                let (value, variable, expression, span) = {
                    let operation = record.borrow();
                    let debug = operation.downcast_ref::<DebugValue>().unwrap();
                    (
                        debug.value().as_value_ref(),
                        debug.variable().as_value().clone(),
                        debug.expression().as_value().clone(),
                        operation.span(),
                    )
                };
                rewriter.set_insertion_point_before(record);
                let replacement =
                    rewriter.create::<DebugValue, (_, _, _)>(span)(value, variable, expression)?
                        .as_operation_ref();
                if let Some(update) = update.as_mut() {
                    update.replace_capture(record, replacement, record)?;
                    record.borrow_mut().erase();
                }
            }
            action => return Err(Report::msg(format!("unknown debug mutation action '{action}'"))),
        }
        if self.action == "preserve-all" {
            state.preserved_analyses_mut().preserve_all();
        }
        state.set_post_pass_status(PostPassStatus::Changed);
        Ok(())
    }
}
