mod foldable;
mod info;
mod types;

use midenc_session::diagnostics::Severity;

pub(crate) use self::info::TraitInfo;
pub use self::{
    foldable::{FoldResult, Foldable, OpFoldResult},
    types::*,
};
use crate::{derive, AttributeValue, Context, Operation, Report, Spanned};

/// Marker trait for commutative ops, e.g. `X op Y == Y op X`
pub trait Commutative {}

/// Marker trait for constant-like ops
pub trait ConstantLike {}

/// Marker trait for ops with side effects
pub trait HasSideEffects {}

/// Marker trait for ops with recursive memory effects, i.e. the effects of the operation includes
/// the effects of operations nested within its regions. If the operation does not implement any
/// effect markers, e.g. `MemoryWrite`, then it can be assumed to have no memory effects itself.
pub trait HasRecursiveMemoryEffects {}

/// Marker trait for ops which allocate memory
pub trait MemoryAlloc {}

/// Marker trait for ops which free memory
pub trait MemoryFree {}

/// Marker trait for ops which read memory
pub trait MemoryRead {}

/// Marker trait for ops which write memory
pub trait MemoryWrite {}

/// Marker trait for return-like ops
pub trait ReturnLike {}

/// Op is a terminator (i.e. it can be used to terminate a block)
pub trait Terminator {}

/// Op's regions do not require blocks to end with a [Terminator]
pub trait NoTerminator {}

/// Marker trait for idemptoent ops, i.e. `op op X == op X (unary) / X op X == X (binary)`
pub trait Idempotent {}

/// Marker trait for ops that exhibit the property `op op X == X`
pub trait Involution {}

/// Marker trait for ops which are not permitted to access values defined above them
pub trait IsolatedFromAbove {}

/// Marker trait for ops which have only regions of [`RegionKind::Graph`]
pub trait HasOnlyGraphRegion {}

/// Op's regions are all single-block graph regions, that not require a terminator
///
/// This trait _cannot_ be derived via `derive!`
pub trait GraphRegionNoTerminator:
    NoTerminator + SingleBlock + crate::RegionKindInterface + HasOnlyGraphRegion
{
}

// TODO(pauls): Implement verifier
/// This interface provides information for branching terminator operations, i.e. terminator
/// operations with successors.
///
/// This interface is meant to model well-defined cases of control-flow of value propagation, where
/// what occurs along control-flow edges is assumed to be side-effect free. For example,
/// corresponding successor operands and successor block arguments may have different types. In such
/// cases, `are_types_compatible` can be implemented to compare types along control-flow edges. By
/// default, type equality is used.
pub trait BranchOpInterface: crate::Op {
    /// Returns the operands that correspond to the arguments of the successor at `index`.
    ///
    /// It consists of a number of operands that are internally produced by the operation, followed
    /// by a range of operands that are forwarded. An example operation making use of produced
    /// operands would be:
    ///
    /// ```hir,ignore
    /// invoke %function(%0)
    ///     label ^success ^error(%1 : i32)
    ///
    /// ^error(%e: !error, %arg0: i32):
    ///     ...
    ///```
    ///
    /// The operand that would map to the `^error`s `%e` operand is produced by the `invoke`
    /// operation, while `%1` is a forwarded operand that maps to `%arg0` in the successor.
    ///
    /// Produced operands always map to the first few block arguments of the successor, followed by
    /// the forwarded operands. Mapping them in any other order is not supported by the interface.
    ///
    /// By having the forwarded operands last allows users of the interface to append more forwarded
    /// operands to the branch operation without interfering with other successor operands.
    fn get_successor_operands(&self, index: usize) -> crate::SuccessorOperandRange<'_> {
        let op = <Self as crate::Op>::as_operation(self);
        let operand_group = op.successors()[index].operand_group as usize;
        crate::SuccessorOperandRange::forward(op.operands().group(operand_group))
    }
    /// The mutable version of [Self::get_successor_operands].
    fn get_successor_operands_mut(&mut self, index: usize) -> crate::SuccessorOperandRangeMut<'_> {
        let op = <Self as crate::Op>::as_operation_mut(self);
        let operand_group = op.successors()[index].operand_group as usize;
        crate::SuccessorOperandRangeMut::forward(op.operands_mut().group_mut(operand_group))
    }
    /// Returns the block argument of the successor corresponding to the operand at `operand_index`.
    ///
    /// Returns `None` if the specified operand is not a successor operand.
    fn get_successor_block_argument(
        &self,
        operand_index: usize,
    ) -> Option<crate::BlockArgumentRef> {
        let op = <Self as crate::Op>::as_operation(self);
        let operand_groups = op.operands().num_groups();
        let mut next_index = 0usize;
        for operand_group in 0..operand_groups {
            let group_size = op.operands().group(operand_group).len();
            if (next_index..(next_index + group_size)).contains(&operand_index) {
                let arg_index = operand_index - next_index;
                // We found the operand group, now map that to a successor
                let succ_info =
                    op.successors().iter().find(|s| operand_group == s.operand_group as usize)?;
                return succ_info.block.borrow().block.borrow().arguments().get(arg_index).cloned();
            }

            next_index += group_size;
        }

        None
    }
    /// Returns the successor that would be chosen with the given constant operands.
    ///
    /// Returns `None` if a single successor could not be chosen.
    #[inline]
    #[allow(unused_variables)]
    fn get_successor_for_operands(
        &self,
        operands: &[Box<dyn AttributeValue>],
    ) -> Option<crate::BlockRef> {
        None
    }
    /// This is called to compare types along control-flow edges.
    ///
    /// By default, types must be exactly equal to be compatible.
    fn are_types_compatible(&self, lhs: &crate::Type, rhs: &crate::Type) -> bool {
        lhs == rhs
    }
}

/// This interface provides information for select-like operations, i.e., operations that forward
/// specific operands to the output, depending on a binary condition.
///
/// If the value of the condition is 1, then the `true` operand is returned, and the third operand
/// is ignored, even if it was poison.
///
/// If the value of the condition is 0, then the `false` operand is returned, and the second operand
/// is ignored, even if it was poison.
///
/// If the condition is poison, then poison is returned.
///
/// Implementing operations can also accept shaped conditions, in which case the operation works
/// element-wise.
pub trait SelectLikeOpInterface {
    /// Returns the operand that represents the boolean condition for this select-like op.
    fn get_condition(&self) -> crate::ValueRef;
    /// Returns the operand that would be chosen for a true condition.
    fn get_true_value(&self) -> crate::ValueRef;
    /// Returns the operand that would be chosen for a false condition.
    fn get_false_value(&self) -> crate::ValueRef;
}

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

// pub trait SingleBlockImplicitTerminator<T: Op + Default> {}

derive! {
    /// Op has a single region
    pub trait SingleRegion {}

    verify {
        fn has_exactly_one_region(op: &Operation, context: &Context) -> Result<(), Report> {
            let num_regions = op.num_regions();
            if num_regions != 1 {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operation")
                    .with_primary_label(
                        op.span(),
                        format!("this operation requires exactly one region, but got {num_regions}")
                    )
                    .into_report());
            }

            Ok(())
        }
    }
}

// pub trait HasParent<T> {}
// pub trait ParentOneOf<(T,...)> {}
