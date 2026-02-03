use midenc_hir::{
    DIExpressionAttr, DILocalVariableAttr, UnsafeIntrusiveEntityRef, derive::operation,
    traits::AnyType,
};

use crate::DebugInfoDialect;

pub type DebugValueRef = UnsafeIntrusiveEntityRef<DebugValue>;
pub type DebugDeclareRef = UnsafeIntrusiveEntityRef<DebugDeclare>;
pub type DebugKillRef = UnsafeIntrusiveEntityRef<DebugKill>;

/// Records the current value of a source-level variable.
///
/// This is the core operation of the debuginfo dialect. It creates a first-class
/// SSA use of the value, which means:
///
/// - If a transform deletes the value without updating its debug uses, that's a
///   hard error (not a silent drop like with metadata-based approaches).
/// - Standard MLIR-style use-def tracking automatically enforces this — transforms
///   must call `replace_all_uses_with` or explicitly handle debug uses.
///
/// The `variable` attribute identifies the source variable, and the `expression`
/// attribute describes how to recover the source-level value from the IR value
/// (e.g., "dereference this pointer" if the value was promoted to an alloca).
///
/// # Example
///
/// ```text
/// debuginfo.value %0 #[variable = di.local_variable(name = x, ...)]
///                     #[expression = di.expression(DW_OP_WASM_local 0)]
/// ```
#[operation(dialect = DebugInfoDialect)]
pub struct DebugValue {
    #[operand]
    value: AnyType,
    #[attr]
    variable: DILocalVariableAttr,
    #[attr]
    expression: DIExpressionAttr,
}

/// Records the storage location (address) of a source-level variable.
///
/// Unlike `DebugValue` which tracks values, `DebugDeclare` tracks the address
/// where a variable is stored. This is useful for variables that live in memory
/// (e.g., stack allocations) where the address itself doesn't change, but the
/// value at that address may be updated through stores.
///
/// Like `DebugValue`, this creates a real SSA use of the address value,
/// preventing silent drops during transforms.
#[operation(dialect = DebugInfoDialect)]
pub struct DebugDeclare {
    #[operand]
    address: AnyType,
    #[attr]
    variable: DILocalVariableAttr,
}

/// Marks a source-level variable as dead at this program point.
///
/// This provides explicit lifetime boundaries for variables, giving the debugger
/// precise information about when a variable is no longer valid. Without this,
/// debuggers must rely on scope-based heuristics which can be inaccurate after
/// optimizations.
///
/// After a `debuginfo.kill`, the debugger should report the variable as
/// "optimized out" or "not available" until the next `debuginfo.value` or
/// `debuginfo.declare` for the same variable.
///
/// # Example
///
/// ```text
/// debuginfo.kill #[variable = di.local_variable(name = x, ...)]
/// ```
#[operation(dialect = DebugInfoDialect)]
pub struct DebugKill {
    #[attr]
    variable: DILocalVariableAttr,
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, string::ToString};

    use midenc_hir::{
        Builder, Context, DILocalVariableAttr, OpPrinter, OpPrintingFlags, SourceSpan, Type,
        interner::Symbol,
    };

    use crate::{DebugInfoDialect, DebugInfoOpBuilder};

    fn make_variable() -> DILocalVariableAttr {
        let mut variable =
            DILocalVariableAttr::new(Symbol::intern("x"), Symbol::intern("main.rs"), 12, Some(7));
        variable.arg_index = Some(0);
        variable.ty = Some(Type::I32);
        variable
    }

    #[test]
    fn debug_value_carries_metadata() {
        let context = Rc::new(Context::default());
        context.get_or_register_dialect::<DebugInfoDialect>();

        let block = context.create_block_with_params([Type::I32]);
        let arg = block.borrow().arguments()[0];
        let value = arg.borrow().as_value_ref();

        let mut builder = context.clone().builder();
        builder.set_insertion_point_to_end(block);

        let variable = make_variable();
        let debug_value = builder
            .debug_value(value, variable.clone(), SourceSpan::UNKNOWN)
            .expect("failed to create debuginfo.value op");

        assert_eq!(debug_value.borrow().variable(), &variable);
        assert_eq!(block.borrow().back(), Some(debug_value.as_operation_ref()));

        let op = debug_value.as_operation_ref();
        let printed = op.borrow().print(&OpPrintingFlags::default(), context.as_ref()).to_string();
        assert!(printed.contains("di.local_variable"));
    }

    #[test]
    fn debug_kill_carries_variable() {
        let context = Rc::new(Context::default());
        context.get_or_register_dialect::<DebugInfoDialect>();

        let block = context.create_block_with_params([Type::I32]);

        let mut builder = context.clone().builder();
        builder.set_insertion_point_to_end(block);

        let variable = make_variable();
        let debug_kill = builder
            .debug_kill(variable.clone(), SourceSpan::UNKNOWN)
            .expect("failed to create debuginfo.kill op");

        assert_eq!(debug_kill.borrow().variable(), &variable);
    }
}
