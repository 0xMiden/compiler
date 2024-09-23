use crate::Operation;

/// This macro is used to generate the boilerplate for [Op] implementations.
///
/// TODO(pauls):
///
/// * Support doc comments
/// * Implement type constraints/inference
/// * Implement `verify` blocks for custom verification rules
/// * FIX: Currently #[operands] simply adds boilerplate for creating an operation with those
///   operands, but it does not create methods to access them, and simply adds them in with the
///   other operands. We should figure out how to store operands in such a way that multiple operand
///   groups can be maintained even when adding/removing operands later.
/// * FIX: Currently #[successors] adds a field to the struct to store whatever custom type is used
///   to represent the successors, but these successors are not reachable from the Operation backing
///   the op, and as a result, any successor operations acting from the Operation and not the Op may
///   cause the two to converge. Like the #[operands] issue above, we need to store the actual
///   successor in the Operation, and provide some way to map between the two, OR change how we
///   represent successors to allow storing arbitrary successor-like types in the Operation
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

        $(derives $DerivedOpTrait:ident $(, $MoreDerivedTraits:ident)*;)*
        $(implements $ImplementedOpTrait:ident $(, $MoreImplementedTraits:ident)*;)*
    ) => {
        $crate::__derive_op!(
            $(#[$outer])*
            $vis struct $Op {
                $(
                    $(#[$inner $($args)*])*
                    $Field: $FieldTy,
                )*
            }

            $(
                derives $DerivedOpTrait
                $(
                    derives $MoreDerivedTraits
                )*
            )*
            $(
                implements $ImplementedOpTrait
                $(
                    implements $MoreImplementedTraits
                )*
            )*
        );
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

        $(derives $DerivedOpTrait:ident)*
        $(implements $ImplementedOpTrait:ident)*
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                $(
                    {
                        unprocessed: [$(#[$inner $($args)*])*],
                        ignore: [],
                        field: $Field,
                        field_type: $FieldTy,
                    }
                )*
            ],
            processed: {
                fields: [],
                dialect: [],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [],
                regions_count: [0usize],
                regions: [],
                successor_groups_count: [0usize],
                successor_groups: [],
                successors_count: [0usize],
                successors: [],
                operand_groups_count: [0usize],
                operand_groups: [],
                operands_count: [0usize],
                operands: [],
                results_count: [0usize],
                results: [],
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_processor {
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
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$(dialect_processed:tt)+],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
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
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [dialect $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [dialect $FieldTy],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `region` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[region $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [region $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [1usize + $regions_count],
                regions: [region $Field at $regions_count $($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `successor` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[successor $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [successor $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [1usize + $succ_count],
                successors: [successor $Field at $succ_count $($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `successors` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[successors $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [successors $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [1usize + $succ_groups_count],
                successor_groups: [successors $Field : $FieldTy $($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
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
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [operand $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [1usize + $operands_count],
                operands: [operand $Field at $operands_count $($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `operands` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[operands $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [operands $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [1usize + $operand_groups_count],
                operand_groups: [operands $Field at $operand_groups_count $($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
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
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [result $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
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
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [attr $($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [attr $Field: $FieldTy $($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle `doc` attr
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [
                    #[doc $($args:tt)*]
                    $($attrs_rest:tt)*
                ],
                ignore: [$($IgnoredReason:tt)*],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                {
                    unprocessed: [
                        $($attrs_rest)*
                    ],
                    ignore: [$($IgnoredReason)*],
                    field: $Field,
                    field_type: $FieldTy,
                }
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle end of unprocessed attributes (ignore=false)
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [],
                ignore: [],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                $($fields_rest)*
            ],
            processed: {
                fields: [field $Field: $FieldTy $($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
                operands_count: [$operands_count],
                operands: [$($operands_processed)*],
                results_count: [$results_count],
                results: [$($results_processed)*],
            }
        }
    };

    // Handle end of unprocessed attributes (ignore=true)
    (
        $(#[$outer:meta])*
        $vis:vis struct $Op:ident;

        unprocessed: [
            {
                unprocessed: [],
                ignore: [$($IgnoredReason:tt)+],
                field: $Field:ident,
                field_type: $FieldTy:ty,
            }
            $($fields_rest:tt)*
        ],
        processed: {
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
            operands_count: [$operands_count:expr],
            operands: [$($operands_processed:tt)*],
            results_count: [$results_count:expr],
            results: [$($results_processed:tt)*],
        }
    ) => {
        $crate::__derive_op_processor! {
            $(#[$outer])*
            $vis struct $Op;

            unprocessed: [
                $($fields_rest)*
            ],
            processed: {
                fields: [$($extra_fields_processed)*],
                dialect: [$($dialect_processed)*],
                traits: [$(derives $DerivedOpTrait),* $(implements $ImplementedOpTrait),*],
                attrs: [$($attrs_processed)*],
                regions_count: [$regions_count],
                regions: [$($regions_processed)*],
                successor_groups_count: [$succ_groups_count],
                successor_groups: [$($succ_groups_processed)*],
                successors_count: [$succ_count],
                successors: [$($succ_processed)*],
                operand_groups_count: [$operand_groups_count],
                operand_groups: [$($operand_groups_processed)*],
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
            fields: [$($extra_fields_processed:tt)*],
            dialect: [$($dialect_processed:tt)*],
            traits: [$(derives $DerivedOpTrait:ident),* $(implements $ImplementedOpTrait:ident),*],
            attrs: [$($attrs_processed:tt)*],
            regions_count: [$regions_count:expr],
            regions: [$($regions_processed:tt)*],
            successor_groups_count: [$succ_groups_count:expr],
            successor_groups: [$($succ_groups_processed:tt)*],
            successors_count: [$succ_count:expr],
            successors: [$($succ_processed:tt)*],
            operand_groups_count: [$operand_groups_count:expr],
            operand_groups: [$($operand_groups_processed:tt)*],
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
            $($extra_fields_processed)*;
            $(derives $DerivedOpTrait)*;
            $(implements $ImplementedOpTrait)*;
            $($attrs_processed)*;
            regions $regions_count;
            $($regions_processed)*;
            $($succ_groups_processed)*;
            $($succ_processed)*;
            $($operand_groups_processed)*;
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
        $(field $Field:ident: $FieldTy:ty)*;
        $(derives $DerivedOpTrait:ident)*;
        $(implements $ImplementedOpTrait:ident)*;
        $(attr $AttrField:ident: $AttrTy:ty)*;
        regions $NumRegions:expr;
        $(region $RegionField:ident at $RegionIdx:expr)*;
        $(successors $SuccGroupField:ident: $SuccGroupTy:ty)*;
        $(successor $SuccField:ident at $SuccIdx:expr)*;
        $(operands $OperandGroupField:ident at $OperandGroupIdx:expr)*;
        $(operand $Operand:ident at $OperandIdx:expr)*;
        $(result $Result:ident at $ResultIdx:expr)*;

    ) => {
        $(#[$outer])*
        $vis struct $Op {
            op: $crate::Operation,
            $(
                $Field: $FieldTy,
            )*
            $(
                $SuccGroupField: $SuccGroupTy,
            )*
        }
        impl ::midenc_session::diagnostics::Spanned for $Op {
            fn span(&self) -> ::midenc_session::diagnostics::SourceSpan {
                self.op.span()
            }
        }

        #[allow(unused)]
        impl $Op {
            /// Get a new, uninitialized instance of this op
            pub fn uninit($($Field: $FieldTy),*) -> Self {
                let mut op = $crate::Operation::uninit::<Self>();
                Self {
                    op,
                    $(
                        $Field,
                    )*
                    $(
                        $SuccGroupField: Default::default(),
                    )*
                }
            }

            pub fn create(
                context: &$crate::Context
                $(
                    , $Operand: $crate::ValueRef
                )*
                $(
                    , $OperandGroupField: impl IntoIterator<Item = $crate::ValueRef>
                )*
                $(
                    , $Field: $FieldTy
                )*
                $(
                    , $AttrField: $AttrTy
                )*
                $(
                    , $SuccGroupField: $SuccGroupTy
                )*
                $(
                    , $SuccField: $crate::OpSuccessor
                )*
            ) -> Result<$crate::UnsafeIntrusiveEntityRef<$Op>, $crate::Report> {
                let mut this = Self::uninit($($Field),*);
                $(
                    this.$SuccGroupField = $SuccGroupField.clone();
                )*

                let mut builder = $crate::OperationBuilder::<Self>::new(context, this);
                $(
                    builder.implement::<dyn $DerivedOpTrait>();
                )*
                $(
                    builder.implement::<dyn $ImplementedOpTrait>();
                )*
                $(
                    builder.with_attr(stringify!($AttrField), $AttrField);
                )*
                builder.with_operands([$($Operand),*]);
                $(
                    builder.with_operands_in_group($OperandGroupIdx, $OperandGroupField);
                )*
                $(
                    #[doc = stringify!($RegionField)]
                    builder.create_region();
                )*
                $(
                    builder.with_successors($SuccGroupField);
                )*
                $(
                    builder.with_successor($SuccField);
                )*
                let num_results = const {
                    let results: &[usize] = &[$($ResultIdx),*];
                    results.len()
                };
                builder.with_results(num_results);
                builder.build()
            }

            $(
                #[inline]
                fn $Field(&self) -> &$FieldTy {
                    &self.$Field
                }

                paste::paste! {
                    #[inline]
                    fn [<$Field _mut>](&mut self) -> &mut $FieldTy {
                        &mut self.$Field
                    }

                    #[doc = concat!("Set the value of ", stringify!($Field))]
                    #[inline]
                    fn [<set_ $Field>](&mut self, $Field: $FieldTy) {
                        self.$Field = $Field;
                    }
                }
            )*

            $(
                fn $AttrField(&self) -> &$AttrTy {
                    let sym = stringify!($AttrField);
                    self.op.get_typed_attribute::<$AttrTy, _>(&::midenc_hir_symbol::Symbol::intern(sym)).unwrap()
                }

                paste::paste! {
                    fn [<$AttrField _mut>](&mut self) -> &mut $AttrTy {
                        let sym = stringify!($AttrField);
                        self.op.get_typed_attribute_mut::<$AttrTy, _>(&::midenc_hir_symbol::Symbol::intern(sym)).unwrap()
                    }

                    fn [<set_ $AttrField>](&mut self, value: $AttrTy) {
                        let sym = stringify!($AttrField);
                        self.op.set_attribute(::midenc_hir_symbol::Symbol::intern(sym), Some(value));
                    }
                }
            )*

            $(
                fn $RegionField(&self) -> $crate::EntityRef<'_, $crate::Region> {
                    self.op.region($RegionIdx)
                }

                paste::paste! {
                    fn [<$RegionField _mut>](&mut self) -> $crate::EntityMut<'_, $crate::Region> {
                        self.op.region_mut($RegionIdx)
                    }
                }
            )*

            $(
                #[inline]
                fn $SuccGroupField(&self) -> &$SuccGroupTy {
                    &self.$SuccGroupField
                }

                paste::paste! {
                    #[inline]
                    fn [<$SuccGroupField _mut>](&mut self) -> &mut $SuccGroupTy {
                        &mut self.$SuccGroupField
                    }
                }
            )*

            $(
                #[inline]
                fn $SuccField(&self) -> &$crate::OpSuccessor {
                    &self.successors()[$SuccIdx]
                }

                paste::paste! {
                    #[inline]
                    fn [<$SuccField _mut>](&mut self) -> &mut $crate::OpSuccessor {
                        &mut self.successors_mut()[$SuccIdx]
                    }
                }
            )*

            $(
                fn $Operand(&self) -> $crate::EntityRef<'_, $crate::OpOperandImpl> {
                    self.op.operands()[$OperandIdx].borrow()
                }

                paste::paste! {
                    fn [<$Operand _mut>](&mut self) -> $crate::EntityMut<'_, $crate::OpOperandImpl> {
                        self.op.operands_mut()[$OperandIdx].borrow_mut()
                    }
                }
            )*

            $(
                fn $OperandGroupField(&self) -> $crate::OpOperandRange<'_> {
                    self.op.operands().group($OperandGroupIdx)
                }

                paste::paste! {
                    fn [<$OperandGroupField _mut>](&mut self) -> $crate::OpOperandRangeMut<'_> {
                        self.op.operands_mut().group_mut($OperandGroupIdx)
                    }
                }
            )*

            $(
                fn $Result(&self) -> $crate::EntityRef<'_, dyn $crate::Value> {
                    self.results()[$ResultIdx].borrow()
                }

                paste::paste! {
                    fn [<$Result _mut>](&mut self) -> $crate::EntityMut<'_, dyn $crate::Value> {
                        self.op.results_mut()[$ResultIdx].borrow_mut()
                    }
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

        $crate::__derive_op_name!($Op, $Dialect);

        impl $crate::Op for $Op {
            fn name(&self) -> $crate::OperationName {
                paste::paste! {
                    *[<__ $Op _NAME>]
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

        $crate::__derive_op_traits!($Op $(, derive $DerivedOpTrait)* $(, implement $ImplementedOpTrait)*);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __derive_op_name {
    ($Op:ident, $Dialect:ty) => {
        paste::paste! {
            #[allow(non_upper_case_globals)]
            static [<__ $Op _NAME>]: ::std::sync::LazyLock<$crate::OperationName> = ::std::sync::LazyLock::new(|| {
                const DIALECT: $Dialect = <$Dialect as $crate::Dialect>::INIT;

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
                let name = ::midenc_hir_symbol::Symbol::intern(buf);
                let dialect = <$Dialect as $crate::Dialect>::name(&DIALECT);
                $crate::OperationName::new(dialect, name)
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

    ($T:ty $(, derive $DeriveTrait:ident)* $(, implement $ImplementTrait:ident)*) => {
        $(
            impl $DeriveTrait for $T {}
        )*

        impl $crate::OpVerifier for $T {
            fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                #[allow(unused_parens)]
                type OpVerifierImpl<'a> = $crate::derive::DeriveVerifier<'a, $T, ($(&'a dyn $DeriveTrait,)* $(&'a dyn $ImplementTrait),*)>;
                #[allow(unused_parens)]
                impl<'a> $crate::OpVerifier for $crate::derive::DeriveVerifier<'a, $T, ($(&'a dyn $DeriveTrait,)* $(&'a dyn $ImplementTrait),*)>
                where
                    $(
                        $T: $crate::verifier::Verifier<dyn $DeriveTrait>,
                    )*
                    $(
                        $T: $crate::verifier::Verifier<dyn $ImplementTrait>,
                    )*
                {
                    fn verify(&self, context: &$crate::Context) -> Result<(), $crate::Report> {
                        let op = self.downcast_ref::<$T>().unwrap();
                        $(
                            if const { !<$T as $crate::verifier::Verifier<dyn $DeriveTrait>>::VACUOUS } {
                                <$T as $crate::verifier::Verifier<dyn $DeriveTrait>>::maybe_verify(op, context)?;
                            }
                        )*
                        $(
                            if const { !<$T as $crate::verifier::Verifier<dyn $ImplementTrait>>::VACUOUS } {
                                <$T as $crate::verifier::Verifier<dyn $ImplementTrait>>::maybe_verify(op, context)?;
                            }
                        )*

                        Ok(())
                    }
                }

                let verifier = OpVerifierImpl::new(&self.op);
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
        Spanned,
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
        struct AddOp : Op {
            #[dialect]
            dialect: HirDialect,
            #[attr]
            overflow: Overflow,
            #[operand]
            lhs: OpOperand,
            #[operand]
            rhs: OpOperand,
        }

        derives SingleBlock, SameTypeOperands;
        implements ArithmeticOp;
    }

    impl ArithmeticOp for AddOp {}

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
