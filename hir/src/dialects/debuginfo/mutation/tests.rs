use alloc::{boxed::Box, string::ToString};

use super::{DebugInfoAudit, DebugInfoMutation, DebugInfoUpdate};
use crate::{
    Builder, EntityMut, Forward, Op, OperationName, OperationRef, RawWalk, Report, SourceSpan,
    Type, ValueRef,
    dialects::{
        builtin::{BuiltinOpBuilder, Function},
        debuginfo::{DIBuilder, DebugInfoDialect, DebugValue, attributes::Variable},
    },
    interner::Symbol,
    pass::{Pass, PassExecutionState},
    testing::Test,
};

struct SilentDebugDrop {
    insert_first: bool,
}

impl Pass for SilentDebugDrop {
    type Target = Function;

    fn name(&self) -> &'static str {
        "silent-debug-drop"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        function: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        let block = function.entry_block();
        drop(function);
        let mut record = None;
        if self.insert_first {
            let value = block.borrow().arguments()[0] as ValueRef;
            let mut builder = context.builder();
            builder.set_insertion_point_before(block.borrow().body().back().as_pointer().unwrap());
            let created = builder.debug_value(
                value,
                Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None),
                SourceSpan::UNKNOWN,
            )?;
            record = Some(created.as_operation_ref());
        } else {
            root.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
                if op.borrow().is::<DebugValue>() {
                    record = Some(op);
                }
            });
        }
        record.unwrap().borrow_mut().erase();
        state.preserved_analyses_mut().preserve_all();
        Ok(())
    }
}

#[test]
fn debug_mutation_audit_is_independent_of_ir_verification_and_preserved_analyses() {
    for verify in [false, true] {
        let mut test = Test::new("unchecked_debug_drop", &[Type::U32], &[Type::U32]);
        test.context().get_or_register_dialect::<DebugInfoDialect>();
        {
            let mut builder = test.function_builder();
            let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value(
                    value,
                    Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None),
                    SourceSpan::UNKNOWN,
                )
                .unwrap();
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }
        let error = test
            .apply_boxed_pass(
                Box::new(SilentDebugDrop {
                    insert_first: false,
                }),
                verify,
            )
            .unwrap_err();
        assert!(error.to_string().contains("unhandled debug-info mutation"), "{error}");
        let context = test.context_rc();
        let tracking = context.debug_info_mutations.borrow();
        assert!(tracking.audits.is_empty());
        assert!(tracking.permissions.is_empty());
    }
}

#[test]
fn newly_inserted_debug_records_are_audited_in_passes_starting_without_debug_info() {
    let mut test = Test::new("insert_then_drop", &[Type::U32], &[Type::U32]);
    test.context().get_or_register_dialect::<DebugInfoDialect>();
    {
        let mut builder = test.function_builder();
        let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
        builder.ret([value], SourceSpan::UNKNOWN).unwrap();
    }
    let error = test
        .apply_boxed_pass(Box::new(SilentDebugDrop { insert_first: true }), false)
        .unwrap_err();
    assert!(error.to_string().contains("unhandled debug-info mutation"), "{error}");
}

#[test]
fn an_outer_rewrite_does_not_authorize_unchecked_edits_in_a_nested_pipeline() {
    let mut test = Test::new("nested_debug_audit", &[Type::U32, Type::U32], &[Type::U32]);
    test.context().get_or_register_dialect::<DebugInfoDialect>();
    let (record, replacement) = {
        let mut builder = test.function_builder();
        let arguments = builder.entry_block().borrow().arguments().to_vec();
        let value = arguments[0] as ValueRef;
        let record = BuiltinOpBuilder::builder_mut(&mut builder)
            .debug_value(
                value,
                Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None),
                SourceSpan::UNKNOWN,
            )
            .unwrap();
        builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        (record.as_operation_ref(), arguments[1] as ValueRef)
    };
    let root = test.function().as_operation_ref();
    let outer_audit = DebugInfoAudit::begin(root);
    {
        let _outer_update = DebugInfoMutation::new(root, DebugInfoUpdate::Salvage);
        let inner_audit = DebugInfoAudit::begin(root);
        {
            let mut operand = record.borrow().operands()[0];
            operand.borrow_mut().set(replacement);
            let _inner_update = DebugInfoMutation::new(record, DebugInfoUpdate::Equivalent);
        }
        let error = inner_audit.finish().unwrap_err();
        assert!(error.to_string().contains("before its DebugInfoUpdate was declared"), "{error}");
    }
    outer_audit.finish().unwrap();
    let context = test.context_rc();
    let tracking = context.debug_info_mutations.borrow();
    assert!(tracking.audits.is_empty());
    assert!(tracking.permissions.is_empty());
}

