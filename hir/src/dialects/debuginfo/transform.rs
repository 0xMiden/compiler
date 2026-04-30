//! Transform utilities for maintaining debug info across IR transformations.
//!
//! This module provides the "transformation hooks" that make the debuginfo dialect practical.
//! Following Mojo's approach, these utilities make it easy for transform authors to keep debug info
//! valid — they only need to describe the *inverse* of their transformation.
//!
//! # Design Philosophy
//!
//! The `di` dialect uses SSA use-def chains for debug values, which means transforms *cannot*
//! silently drop debug info. When a transform replaces or deletes a value, any `di.value`
//! operations using that value must be updated. The standard `replace_all_uses_with` already
//! handles this correctly for simple value replacements.
//!
//! For more complex transforms (e.g., promoting a value to memory, splitting a value into pieces),
//! the transform author uses `salvage_debug_info` to describe how the debug expression should be
//! updated to recover the source-level value from the new representation.
//!
//! # Examples
//!
//! ## Simple value replacement (handled automatically)
//!
//! When CSE replaces `%1 = add %a, %b` with an existing `%0 = add %a, %b`:
//!
//! ```text,ignore
//! // Before: di.value %1 #[variable = x]
//! rewriter.replace_all_uses_with(%1, %0)
//! // After:  di.value %0 #[variable = x]  -- automatic!
//! ```
//!
//! ## Value promoted to memory (using `salvage_debug_info`)
//!
//! When a transform promotes a value to a stack allocation:
//!
//! ```text
//! // Before: di.value %val #[variable = x]
//! // Transform creates: %ptr = alloca T
//! //                    store %val, %ptr
//! // Call: salvage_debug_info(%val, SalvageAction::Deref { new_value: %ptr })
//! // After:  di.value %ptr #[variable = x, expression = di.expression(DW_OP_deref)]
//! ```
use alloc::vec::Vec;

use midenc_hir::{
    Builder, DialectRegistration, Operation, OperationRef, SmallVec, Spanned, ValueRef,
    dialects::debuginfo::attributes::ExpressionOp,
};

use super::{DIBuilder, ops::DebugValue};

/// Describes how to recover the original source-level value after a transformation.
///
/// When a transform changes a value's representation, it creates a [SalvageAction] describing the
/// inverse operation. The debuginfo framework then updates the `DIExpressionAttr` accordingly so
/// the debugger can still find the variable's value.
///
/// Transform authors only need to pick the right variant — the framework handles updating all
/// affected `di.value` operations.
#[derive(Clone, Debug)]
pub enum SalvageAction {
    /// The value is now behind a pointer; dereference to recover the original.
    ///
    /// Use this when a value is promoted to a stack allocation. The expression will have
    /// `DW_OP_deref` prepended.
    Deref {
        /// The new pointer value that replaces the original.
        new_value: ValueRef,
    },

    /// A constant offset was added to the value.
    ///
    /// Use this when a value is relocated by a fixed amount (e.g., frame pointer adjustments). The
    /// expression will encode the inverse subtraction.
    OffsetBy {
        /// The new value (original + offset).
        new_value: ValueRef,
        /// The offset that was added.
        offset: u64,
    },

    /// The value was replaced by a new value with an arbitrary expression.
    ///
    /// Use this for complex transformations where the simple patterns don't apply. The caller
    /// provides the full expression describing how to recover the source-level value from the new
    /// IR value.
    WithExpression {
        /// The new value replacing the original.
        new_value: ValueRef,
        /// Expression operations describing the inverse transform.
        ops: Vec<ExpressionOp>,
    },

    /// The value is now a constant.
    ///
    /// Use this when constant propagation determines the value at this point.
    Constant {
        /// The constant value.
        value: u64,
    },

    /// The value was completely removed with no recovery possible.
    ///
    /// Use this as a last resort when the value cannot be recovered. This will emit a `di.kill` for
    /// the affected variable.
    Undef,
}

