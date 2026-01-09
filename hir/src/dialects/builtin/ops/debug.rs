use crate::{
    UnsafeIntrusiveEntityRef,
    attributes::{DIExpressionAttr, DILocalVariableAttr},
    derive::operation,
    dialects::builtin::BuiltinDialect,
    traits::AnyType,
};

pub type DbgValueRef = UnsafeIntrusiveEntityRef<DbgValue>;
pub type DbgDeclareRef = UnsafeIntrusiveEntityRef<DbgDeclare>;

/// Records the value of an SSA operand for debug information consumers.
#[operation(dialect = BuiltinDialect)]
pub struct DbgValue {
    #[operand]
    value: AnyType,
    #[attr]
    variable: DILocalVariableAttr,
    #[attr]
    expression: DIExpressionAttr,
}

/// Records the storage location of a source-level variable.
#[operation(dialect = BuiltinDialect)]
pub struct DbgDeclare {
    #[operand]
    address: AnyType,
    #[attr]
    variable: DILocalVariableAttr,
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, string::ToString};

    use crate::{
        Builder, Context, OpPrinter, OpPrintingFlags, SourceSpan, Type,
        attributes::DILocalVariableAttr,
        dialects::builtin::{BuiltinDialect, BuiltinOpBuilder},
        interner::Symbol,
    };

    fn make_variable() -> DILocalVariableAttr {
        let mut variable =
            DILocalVariableAttr::new(Symbol::intern("x"), Symbol::intern("main.rs"), 12, Some(7));
        variable.arg_index = Some(0);
        variable.ty = Some(Type::I32);
        variable
    }

    #[test]
    fn dbg_value_carries_metadata() {
        let context = Rc::new(Context::default());
        context.get_or_register_dialect::<BuiltinDialect>();

        let block = context.create_block_with_params([Type::I32]);
        let arg = block.borrow().arguments()[0];
        let value = arg.borrow().as_value_ref();

        let mut builder = context.clone().builder();
        builder.set_insertion_point_to_end(block);

        let variable = make_variable();
        let dbg_value = builder
            .dbg_value(value, variable.clone(), SourceSpan::UNKNOWN)
            .expect("failed to create dbg.value op");

        assert_eq!(dbg_value.borrow().variable(), &variable);
        assert_eq!(block.borrow().back(), Some(dbg_value.as_operation_ref()));

        let op = dbg_value.as_operation_ref();
        let printed = op.borrow().print(&OpPrintingFlags::default(), context.as_ref()).to_string();
        assert!(printed.contains("di.local_variable"));
    }
}