fn fixture() -> (Test, OperationRef, ValueRef) {
    let mut test = Test::new("audit_contract", &[Type::U32], &[Type::U32]);
    test.context().get_or_register_dialect::<DebugInfoDialect>();
    let (record, value) = {
        let mut builder = test.function_builder();
        let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let record = BuiltinOpBuilder::builder_mut(&mut builder)
            .debug_value(value, variable(), SourceSpan::UNKNOWN)
            .unwrap()
            .as_operation_ref();
        builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        (record, value)
    };
    (test, record, value)
}

fn variable() -> Variable {
    Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None)
}

fn raw_capture(test: &Test, before: OperationRef, value: ValueRef) -> OperationRef {
    use crate::BuilderExt;
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(before);
    builder.create::<DebugValue, (_, _, _)>(SourceSpan::UNKNOWN)(
        value,
        variable(),
        super::Expression::default(),
    )
    .unwrap()
    .as_operation_ref()
}

#[test]
fn raw_capture_insertion_requires_provenance_even_inside_a_guard() {
    for update in [
        DebugInfoUpdate::Preserve,
        DebugInfoUpdate::Equivalent,
        DebugInfoUpdate::DiscardUnreachable,
        DebugInfoUpdate::Salvage,
    ] {
        for erase in [false, true] {
            let (test, record, value) = fixture();
            let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
            {
                let _guard = DebugInfoMutation::new(record, update);
                let mut inserted = raw_capture(&test, record, value);
                if erase {
                    inserted.borrow_mut().erase();
                }
            }
            assert!(audit.finish().is_err(), "raw insertion accepted: {update:?}, erase={erase}");
        }
    }
}

#[test]
fn raw_insert_then_erase_in_an_initially_empty_audit_is_rejected() {
    let (test, mut record, value) = fixture();
    let next = record.next().unwrap();
    record.borrow_mut().erase();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    raw_capture(&test, next, value).borrow_mut().erase();
    assert!(audit.finish().is_err());
}

#[test]
fn discard_requires_the_missing_records_own_container_to_disappear() {
    let (test, mut record, value) = fixture();
    let live = record.parent().unwrap();
    let region = live.parent().unwrap();
    let mut builder = test.context_rc().builder();
    let mut dead = builder.create_block(region, None, &[]);
    builder.debug_value(value, variable(), SourceSpan::UNKNOWN).unwrap();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    {
        let _guard =
            DebugInfoMutation::for_blocks(&[dead, live], DebugInfoUpdate::DiscardUnreachable);
        dead.borrow_mut().erase();
        record.borrow_mut().erase();
    }
    assert!(audit.finish().is_err());
}

#[test]
fn out_of_order_guard_drop_removes_only_its_own_permission() {
    let (test, mut record, value) = fixture();
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(record);
    let other = builder
        .debug_value(value, variable(), SourceSpan::UNKNOWN)
        .unwrap()
        .as_operation_ref();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    let outer = DebugInfoMutation::new(record, DebugInfoUpdate::Relocate);
    let inner = DebugInfoMutation::new(other, DebugInfoUpdate::Relocate);
    drop(outer);
    let point = crate::ProgramPoint::after(other);
    record.borrow_mut().move_to(point);
    drop(inner);
    assert!(audit.finish().is_err(), "dropped guard retained authority");
}

#[test]
fn a_guard_cannot_outlive_its_audit() {
    let (test, record, _) = fixture();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    let guard = DebugInfoMutation::new(record, DebugInfoUpdate::Preserve);
    let result = audit.finish();
    // Releasing an expired guard must also be safe.
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)));
    assert!(result.is_err(), "audit accepted an unfinished rewrite");
    assert!(released.is_ok(), "expired guard panicked");
}

#[test]
fn producer_location_changes_are_not_semantic_dependency_mutations() {
    use crate::dialects::test::{TestDialect, TestOpBuilder};
    let (test, record, _) = fixture();
    test.context().get_or_register_dialect::<TestDialect>();
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(record);
    let value = builder.u32(42, SourceSpan::UNKNOWN).unwrap();
    let mut operand = record.borrow().operands()[0];
    operand.borrow_mut().set(value);
    let mut producer = value.borrow().get_defining_op().unwrap();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    producer
        .borrow_mut()
        .set_span(SourceSpan::new(SourceSpan::UNKNOWN.source_id(), 1..2));
    audit.finish().unwrap();
}

