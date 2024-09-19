mod multitrait;

use midenc_session::diagnostics::Severity;

pub(crate) use self::multitrait::MultiTraitVtable;
use crate::{derive, Context, Operation, Report, Spanned};

/// Marker trait for commutative ops, e.g. `X op Y == Y op X`
pub trait Commutative {}

/// Marker trait for constant-like ops
pub trait ConstantLike {}

/// Marker trait for ops with side effects
pub trait HasSideEffects {}

/// Marker trait for ops which read memory
pub trait MemoryRead {}

/// Marker trait for ops which write memory
pub trait MemoryWrite {}

/// Marker trait for return-like ops
pub trait ReturnLike {}

/// Op is a terminator (i.e. it can be used to terminate a block)
pub trait Terminator {}

/// Marker trait for idemptoent ops, i.e. `op op X == op X (unary) / X op X == X (binary)`
pub trait Idempotent {}

/// Marker trait for ops that exhibit the property `op op X == X`
pub trait Involution {}

/// Marker trait for ops which are not permitted to access values defined above them
pub trait IsolatedFromAbove {}

derive! {
    /// Marker trait for unary ops, i.e. those which take a single operand
    pub trait UnaryOp {}

    verify {
        fn is_unary_op(op: &Operation, context: &Context) -> Result<(), Report> {
            if op.num_operands() == 1 {
                Ok(())
            } else {
                Err(
                    context.session.diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid operation")
                        .with_primary_label(op.span(), format!("incorrect number of operands, expected 1, got {}", op.num_operands()))
                        .with_help("this operator implements 'UnaryOp', which requires it to have exactly one operand")
                        .into_report()
                )
            }
        }
    }
}

derive! {
    /// Marker trait for binary ops, i.e. those which take two operands
    pub trait BinaryOp {}

    verify {
        fn is_binary_op(op: &Operation, context: &Context) -> Result<(), Report> {
            if op.num_operands() == 2 {
                Ok(())
            } else {
                Err(
                    context.session.diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid operation")
                        .with_primary_label(op.span(), format!("incorrect number of operands, expected 2, got {}", op.num_operands()))
                        .with_help("this operator implements 'BinaryOp', which requires it to have exactly two operands")
                        .into_report()
                )
            }
        }
    }
}

derive! {
    /// Op expects all operands to be of the same type
    pub trait SameTypeOperands {}

    verify {
        fn operands_are_the_same_type(op: &Operation, context: &Context) -> Result<(), Report> {
            if let Some((first_operand, operands)) = op.operands().split_first() {
                let (expected_ty, set_by) = {
                    let operand = first_operand.borrow();
                    let value = operand.value();
                    (value.ty().clone(), value.span())
                };
                for operand in operands {
                    let operand = operand.borrow();
                    let value = operand.value();
                    let value_ty = value.ty();
                    if value_ty != &expected_ty {
                        return Err(context
                            .session
                            .diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(
                                op.span(),
                                "this operation expects all operands to be of the same type"
                            )
                            .with_secondary_label(
                                set_by,
                                "inferred the expected type from this value"
                            )
                            .with_secondary_label(
                                value.span(),
                                "which differs from this value"
                            )
                            .with_help(format!("expected '{expected_ty}', got '{value_ty}'"))
                            .into_report()
                        );
                    }
                }
            }

            Ok(())
        }
    }
}

derive! {
    /// Op expects all operands and results to be of the same type
    ///
    /// TODO(pauls): Implement verification for this. Ideally we could require `SameTypeOperands`
    /// as a super trait, check the operands using its implementation, and then check the results
    /// separately
    pub trait SameOperandsAndResultType {}
}

derive! {
    /// Op's regions have no arguments
    pub trait NoRegionArguments {}

    verify {
        fn no_region_arguments(op: &Operation, context: &Context) -> Result<(), Report> {
            for region in op.regions().iter() {
                if region.entry().has_arguments() {
                    return Err(context
                        .session
                        .diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid operation")
                        .with_primary_label(
                            op.span(),
                            "this operation does not permit regions with arguments, but one was found"
                        )
                        .into_report());
                }
            }

            Ok(())
        }
    }
}

derive! {
    /// Op's regions have a single block
    pub trait SingleBlock {}

    verify {
        fn has_only_single_block_regions(op: &Operation, context: &Context) -> Result<(), Report> {
            for region in op.regions().iter() {
                if region.body().iter().count() > 1 {
                    return Err(context
                        .session
                        .diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid operation")
                        .with_primary_label(
                            op.span(),
                            "this operation requires single-block regions, but regions with multiple \
                             blocks were found",
                        )
                        .into_report());
                }
            }

            Ok(())
        }
    }
}
