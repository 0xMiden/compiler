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