#[test]
fn prebuilt_raw_capture_attachment_cannot_escape_an_empty_audit() {
    let (test, mut record, value) = fixture();
    let live = record.parent().unwrap();
    let mut region = live.parent().unwrap();
    record.borrow_mut().erase();
    let mut builder = test.context_rc().builder();
    let mut detached = builder.create_block(region, None, &[]);
    // This was built before the audit and then attached and removed while the region is borrowed.
    let terminator = live.borrow().body().back().as_pointer().unwrap();
    let mut raw = raw_capture(&test, terminator, value);
    raw.borrow_mut().move_to(crate::ProgramPoint::at_end_of(detached));
    detached.borrow_mut().erase();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    region.borrow_mut().body_mut().push_back(detached);
    detached.borrow_mut().erase();
    assert!(audit.finish().is_err());
}

#[test]
fn raw_replacement_requires_an_explicit_capture_mapping() {
    let (test, mut original, value) = fixture();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    {
        let _guard = DebugInfoMutation::new(original, DebugInfoUpdate::Salvage);
        raw_capture(&test, original, value);
        original.borrow_mut().erase();
    }
    assert!(audit.finish().is_err());
}

#[test]
fn mapped_raw_replacement_is_accepted_and_later_motion_is_rejected() {
    for move_after_mapping in [false, true] {
        let (test, mut original, value) = fixture();
        let terminator = original.next().unwrap();
        let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
        {
            let mut guard = DebugInfoMutation::new(original, DebugInfoUpdate::Salvage);
            let mut replacement = raw_capture(&test, original, value);
            guard.replace_capture(original, replacement, original).unwrap();
            original.borrow_mut().erase();
            if move_after_mapping {
                replacement.borrow_mut().move_to(crate::ProgramPoint::after(terminator));
            }
        }
        assert_eq!(audit.finish().is_ok(), !move_after_mapping);
    }
}

#[test]
fn coalescence_requires_named_original_and_surviving_capture_with_value_mapping() {
    let (test, mut original, value) = fixture();
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(original);
    let retained = builder
        .debug_value(value, variable(), SourceSpan::UNKNOWN)
        .unwrap()
        .as_operation_ref();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    {
        let mut guard =
            DebugInfoMutation::for_operations(&[original, retained], DebugInfoUpdate::Equivalent);
        guard.coalesce_capture(original, retained, &[]).unwrap();
        original.borrow_mut().erase();
    }
    audit.finish().unwrap();
}

#[test]
fn guard_validates_each_owning_audit_in_its_own_root() {
    let (test, original, value) = fixture();
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(original);
    let other = builder
        .debug_value(value, variable(), SourceSpan::UNKNOWN)
        .unwrap()
        .as_operation_ref();
    let outer = DebugInfoAudit::begin(test.function().as_operation_ref());
    let inner = DebugInfoAudit::begin(other);
    drop(DebugInfoMutation::new(original, DebugInfoUpdate::Preserve));
    inner.finish().unwrap();
    outer.finish().unwrap();
}

#[test]
fn authored_capture_remains_audited_after_creation_inside_a_guard() {
    let (test, original, value) = fixture();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    {
        let _guard = DebugInfoMutation::new(original, DebugInfoUpdate::Salvage);
        let mut builder = test.context_rc().builder();
        builder.set_insertion_point_before(original);
        let mut authored = builder
            .debug_value(value, variable(), SourceSpan::UNKNOWN)
            .unwrap()
            .as_operation_ref();
        authored
            .borrow_mut()
            .set_span(SourceSpan::new(SourceSpan::UNKNOWN.source_id(), 2..3));
    }
    assert!(audit.finish().is_err());
}

#[test]
fn detached_authoring_is_registered_on_attachment_and_subsequent_edits_are_audited() {
    for edit in [false, true] {
        let (test, _, value) = fixture();
        let mut builder = test.context_rc().builder();
        let mut region = test.function().borrow().body().as_region_ref();
        let mut block = builder.create_block(region, None, &[]);
        block.borrow_mut().erase();
        let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
        builder.set_insertion_point_to_end(block);
        let mut record = builder
            .debug_value(value, variable(), SourceSpan::UNKNOWN)
            .unwrap()
            .as_operation_ref();
        region.borrow_mut().body_mut().push_back(block);
        if edit {
            record
                .borrow_mut()
                .set_span(SourceSpan::new(SourceSpan::UNKNOWN.source_id(), 3..4));
        }
        assert_eq!(audit.finish().is_ok(), !edit);
    }
}

