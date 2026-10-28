use midenc_hir::{
    Visibility,
    dialects::debuginfo::{
        DebugDeclare, DebugKill, DebugValue,
        attributes::{CompileUnit, Subprogram},
    },
};

use super::*;
use crate::{
    component::test_support::world_with_core_module,
    module::debug_info::{LocalDebugInfo, LocationDescriptor},
};

fn with_builder(test: impl FnOnce(&mut FunctionBuilderExt<'_, OpBuilder<SSABuilderListener>>)) {
    let (context, _world, mut module) = world_with_core_module();
    let function = module
        .define_function(
            Ident::from("reconcile"),
            Visibility::Internal,
            Signature::new(&context, [Type::I1], []),
        )
        .unwrap();
    let state = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut builder = OpBuilder::new(context).with_listener(SSABuilderListener::new(state));
    let mut builder = FunctionBuilderExt::new(function, &mut builder);
    builder.seal_block(builder.current_block());
    test(&mut builder);
}

fn metadata(ranges: &[(u64, u64)]) -> Rc<RefCell<FunctionDebugInfo>> {
    let locals = ranges
        .iter()
        .enumerate()
        .map(|(index, &(start, end))| {
            Some(LocalDebugInfo {
                inline_calls: Vec::new(),
                attr: DebugVariable::new(
                    Symbol::intern("x"),
                    Symbol::intern("scope.rs"),
                    index as u32 + 1,
                    None,
                ),
                expression: None,
                scope: None,
                locations: vec![LocationDescriptor {
                    start,
                    end: Some(end),
                    storage: Expression::with_ops(vec![
                        ExpressionOp::ConstU64(index as u64 + 1),
                        ExpressionOp::StackValue,
                    ]),
                }],
            })
        })
        .collect();
    Rc::new(RefCell::new(FunctionDebugInfo {
        compile_unit: CompileUnit::default(),
        subprogram: Subprogram::default(),
        locals,
        function_span: None,
        location_schedule: Vec::new(),
        next_location_event: 0,
    }))
}

fn event(offset: u64, var_index: usize, available: bool) -> LocationScheduleEntry {
    LocationScheduleEntry {
        offset,
        var_index,
        storage: available.then(|| {
            Expression::with_ops(vec![
                ExpressionOp::ConstU64(var_index as u64 + 1),
                ExpressionOp::StackValue,
            ])
        }),
    }
}

fn records(block: BlockRef) -> Vec<(u32, bool)> {
    block
        .borrow()
        .body()
        .iter()
        .filter_map(|op| {
            if let Some(declare) = op.downcast_ref::<DebugDeclare>() {
                Some((declare.variable().as_value().line, true))
            } else {
                op.downcast_ref::<DebugKill>()
                    .map(|kill| (kill.variable().as_value().line, false))
            }
        })
        .collect()
}

// A repair must retain each incoming binding's kill, even though all three bindings have the
// same name. Neither a name-keyed merge nor a target-only repair is sufficient.
fn assert_shadow_join(target_offset: u64) {
    with_builder(|builder| {
        let info = metadata(&[(0, 100), (10, 20), (30, 40)]);
        info.borrow_mut().location_schedule = vec![
            event(0, 0, true),
            event(10, 0, false),
            event(10, 1, true),
            event(20, 1, false),
            event(20, 0, true),
            event(30, 0, false),
            event(30, 2, true),
            event(40, 2, false),
            event(40, 0, true),
        ];
        builder.set_debug_metadata(info);
        let first = builder.create_block();
        let second = builder.create_block();
        let target = builder.create_block();
        let condition = builder.current_block().borrow().arguments()[0] as ValueRef;
        builder.apply_location_schedule(0, SourceSpan::UNKNOWN, &[]);
        builder.cond_br(condition, first, [], second, [], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(first);
        builder.apply_location_schedule(15, SourceSpan::UNKNOWN, &[]);
        builder.br(target, [], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(second);
        builder.apply_location_schedule(35, SourceSpan::UNKNOWN, &[]);
        builder.br(target, [], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(target);
        builder.apply_location_schedule(target_offset, SourceSpan::UNKNOWN, &[]);
        builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        builder.reconcile_debug_locations().unwrap();
        assert_eq!(records(target), [(2, false), (3, false), (1, true)]);
    });
}

#[test]
fn a_join_kills_each_departing_shadow_binding_before_restoring_the_outer_binding() {
    assert_shadow_join(5);
}

#[test]
fn an_entry_prefix_restoring_the_outer_binding_keeps_other_predecessor_kills() {
    assert_shadow_join(50);
}

// Schedule flushing can emit an event older than the first reachable instruction. Both that
// delayed event and an exact-offset event already establish the target binding.
#[test]
fn entry_prefix_declarations_are_not_repeated_by_edge_repairs() {
    for event_offset in [20, 25] {
        with_builder(|builder| {
            let info = metadata(&[(event_offset, 100)]);
            info.borrow_mut().location_schedule = vec![event(event_offset, 0, true)];
            builder.set_debug_metadata(info);
            let target = builder.create_block();
            builder.apply_location_schedule(10, SourceSpan::UNKNOWN, &[]);
            builder.br(target, [], SourceSpan::UNKNOWN).unwrap();
            builder.switch_to_block(target);
            builder.apply_location_schedule(25, SourceSpan::UNKNOWN, &[]);
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
            builder.reconcile_debug_locations().unwrap();
            assert_eq!(records(target), [(1, true)], "event at {event_offset}");
        });
    }
}

fn inline_frame() -> InlineCallFrame {
    InlineCallFrame {
        name: Symbol::intern("callee"),
        linkage_name: None,
        file: Symbol::intern("callee.rs"),
        line: 1,
        column: 1,
        call_file: Symbol::intern("caller.rs"),
        call_line: 2,
        call_column: 1,
    }
}

#[test]
fn scheduled_caller_bindings_keep_their_owner_at_inlined_instructions() {
    with_builder(|builder| {
        let info = metadata(&[(0, 10)]);
        info.borrow_mut().location_schedule = vec![event(0, 0, true), event(10, 0, false)];
        builder.set_debug_metadata(info);
        builder.set_inline_calls(vec![inline_frame()]);
        builder.apply_location_schedule(0, SourceSpan::UNKNOWN, &[]);
        builder.apply_location_schedule(10, SourceSpan::UNKNOWN, &[]);
        let ret = builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        let block = builder.current_block();
        assert_eq!(records(block), [(1, true), (1, false)]);
        for operation in block.borrow().body() {
            if operation.is::<DebugDeclare>() || operation.is::<DebugKill>() {
                assert!(operation.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_none());
            }
        }
        assert!(ret.borrow().as_operation().get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_some());
    });
}

#[test]
fn repaired_caller_bindings_keep_their_owner_at_inlined_block_entries() {
    with_builder(|builder| {
        let info = metadata(&[(0, 10)]);
        info.borrow_mut().location_schedule = vec![event(0, 0, true), event(10, 0, false)];
        builder.set_debug_metadata(info);
        let target = builder.create_block();
        builder.apply_location_schedule(5, SourceSpan::UNKNOWN, &[]);
        builder.br(target, [], SourceSpan::UNKNOWN).unwrap();
        // Consume the scheduled kill before translating the target, so this edge needs a repair.
        builder.debug_info.as_ref().unwrap().borrow_mut().next_location_event = 2;
        builder.switch_to_block(target);
        builder.set_inline_calls(vec![inline_frame()]);
        builder.apply_location_schedule(15, SourceSpan::UNKNOWN, &[]);
        builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        builder.reconcile_debug_locations().unwrap();
        assert_eq!(records(target), [(1, false)]);
        let block = target.borrow();
        let first = block.body().front().get().unwrap();
        assert!(first.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_none());
        let last = block.body().back().get().unwrap();
        assert!(last.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_some());
    });
}

#[test]
fn stack_captures_of_caller_bindings_keep_their_owner_at_inlined_instructions() {
    with_builder(|builder| {
        let info = metadata(&[(0, 10)]);
        info.borrow_mut().location_schedule = vec![LocationScheduleEntry {
            offset: 0,
            var_index: 0,
            storage: Some(Expression::with_ops(vec![
                ExpressionOp::OperandStackSlot(0),
                ExpressionOp::StackValue,
            ])),
        }];
        builder.set_debug_metadata(info);
        let value = builder.imm(midenc_hir::Immediate::U32(7), SourceSpan::UNKNOWN);
        builder.set_inline_calls(vec![inline_frame()]);
        builder.apply_location_schedule(0, SourceSpan::UNKNOWN, &[value]);
        let block = builder.current_block().borrow();
        let capture = block.body().iter().find(|op| op.is::<DebugValue>()).unwrap();
        assert!(capture.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_none());
    });
}

#[test]
fn unscheduled_caller_bindings_keep_their_owner_at_inlined_local_updates() {
    with_builder(|builder| {
        let info = metadata(&[]);
        let local = metadata(&[(0, 10)]).borrow().locals[0].clone();
        info.borrow_mut().locals.push(local);
        info.borrow_mut().locals[0].as_mut().unwrap().locations.clear();
        builder.set_debug_metadata(info);
        let value = builder.imm(midenc_hir::Immediate::U32(7), SourceSpan::UNKNOWN);
        builder.set_inline_calls(vec![inline_frame()]);
        builder.emit_dbg_value_for_var(Variable::new(0), value, SourceSpan::UNKNOWN);
        let block = builder.current_block().borrow();
        let capture = block.body().iter().find(|op| op.is::<DebugValue>()).unwrap();
        assert!(capture.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME).is_none());
    });
}

#[test]
fn entry_prefix_stack_captures_are_not_invalidated_or_repeated_by_repairs() {
    with_builder(|builder| {
        let info = metadata(&[(20, 100)]);
        info.borrow_mut().location_schedule = vec![LocationScheduleEntry {
            offset: 20,
            var_index: 0,
            storage: Some(Expression::with_ops(vec![
                ExpressionOp::OperandStackSlot(0),
                ExpressionOp::StackValue,
            ])),
        }];
        builder.set_debug_metadata(info);
        let target = builder.create_block();
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        builder.apply_location_schedule(10, SourceSpan::UNKNOWN, &[]);
        builder.br(target, [], SourceSpan::UNKNOWN).unwrap();
        builder.switch_to_block(target);
        builder.apply_location_schedule(25, SourceSpan::UNKNOWN, &[value]);
        builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        builder.reconcile_debug_locations().unwrap();
        let block = target.borrow();
        let captures = block.body().iter().filter(|op| op.is::<DebugValue>()).count();
        let kills = block.body().iter().filter(|op| op.is::<DebugKill>()).count();
        assert_eq!((captures, kills), (1, 0));
    });
}
