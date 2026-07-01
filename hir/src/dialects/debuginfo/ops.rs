use midenc_hir::{
    Context, OpPrinter, Report, UnsafeIntrusiveEntityRef, Verify,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::debuginfo::attributes::{ExpressionAttr, VariableAttr},
    effects::{
        DebugEffect, DebugEffectOpInterface, EffectOpInterface, MemoryEffect,
        MemoryEffectOpInterface,
    },
    smallvec,
    traits::{AnyType, Transparent},
};

use super::DebugInfoDialect;

pub type DebugValueRef = UnsafeIntrusiveEntityRef<DebugValue>;
pub type DebugDeclareRef = UnsafeIntrusiveEntityRef<DebugDeclare>;
pub type DebugKillRef = UnsafeIntrusiveEntityRef<DebugKill>;

/// Records the current value of a source-level variable.
///
/// This is the core operation of the debuginfo dialect. It records a transparent SSA use of the
/// value, which means:
///
/// - If a transform deletes the value without updating its debug uses, that's a hard error (not a
///   silent drop like with metadata-based approaches).
/// - Standard MLIR-style use-def tracking automatically enforces this — transforms must call
///   `replace_all_uses_with`, explicitly handle debug uses, or drop the debug op when its referent
///   is dead.
///
/// The `variable` attribute identifies the source variable, and the `expression` attribute
/// describes how to recover the source-level value from the IR value (e.g., "dereference this
/// pointer" if the value was promoted to an alloca).
///
/// # Example
///
/// ```text
/// di.debug_value %0 #[variable = di.local_variable(name = x, ...)]
///             #[expression = di.expression(DW_OP_WASM_local 0)]
/// ```
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugValue {
    #[operand]
    #[effects(DebugEffect(DebugEffect::Read, DebugEffect::Write))]
    value: AnyType,
    #[attr]
    #[effects(DebugEffect(DebugEffect::Write))]
    variable: VariableAttr,
    #[attr]
    expression: ExpressionAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugValue {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}

impl Verify<dyn DebugEffectOpInterface> for DebugValue {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        let value = self.value().as_value_ref();
        let value = value.borrow();
        let is_orphaned = if let Some(defining_op) = value.get_defining_op() {
            defining_op.borrow().parent().is_none()
        } else {
            value.parent_block().is_none()
        };

        if is_orphaned {
            return Err(Report::msg(
                "di.debug_value operand refers to an erased SSA value; salvage the debug value or \
                 erase the debug op before removing its producer",
            ));
        }

        Ok(())
    }
}