#[test]
fn pre_audit_authoring_does_not_authorize_new_attachments() {
    let (test, _, value) = fixture();
    let mut builder = test.context_rc().builder();
    let mut region = test.function().borrow().body().as_region_ref();
    let mut block = builder.create_block(region, None, &[]);
    builder.debug_value(value, variable(), SourceSpan::UNKNOWN).unwrap();
    block.borrow_mut().erase();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    region.borrow_mut().body_mut().push_back(block);
    assert!(audit.finish().is_err());
}

#[test]
fn expired_permission_cannot_authorize_a_later_audit_at_the_same_depth() {
    let (test, mut record, _) = fixture();
    let first = DebugInfoAudit::begin(test.function().as_operation_ref());
    let guard = DebugInfoMutation::new(record, DebugInfoUpdate::Salvage);
    assert!(first.finish().is_err());
    let second = DebugInfoAudit::begin(test.function().as_operation_ref());
    record.borrow_mut().erase();
    drop(guard);
    assert!(second.finish().is_err());
}

#[test]
fn coalescence_checks_ssa_mapping_and_binding_identity() {
    use crate::dialects::test::{TestDialect, TestOpBuilder};
    for map_value in [false, true] {
        let (test, mut original, original_value) = fixture();
        test.context().get_or_register_dialect::<TestDialect>();
        let mut builder = test.context_rc().builder();
        builder.set_insertion_point_before(original);
        let new_value = builder.u32(42, SourceSpan::UNKNOWN).unwrap();
        let retained = builder
            .debug_value(new_value, variable(), SourceSpan::UNKNOWN)
            .unwrap()
            .as_operation_ref();
        let other_binding = builder
            .debug_value(
                new_value,
                Variable::new(Symbol::intern("y"), Symbol::intern("test.rs"), 2, None),
                SourceSpan::UNKNOWN,
            )
            .unwrap()
            .as_operation_ref();
        let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
        {
            let mut guard = DebugInfoMutation::for_operations(
                &[original, retained, other_binding],
                DebugInfoUpdate::Equivalent,
            );
            let values = [(original_value, new_value)];
            assert!(guard.coalesce_capture(original, other_binding, &values).is_err());
            let result =
                guard.coalesce_capture(original, retained, if map_value { &values } else { &[] });
            assert_eq!(result.is_ok(), map_value);
            if map_value {
                original.borrow_mut().erase();
            }
        }
        audit.finish().unwrap();
    }
}

#[test]
fn same_point_contracts_reject_owner_motion_even_after_a_roundtrip() {
    for update in [DebugInfoUpdate::Preserve, DebugInfoUpdate::Equivalent] {
        for roundtrip in [false, true] {
            let (test, record, _) = fixture();
            let mut block = record.parent().unwrap();
            let mut region = block.parent().unwrap();
            test.context_rc().builder().create_block(region, None, &[]);
            let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
            {
                let _guard = DebugInfoMutation::for_blocks(&[block], update);
                block.borrow_mut().erase();
                region.borrow_mut().body_mut().push_back(block);
                if roundtrip {
                    block.borrow_mut().erase();
                    region.borrow_mut().body_mut().push_front(block);
                }
            }
            assert!(audit.finish().is_err(), "{update:?}, roundtrip={roundtrip}");
        }
    }
}

#[test]
fn unchecked_container_roundtrip_does_not_hide_capture_motion() {
    let (test, record, _) = fixture();
    let mut block = record.parent().unwrap();
    let mut region = block.parent().unwrap();
    test.context_rc().builder().create_block(region, None, &[]);
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    block.borrow_mut().erase();
    region.borrow_mut().body_mut().push_back(block);
    block.borrow_mut().erase();
    region.borrow_mut().body_mut().push_front(block);
    assert!(audit.finish().is_err());
}

#[test]
fn record_free_audits_and_guards_do_not_walk_borrowed_descendants() {
    let test = Test::new("no_records", &[], &[]);
    let root = test.function().as_operation_ref();
    let mut block = test.function().borrow().entry_block();
    let _borrowed_descendant = block.borrow_mut();
    let audit = DebugInfoAudit::begin(root);
    drop(DebugInfoMutation::new(root, DebugInfoUpdate::Preserve));
    audit.finish().unwrap();
}

#[test]
fn replacement_mapping_requires_the_original_insertion_anchor() {
    let (test, original, value) = fixture();
    let audit = DebugInfoAudit::begin(test.function().as_operation_ref());
    {
        let mut guard = DebugInfoMutation::new(original, DebugInfoUpdate::Salvage);
        let mut replacement = raw_capture(&test, original.next().unwrap(), value);
        replacement.borrow_mut().move_to(crate::ProgramPoint::before(original));
        assert!(guard.replace_capture(original, replacement, original).is_err());
    }
    assert!(audit.finish().is_err());
}
