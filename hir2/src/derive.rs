pub use midenc_hir_macros::operation;

/// This macro is used to generate the boilerplate for operation trait implementations.
#[macro_export]
macro_rules! derive {
    (
        $(#[$outer:meta])*
        $vis:vis trait $OpTrait:ident {
            $(
                $OpTraitItem:item
            )*
        }

        verify {
            $(
                fn $verify_fn:ident($op:ident: &$OperationPath:path, $ctx:ident: &$ContextPath:path) -> $VerifyResult:ty $verify:block
            )+
        }

        $($t:tt)*
    ) => {
        $crate::__derive_op_trait! {
            $(#[$outer])*
            $vis trait $OpTrait {
                $(
                    $OpTraitItem:item
                )*
            }

            verify {
                $(
                    fn $verify_fn($op: &$OperationPath, $ctx: &$ContextPath) -> $VerifyResult $verify
                )*
            }
        }

        $($t)*
    };

    (
        $(#[$outer:meta])*
        $vis:vis trait $OpTrait:ident {
            $(
                $OpTraitItem:item
            )*
        }

        $($t:tt)*
    ) => {
        $crate::__derive_op_trait! {
            $(#[$outer])*
            $vis trait $OpTrait {
                $(
                    $OpTraitItem:item
                )*
            }
        }

        $($t)*
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_trait {
    (
        $(#[$outer:meta])*
        $vis:vis trait $OpTrait:ident {
            $(
                $OpTraitItem:item
            )*
        }

        verify {
            $(
                fn $verify_fn:ident($op:ident: &$OperationPath:path, $ctx:ident: &$ContextPath:path) -> $VerifyResult:ty $verify:block
            )+
        }
    ) => {
        $(#[$outer])*
        $vis trait $OpTrait {
            $(
                $OpTraitItem
            )*
        }

        impl<T: $crate::Op + $OpTrait> $crate::Verify<dyn $OpTrait> for T {
            #[inline]
            fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                <$crate::Operation as $crate::Verify<dyn $OpTrait>>::verify(self.as_operation(), context)
            }
        }

        impl $crate::Verify<dyn $OpTrait> for $crate::Operation {
            fn should_verify(&self, _context: &$crate::Context) -> bool {
                self.implements::<dyn $OpTrait>()
            }

            fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                $(
                    #[inline]
                    fn $verify_fn($op: &$OperationPath, $ctx: &$ContextPath) -> $VerifyResult $verify
                )*

                $(
                    $verify_fn(self, context)?;
                )*

                Ok(())
            }
        }
    };

    (
        $(#[$outer:meta])*
        $vis:vis trait $OpTrait:ident {
            $(
                $OpTraitItem:item
            )*
        }
    ) => {
        $(#[$outer])*
        $vis trait $OpTrait {
            $(
                $OpTraitItem
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_session::diagnostics::Severity;

    use crate::{
        attributes::Overflow, dialects::test::Add, Builder, BuilderExt, Context, Op, Operation,
        Report, Spanned,
    };

    derive! {
        /// A marker trait for arithmetic ops
        trait ArithmeticOp {}

        verify {
            fn is_binary_op(op: &Operation, ctx: &Context) -> Result<(), Report> {
                if op.num_operands() == 2 {
                    Ok(())
                } else {
                    Err(
                        ctx.diagnostics()
                            .diagnostic(Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(op.span(), format!("incorrect number of operands, expected 2, got {}", op.num_operands()))
                            .with_help("this operator implements 'ArithmeticOp' which requires ops to be binary")
                            .into_report()
                    )
                }
            }
        }
    }

    impl ArithmeticOp for Add {}

    #[test]
    fn derived_op_builder_test() {
        use crate::{SourceSpan, Type};

        let context = Rc::new(Context::default());
        context.register_dialect_hook("test", |info, _ctx| {
            info.register_operation_trait::<Add, dyn ArithmeticOp>();
        });
        let block = context.create_block_with_params([Type::U32, Type::U32]);
        let (lhs, rhs) = {
            let block = block.borrow();
            let lhs = block.get_argument(0).upcast::<dyn crate::Value>();
            let rhs = block.get_argument(1).upcast::<dyn crate::Value>();
            (lhs, rhs)
        };
        let mut builder = context.builder();
        builder.set_insertion_point_to_end(block);
        let op_builder = builder.create::<Add, _>(SourceSpan::default());
        let op = op_builder(lhs, rhs, Overflow::Wrapping);
        let op = op.expect("failed to create AddOp");
        let op = op.borrow();
        assert!(op.as_operation().implements::<dyn ArithmeticOp>());
        assert!(core::hint::black_box(
            !<Add as crate::verifier::Verifier<dyn ArithmeticOp>>::VACUOUS
        ));
    }

    #[ignore = "until https://github.com/0xPolygonMiden/compiler/issues/378 is fixed"]
    #[test]
    #[should_panic = "expected 'u32', got 'i64'"]
    fn derived_op_verifier_test() {
        use crate::{SourceSpan, Type};

        let context = Rc::new(Context::default());
        let block = context.create_block_with_params([Type::U32, Type::I64]);
        let (lhs, invalid_rhs) = {
            let block = block.borrow();
            let lhs = block.get_argument(0).upcast::<dyn crate::Value>();
            let rhs = block.get_argument(1).upcast::<dyn crate::Value>();
            (lhs, rhs)
        };
        let mut builder = context.builder();
        builder.set_insertion_point_to_end(block);
        // Try to create instance of AddOp with mismatched operand types
        let op_builder = builder.create::<Add, _>(SourceSpan::default());
        let op = op_builder(lhs, invalid_rhs, Overflow::Wrapping);
        let _op = op.unwrap();
    }
}