/// Records the storage location (address) of a source-level variable.
///
/// Unlike [DebugValue] which tracks values, [DebugDeclare] tracks the location where a variable is
/// stored. This is useful for variables that live in memory (e.g., stack slots) where the address is
/// described by a debug expression such as `DW_OP_fbreg`.
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugDeclare {
    #[attr]
    #[effects(DebugEffect(DebugEffect::Allocate))]
    variable: VariableAttr,
    #[attr]
    #[effects(DebugEffect(DebugEffect::Write))]
    expression: ExpressionAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugDeclare {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}

/// Marks a source-level variable as dead at this program point.
///
/// This provides explicit lifetime boundaries for variables, giving the debugger precise
/// information about when a variable is no longer valid. Without this, debuggers must rely on
/// scope-based heuristics which can be inaccurate after optimizations.
///
/// After a `di.debug_kill`, the debugger should report the variable as "optimized out" or "not
/// available" until the next `di.debug_value` or `di.debug_declare` for the same variable.
///
/// # Example
///
/// ```text
/// di.debug_kill #[variable = di.local_variable(name = x, ...)]
/// ```
#[derive(EffectOpInterface, OpParser, OpPrinter)]
#[operation(
    dialect = DebugInfoDialect,
    traits(Transparent),
    implements(DebugEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct DebugKill {
    #[attr]
    #[effects(DebugEffect(DebugEffect::Free))]
    variable: VariableAttr,
}

impl EffectOpInterface<MemoryEffect> for DebugKill {
    fn effects(&self) -> midenc_hir::effects::EffectIterator<MemoryEffect> {
        midenc_hir::effects::EffectIterator::from_smallvec(smallvec![])
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use midenc_hir::{
        Builder, RawWalk, Report, SourceSpan, Type, ValueRef,
        dialects::{
            builtin::BuiltinOpBuilder,
            debuginfo::{
                DIBuilder, DebugInfoDialect,
                attributes::Variable,
                transform::{SalvageAction, erase_debug_info, salvage_debug_info},
            },
            test::TestOpBuilder,
        },
        interner::Symbol,
        testing::Test,
    };

    #[test]
    fn debug_value_verifier_rejects_erased_producer() -> Result<(), Report> {
        let mut test =
            Test::new("debug_value_verifier_rejects_erased_producer", &[Type::U32], &[Type::U32]);
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        let (mut producer_op, debug_op) = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();

            let builder = builder.builder_mut();
            builder.set_insertion_point_to_end(entry);

            let input = entry.borrow().arguments()[0] as ValueRef;
            let value = builder.add(input, input, SourceSpan::UNKNOWN)?;
            let producer_op = value.borrow().get_defining_op().unwrap();

            let variable =
                Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, Some(1));
            let debug_op =
                builder.debug_value(value, variable, SourceSpan::UNKNOWN)?.as_operation_ref();

            builder.ret([input], SourceSpan::UNKNOWN)?;

            (producer_op, debug_op)
        };

        producer_op.borrow_mut().erase();

        let err = debug_op.borrow().verify().expect_err("expected dangling debug value to fail");
        assert!(err.to_string().contains("di.debug_value operand refers to an erased SSA value"));

        Ok(())
    }

    #[test]
    fn erase_debug_info_removes_debug_value_before_producer_erasure() -> Result<(), Report> {
        let mut test = Test::new(
            "erase_debug_info_removes_debug_value_before_producer_erasure",
            &[Type::U32],
            &[Type::U32],
        );
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        let (value, mut producer_op, debug_op) = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();

            let builder = builder.builder_mut();
            builder.set_insertion_point_to_end(entry);

            let input = entry.borrow().arguments()[0] as ValueRef;
            let value = builder.add(input, input, SourceSpan::UNKNOWN)?;
            let producer_op = value.borrow().get_defining_op().unwrap();

            let variable =
                Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, Some(1));
            let debug_op =
                builder.debug_value(value, variable, SourceSpan::UNKNOWN)?.as_operation_ref();

            builder.ret([input], SourceSpan::UNKNOWN)?;

            (value, producer_op, debug_op)
        };

        erase_debug_info(&value);
        producer_op.borrow_mut().erase();

        assert!(debug_op.borrow().parent().is_none());

        // Erasing a debug value must leave an explicit end-of-lifetime marker behind
        let mut found_kill = false;
        test.function().as_operation_ref().raw_prewalk_all::<midenc_hir::Forward, _>(
            |op: midenc_hir::OperationRef| {
                if op.borrow().is::<super::DebugKill>() {
                    found_kill = true;
                }
            },
        );
        assert!(found_kill, "expected a di.debug_kill to be emitted for the erased debug value");

        test.function().as_operation_ref().borrow().recursively_verify()?;

        Ok(())
    }

    #[test]
    fn salvage_debug_info_rehomes_debug_value_before_producer_erasure() -> Result<(), Report> {
        let mut test = Test::new(
            "salvage_debug_info_rehomes_debug_value_before_producer_erasure",
            &[Type::U32],
            &[Type::U32],
        );
        test.context().get_or_register_dialect::<DebugInfoDialect>();

        let (old_value, replacement, mut producer_op) = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();

            let builder = builder.builder_mut();
            builder.set_insertion_point_to_end(entry);

            let input = entry.borrow().arguments()[0] as ValueRef;
            let old_value = builder.add(input, input, SourceSpan::UNKNOWN)?;
            let producer_op = old_value.borrow().get_defining_op().unwrap();
            let replacement = builder.mul(input, input, SourceSpan::UNKNOWN)?;

            let variable =
                Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, Some(1));
            builder.debug_value(old_value, variable, SourceSpan::UNKNOWN)?;

            builder.ret([replacement], SourceSpan::UNKNOWN)?;

            (old_value, replacement, producer_op)
        };

        let mut builder = test.function_builder();
        builder.builder_mut().set_insertion_point_after(producer_op);
        salvage_debug_info(
            &old_value,
            &SalvageAction::WithExpression {
                new_value: replacement,
                ops: Default::default(),
            },
            builder.builder_mut(),
        );
        producer_op.borrow_mut().erase();

        test.function().as_operation_ref().borrow().recursively_verify()?;

        Ok(())
    }
}
