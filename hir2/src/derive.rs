pub use midenc_hir_macros::operation;

use crate::Operation;

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

/// This type represents the concrete set of derived traits for some op `T`, paired with a
/// type-erased [Operation] reference for an instance of that op.
///
/// This is used for two purposes:
///
/// 1. To generate a specialized [OpVerifier] for `T` which contains all of the type and
///    trait-specific validation logic for that `T`.
/// 2. To apply the specialized verifier for `T` using the wrapped [Operation] reference.
#[doc(hidden)]
pub struct DeriveVerifier<'a, T, Derived: ?Sized> {
    op: &'a Operation,
    _t: core::marker::PhantomData<T>,
    _derived: core::marker::PhantomData<Derived>,
}
impl<'a, T, Derived: ?Sized> DeriveVerifier<'a, T, Derived> {
    #[doc(hidden)]
    pub const fn new(op: &'a Operation) -> Self {
        Self {
            op,
            _t: core::marker::PhantomData,
            _derived: core::marker::PhantomData,
        }
    }
}
impl<'a, T, Derived: ?Sized> core::ops::Deref for DeriveVerifier<'a, T, Derived> {
    type Target = Operation;

    fn deref(&self) -> &Self::Target {
        self.op
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;
    use core::fmt;

    use super::operation;
    use crate::{
        define_attr_type, dialects::hir::HirDialect, formatter, traits::*, Builder, Context, Op,
        Operation, Report, Spanned, Value,
    };

    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    enum Overflow {
        #[allow(unused)]
        None,
        Wrapping,
        #[allow(unused)]
        Overflowing,
    }
    impl fmt::Display for Overflow {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            fmt::Debug::fmt(self, f)
        }
    }
    impl formatter::PrettyPrint for Overflow {
        fn render(&self) -> formatter::Document {
            use formatter::*;
            display(self)
        }
    }
    define_attr_type!(Overflow);

    /// An example op implementation to make sure all of the type machinery works
    #[operation(
        dialect = HirDialect,
        traits(ArithmeticOp, BinaryOp, Commutative, SingleBlock, SameTypeOperands),
        implements(InferTypeOpInterface)
    )]
    struct AddOp {
        #[attr]
        overflow: Overflow,
        #[operand]
        #[order(0)]
        lhs: AnyInteger,
        #[operand]
        #[order(1)]
        rhs: AnyInteger,
        #[result]
        result: AnyInteger,
    }

    impl InferTypeOpInterface for AddOp {
        fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
            let lhs = self.lhs().ty();
            {
                let rhs = self.rhs();
                let rhs = rhs.value();
                let rhs_ty = rhs.ty();
                if &lhs != rhs_ty {
                    return Err(Report::msg(format!(
                        "lhs and rhs types do not match: expected '{lhs}', got '{rhs_ty}'"
                    )));
                }
            }
            self.result_mut().set_type(lhs);
            Ok(())
        }
    }

    derive! {
        /// A marker trait for arithmetic ops
        trait ArithmeticOp {}

        verify {
            fn is_binary_op(op: &Operation, ctx: &Context) -> Result<(), Report> {
                if op.num_operands() == 2 {
                    Ok(())
                } else {
                    Err(
                        ctx.session.diagnostics
                            .diagnostic(miden_assembly::diagnostics::Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(op.span(), format!("incorrect number of operands, expected 2, got {}", op.num_operands()))
                            .with_help("this operator implements 'ArithmeticOp' which requires ops to be binary")
                            .into_report()
                    )
                }
            }
        }
    }

    #[test]
    fn derived_op_builder_test() {
        use crate::{SourceSpan, Type};

        let context = Rc::new(Context::default());
        let block = context.create_block_with_params([Type::U32, Type::U32]);
        let (lhs, rhs) = {
            let block = block.borrow();
            let lhs = block.get_argument(0).upcast::<dyn crate::Value>();
            let rhs = block.get_argument(1).upcast::<dyn crate::Value>();
            (lhs, rhs)
        };
        let mut builder = context.builder();
        builder.set_insertion_point_to_end(block);
        let op_builder = builder.create::<AddOp, _>(SourceSpan::default());
        let op = op_builder(lhs, rhs, Overflow::Wrapping);
        let op = op.expect("failed to create AddOp");
        let op = op.borrow();
        assert!(op.as_operation().implements::<dyn ArithmeticOp>());
        assert!(core::hint::black_box(
            !<AddOp as crate::verifier::Verifier<dyn ArithmeticOp>>::VACUOUS
        ));
    }

    #[test]
    #[should_panic = "lhs and rhs types do not match: expected 'u32', got 'i64'"]
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
        let op_builder = builder.create::<AddOp, _>(SourceSpan::default());
        let op = op_builder(lhs, invalid_rhs, Overflow::Wrapping);
        let _op = op.unwrap();
    }
}
