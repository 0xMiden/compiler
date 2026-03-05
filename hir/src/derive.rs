pub use midenc_hir_macros::{
    Dialect, DialectAttribute, DialectRegistration, EffectOpInterface, OpParser, OpPrinter,
    operation, operation_trait,
};

#[cfg(test)]
mod tests {
    use alloc::format;

    use midenc_session::diagnostics::Severity;

    use crate::{
        BuilderExt, Context, Op, Operation, Report, Spanned, Value, ValueRef,
        derive::operation_trait,
        dialects::{
            builtin::attributes::Overflow,
            test::{self, Add},
        },
        pass::{Nesting, PassManager},
        testing::Test,
    };

    /// A marker trait for arithmetic ops
    #[operation_trait]
    trait ArithmeticOp {
        #[verifier]
        fn is_binary_op(op: &Operation, ctx: &Context) -> Result<(), Report> {
            if op.num_operands() == 2 {
                Ok(())
            } else {
                Err(ctx
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message("invalid operation")
                    .with_primary_label(
                        op.span(),
                        format!(
                            "incorrect number of operands, expected 2, got {}",
                            op.num_operands()
                        ),
                    )
                    .with_help(
                        "this operator implements 'ArithmeticOp' which requires ops to be binary",
                    )
                    .into_report())
            }
        }
    }

    impl ArithmeticOp for Add {}

    inventory::submit!(crate::DialectRegistrationHookInfo::new::<test::TestDialect>(
        register_arithmetic_op_trait
    ));

    fn register_arithmetic_op_trait(info: &mut crate::DialectInfo) {
        info.register_operation_trait::<Add, dyn ArithmeticOp>();
    }

    #[test]
    fn derived_op_builder_test() {
        use crate::{SourceSpan, Type};

        let mut test = Test::new("derived_op_builder", &[Type::U32, Type::U32], &[]);

        let entry = test.entry_block();
        let (lhs, rhs) = {
            let block = entry.borrow();
            let lhs = block.get_argument(0) as ValueRef;
            let rhs = block.get_argument(1) as ValueRef;
            (lhs, rhs)
        };
        let builder = test.builder_mut();
        let add_builder = builder.create::<Add, _>(SourceSpan::default());
        let op = add_builder(lhs, rhs, Overflow::Wrapping);
        let op = op.expect("failed to create AddOp");
        let op = op.borrow();
        assert!(op.as_operation().implements::<dyn ArithmeticOp>());
        assert!(core::hint::black_box(
            !<Add as crate::verifier::Verifier<dyn ArithmeticOp>>::VACUOUS
        ));
    }

    #[test]
    #[should_panic = "expected 'u32', got 'i64'"]
    fn derived_op_verifier_test() {
        use crate::{SourceSpan, Type};

        let mut test = Test::new("derived_op_verifier", &[Type::U32, Type::I64], &[]);

        let entry = test.entry_block();
        let (lhs, invalid_rhs) = {
            let block = entry.borrow();
            let lhs = block.get_argument(0) as ValueRef;
            let rhs = block.get_argument(1) as ValueRef;
            (lhs, rhs)
        };

        // Try to create instance of AddOp with mismatched operand types
        let add_builder = test.builder_mut().create::<Add, _>(SourceSpan::default());
        let op = add_builder(lhs, invalid_rhs, Overflow::Wrapping);
        let op = op.unwrap();

        // Construct a pass manager with the default pass pipeline
        let mut pm = PassManager::on::<Add>(test.context_rc(), Nesting::Implicit);
        // Run pass pipeline
        pm.run(op.as_operation_ref()).unwrap();
    }

    /// Fails if [`InvalidOpsWithReturn`] is created successfully. [`InvalidOpsWithReturn`] is a
    /// struct that has differing types in its result and arguments, despite implementing the
    /// [`SameOperandsAndResultType`] trait.
    #[test]
    #[should_panic = "expected 'i32', got 'u64'"]
    fn same_operands_and_result_type_verifier_test() {
        use crate::{SourceSpan, Type};

        let mut test =
            Test::new("same_operands_and_result_type_verifier", &[Type::I32, Type::I32], &[]);
        let block = test.entry_block();
        let (lhs, rhs) = {
            let block = block.borrow();
            let lhs = block.get_argument(0) as ValueRef;
            let rhs = block.get_argument(1) as ValueRef;
            (lhs, rhs)
        };

        let add_builder = test.builder_mut().create::<Add, _>(SourceSpan::default());
        let op = add_builder(lhs, rhs, Overflow::Wrapping);
        let mut op = op.unwrap();

        // NOTE: We override the result's type in order to force the SameOperandsAndResultType
        // verification function to trigger an error
        {
            let mut binding = op.borrow_mut();
            let mut result = binding.result_mut();
            result.set_type(Type::U64);
        }

        // Construct a pass manager with the default pass pipeline
        let mut pm = PassManager::on::<Add>(test.context_rc(), Nesting::Implicit);
        // Run pass pipeline
        pm.run(op.as_operation_ref()).unwrap();
    }
}