/// Salvage debug info for all `di.value` operations that use `old_value`.
///
/// When a transform is about to delete or replace a value, call this function to update all debug
/// uses. The `action` describes how the debugger can recover the original source-level value from
/// the new representation.
///
/// This is the main entry point for transform authors who need to update debug info beyond simple
/// `replace_all_uses_with` scenarios.
///
/// # Example
///
/// ```rust,ignore
/// // Value was promoted to memory:
/// let ptr = builder.alloca(ty, span)?;
/// builder.store(old_val, ptr, span)?;
/// salvage_debug_info(
///     &old_val,
///     &SalvageAction::Deref { new_value: ptr },
///     &mut builder,
/// );
/// ```
pub fn salvage_debug_info<B: ?Sized + Builder>(
    old_value: &ValueRef,
    action: &SalvageAction,
    builder: &mut B,
) {
    // Collect all debug value ops that use the old value
    for mut debug_op in debug_value_users(old_value) {
        apply_salvage_action(&mut debug_op, action, builder);
    }
}

/// Apply a salvage action to a single debug value operation.
fn apply_salvage_action<B: ?Sized + Builder>(
    debug_op: &mut OperationRef,
    action: &SalvageAction,
    builder: &mut B,
) {
    let span = debug_op.borrow().span();

    match action {
        SalvageAction::Deref { new_value } => {
            // Get existing expression and prepend deref
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            expr.operations.insert(0, ExpressionOp::Deref);

            // Erase old op and create new one with updated value and expression
            debug_op.borrow_mut().erase();
            let _ = builder.debug_value_with_expr(*new_value, variable, Some(expr), span);
        }

        SalvageAction::OffsetBy { new_value, offset } => {
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            // To recover: subtract the offset that was added
            expr.operations.push(ExpressionOp::ConstU64(*offset));
            expr.operations.push(ExpressionOp::Minus);

            debug_op.borrow_mut().erase();
            let _ = builder.debug_value_with_expr(*new_value, variable, Some(expr), span);
        }

        SalvageAction::WithExpression { new_value, ops } => {
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            expr.operations.extend(ops.iter().cloned());

            debug_op.borrow_mut().erase();
            let _ = builder.debug_value_with_expr(*new_value, variable, Some(expr), span);
        }

        SalvageAction::Constant { value } => {
            let variable = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                dv.variable().as_value().clone()
            };

            debug_op.borrow_mut().erase();
            // Emit a kill since we can't create a di.value without a live SSA operand for constants
            // — the constant value is encoded in the expression
            let _ = builder.debug_kill(variable, span);
            // TODO: in the future, could emit a di.value with a materialized constant and a
            // ConstU64/StackValue expression pair
            let _ = value;
        }

        SalvageAction::Undef => {
            let variable = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                dv.variable().as_value().clone()
            };

            debug_op.borrow_mut().erase();
            let _ = builder.debug_kill(variable, span);
        }
    }
}

/// Check if an operation is a debug info operation.
///
/// This is useful for transforms that need to skip or handle debug ops differently (e.g., DCE
/// should not consider debug uses as "real" uses that keep a value alive).
pub fn is_debug_info_op(op: &Operation) -> bool {
    op.dialect().name() == super::DebugInfoDialect::NAMESPACE
}

/// Collect all `di.value` operations that reference the given value.
///
/// Useful for transforms that need to inspect or update debug info for a specific value.
pub fn debug_value_users(value: &ValueRef) -> SmallVec<[OperationRef; 2]> {
    let value = value.borrow();
    let mut ops = SmallVec::new_const();
    for user in value.iter_uses() {
        if user.owner.borrow().is::<DebugValue>() {
            ops.push(user.owner);
        }
    }
    ops
}

/// Recursively collect all debug info operations within an operation's regions.
pub fn collect_debug_ops(op: &OperationRef) -> Vec<OperationRef> {
    let mut debug_ops = Vec::new();
    collect_debug_ops_recursive(op, &mut debug_ops);
    debug_ops
}

fn collect_debug_ops_recursive(op: &OperationRef, debug_ops: &mut Vec<OperationRef>) {
    use midenc_hir::{Forward, RawWalk};

    op.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        if is_debug_info_op(&op.borrow()) {
            debug_ops.push(op);
        }
    });
}
