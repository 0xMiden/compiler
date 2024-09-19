use crate::Operation;

/// This macro is used to generate the boilerplate for [Op] implementations.
///
/// TODO(pauls):
///
/// * Implement `#[region]` support
/// * Implement `#[successor]` support
/// * Implement `#[successors]` support for variadic successors
/// * Implement `#[successors(interface)]` to access successors through `SuccessorInterface`
/// * Support doc comments
/// * Implement type constraints/inference
/// * Implement `verify` blocks for custom verification rules
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

    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident : Op {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        $($t:tt)*
    ) => {
        $crate::__derive_op!(
            $(#[$outer])*
            #[derive($crate::Spanned)]
            $vis struct $Op {
                $(
                    $(#[$inner $($args)*])*
                    $Field: $FieldTy
                ),*
            }
        );

        $($t)*
    };

    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident : Op implements $OpTrait:ident {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        $($t:tt)*
    ) => {
        $crate::__derive_op!(
            $(#[$outer])*
            $vis struct $Op {
                $(
                    $(#[$inner $($args)*])*
                    $Field: $FieldTy,
                )*
            }

            implement $OpTrait;
        );

        $($t)*
    };

    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident : Op implements $OpTrait1:ident $(, $OpTraitRest:ident)* {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        $($t:tt)*
    ) => {
        $crate::__derive_op!(
            $(#[$outer])*
            $vis struct $Op {
                $(
                    $(#[$inner $($args)*])*
                    $Field: $FieldTy,
                )*
            }

            implement $OpTrait1
            $(, implement $OpTraitRest)*;
        );

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

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op {
    // Entry
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        $(implement $OpTrait:ident),*;
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                $(
                    {
                        unprocessed: [$(#[$inner $($args)*])*],
                        field: $Field,
                        field_type: $FieldTy,
                    }
                )*
            ],
            processed: {
                dialect: [],
                traits: [$(implement $OpTrait),*],
                attrs: [],
                operands_count: [0usize],
                operands: [],
                results_count: [0usize],
                results: [],
            }
        }
    };

    // Handle duplicate `dialect` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[dialect]
                    $($attrs_rest:tt)*
                ],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [$(dialect_processed:tt)+],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        compile_error!("unexpected duplicate dialect attr: got '{}', but '{}' was previously seen", stringify!($Dialect), stringify!($dialect_processed));
    };

    // Handle `dialect` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[dialect]
                    $($attrs_rest:tt)*
                ],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                dialect: [dialect $FieldTy],
                traits: [$(implement $OpTrait),*],
                attrs: [$($attrs_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `operand` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[operand $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [$($dialect_processed:tt)*],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                dialect: [$($dialect_processed)*],
                traits: [$(implement $OpTrait),*],
                attrs: [$($attrs_processed)*],
                operands_count: [1usize + $operands_count],
                operands: [operand $Field at $operands_count $($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `result` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[result $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [$($dialect_processed:tt)*],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                dialect: [$($dialect_processed)*],
                traits: [$(implement $OpTrait),*],
                attrs: [$($attrs_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [1usize + $results_count],
                results: [result $Field at $results_count $($results_processed)*],
            }
        }
    };

    // Handle `attr` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[attr $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [$($dialect_processed:tt)*],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                dialect: [$($dialect_processed)*],
                traits: [$(implement $OpTrait),*],
                attrs: [attr $Field: $FieldTy $($attrs_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle end of unprocessed attributes
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            dialect: [$($dialect_processed:tt)*],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                $($fields_rest)*
            ],
            processed: {
                dialect: [$($dialect_processed)*],
                traits: [$(implement $OpTrait),*],
                attrs: [$($attrs_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle end of unprocessed fields
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [],
        processed: {
            dialect: [$($dialect_processed:tt)*],
            traits: [$(implement $OpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_impl!(
            $(#[$outer])*
            $vis struct $Op;

            $($dialect_processed)*;
            $(implement $OpTrait),*;
            $($attrs_processed)*;
            $($operands_processed)*;
            $($results_processed)*;
        );
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_impl {
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        dialect $Dialect:ty;
        $(implement $OpTrait:ident),*;
        $(attr $AttrField:ident: $AttrTy:ty)*;
        $(operand $Operand:ident at $OperandIdx:expr)*;
        $(result $Result:ident at $ResultIdx:expr)*;

    ) => {
        $(#[$outer])*
        #[derive(Spanned)]
        $vis struct $Op {
            #[span]
            op: $crate::Operation,
        }

        #[allow(unused)]
        impl $Op {
            /// Get a new, uninitialized instance of this op
            pub fn uninit() -> Self {
                Self {
                    op: $crate::Operation::uninit::<Self>(),
                }
            }

            pub fn create(
                context: &$crate::Context
                $(
                    , $Operand: $crate::ValueRef
                )*
                $(
                    , $AttrField: $AttrTy
                )*
            ) -> Result<$crate::UnsafeIntrusiveEntityRef<$Op>, $crate::Report> {
                let mut builder = $crate::OperationBuilder::<Self>::new(context, Self::uninit());
                $(
                    builder.implement::<dyn $OpTrait>();
                )*
                $(
                    builder.with_attr(stringify!($AttrField), $AttrField);
                )*
                builder.with_operands([$($Operand),*]);
                let num_results = const {
                    let results: &[usize] = &[$($ResultIdx),*];
                    results.len()
                };
                builder.with_results(num_results);
                builder.build()
            }

            $(
                fn $AttrField(&self) -> $AttrTy {
                    let sym = stringify!($AttrField);
                    let value = self.op.get_attribute(&::midenc_hir_symbol::Symbol::intern(sym)).unwrap();
                    value.downcast_ref::<$AttrTy>().unwrap().clone()
                }
            )*

            $(
                fn $Operand(&self) -> $crate::OpOperand {
                    self.operands()[$OperandIdx].clone()
                }
            )*

            $(
                fn $Result(&self) -> $crate::ValueRef {
                    self.results()[$ResultIdx].clone()
                }
            )*
        }

        impl AsRef<$crate::Operation> for $Op {
            #[inline(always)]
            fn as_ref(&self) -> &$crate::Operation {
                &self.op
            }
        }

        impl AsMut<$crate::Operation> for $Op {
            #[inline(always)]
            fn as_mut(&mut self) -> &mut $crate::Operation {
                &mut self.op
            }
        }

        __derive_op_name!($Op);

        impl $crate::Op for $Op {
            fn name(&self) -> $crate::OperationName {
                const DIALECT: $Dialect = <$Dialect as $crate::Dialect>::INIT;
                let dialect = <$Dialect as $crate::Dialect>::name(&DIALECT);
                paste::paste! {
                    $crate::OperationName::new(dialect, *[<__ $Op _NAME>])
                }
            }

            #[inline(always)]
            fn as_operation(&self) -> &$crate::Operation {
                &self.op
            }

            #[inline(always)]
            fn as_operation_mut(&mut self) -> &mut $crate::Operation {
                &mut self.op
            }
        }

        __derive_op_traits!($Op, $($OpTrait),*);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_name {
    ($Op:ident) => {
        paste::paste! {
            #[allow(non_upper_case_globals)]
            static [<__ $Op _NAME>]: ::std::sync::LazyLock<::midenc_hir_symbol::Symbol> = ::std::sync::LazyLock::new(|| {
                // CondBrOp => CondBr => cond_br
                //                Add => add
                let type_name = stringify!($Op);
                let type_name = type_name.strip_suffix("Op").unwrap_or(type_name);
                let mut buf = ::alloc::string::String::with_capacity(type_name.len());
                let mut word_started_at = None;
                for (i, c) in type_name.char_indices() {
                    if c.is_ascii_uppercase() {
                        if word_started_at.is_some() {
                            buf.push('_');
                            buf.push(c.to_ascii_lowercase());
                        } else {
                            word_started_at = Some(i);
                            buf.push(c.to_ascii_lowercase());
                        }
                    } else if word_started_at.is_none() {
                        word_started_at = Some(i);
                        buf.push(c);
                    } else {
                        buf.push(c);
                    }
                }
                ::midenc_hir_symbol::Symbol::intern(buf)
            });
        }
    }
}

/// This macro emits the trait derivations and specialized verifier for a given [Op] impl.
#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_traits {
    ($T:ty) => {
        impl $crate::OpVerifier for $T {
            #[inline(always)]
            fn verify(&self, _context: &$crate::Context) -> Result<(), $crate::Report> {
                Ok(())
            }
        }
    };

    ($T:ty, $($Trait:ident),+) => {
        $(
            impl $Trait for $T {}
        )*

        impl $crate::OpVerifier for $T {
            fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                #[allow(unused_parens)]
                type OpVerifierImpl<'a> = $crate::derive::DeriveVerifier<'a, $T, ($(&'a dyn $Trait),*)>;
                #[allow(unused_parens)]
                impl<'a> $crate::OpVerifier for $crate::derive::DeriveVerifier<'a, $T, ($(&'a dyn $Trait),*)>
                where
                    $(
                        $T: $crate::verifier::Verifier<dyn $Trait>
                    ),*
                {
                    fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                        let op = self.downcast_ref::<$T>().unwrap();
                        $(
                            if const { !<$T as $crate::verifier::Verifier<dyn $Trait>>::VACUOUS } {
                                <$T as $crate::verifier::Verifier<dyn $Trait>>::maybe_verify(op, context)?;
                            }
                        )*

                        Ok(())
                    }
                }

                let op = self.as_operation();
                let verifier = OpVerifierImpl::new(op);
                verifier.verify(context)
            }
        }
    }
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
    use core::fmt;

    use crate::{
        define_attr_type, dialects::hir::HirDialect, traits::*, Context, Op, Operation, Report,
        SourceSpan, Spanned,
    };

    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
    define_attr_type!(Overflow);

    derive! {
        /// An example op implementation to make sure all of the type machinery works
        struct AddOp : Op implements SingleBlock, SameTypeOperands, ArithmeticOp {
            #[dialect]
            dialect: HirDialect,
            #[attr]
            overflow: Overflow,
            #[operand]
            lhs: OpOperand,
            #[operand]
            rhs: OpOperand,
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
    fn test_derived_op() {
        use crate::Type;

        let context = Context::default();
        let block = context.create_block_with_params([Type::U32, Type::I64]);
        let block = block.borrow();
        let lhs = block.get_argument(0);
        let rhs = block.get_argument(1);
        let op = AddOp::create(&context, rhs, lhs, Overflow::Wrapping);
        let op = op.expect("failed to create AddOp");
        let op = op.borrow();
        assert!(op.as_operation().implements::<dyn ArithmeticOp>());
        assert!(core::hint::black_box(
            !<AddOp as crate::verifier::Verifier<dyn ArithmeticOp>>::VACUOUS
        ));
    }
}
