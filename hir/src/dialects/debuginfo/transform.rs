//! Transform utilities for maintaining debug info across IR transformations.
//!
//! This module provides the "transformation hooks" that make the debuginfo dialect practical.
//! Following Mojo's approach, these utilities make it easy for transform authors to keep debug info
//! valid — they only need to describe the *inverse* of their transformation.
//!
//! # Design Philosophy
//!
//! SSA use-def chains keep debug operands connected, but do not protect capture points or prove
//! that replacements preserve source values. Checked rewriter APIs require a [`DebugInfoUpdate`]
//! contract, and the pass manager audits records and their dependencies independently of normal
//! IR verification. Recovery expressions remain the transform author's responsibility.
//!
//! For more complex transforms (e.g., promoting a value to memory, splitting a value into pieces),
//! the transform author uses `salvage_debug_info` to describe how the debug expression should be
//! updated to recover the source-level value from the new representation.
//!
//! # Examples
//!
//! ## Equivalent value replacement
//!
//! When CSE replaces `%1 = add %a, %b` with an existing `%0 = add %a, %b`:
//!
//! ```text,ignore
//! // Before: di.debug_value %1 #[variable = x]
//! rewriter.replace_value_with_debug(%1, %0, DebugInfoUpdate::Equivalent)
//! // After:  di.debug_value %0 #[variable = x]
//! ```
//!
//! ## Value promoted to memory (using `salvage_debug_info`)
//!
//! When a transform promotes a value to a stack allocation:
//!
//! ```text
//! // Before: di.debug_value %val #[variable = x]
//! // Transform creates: %ptr = alloca T
//! //                    store %val, %ptr
//! // Call: salvage_debug_info(%val, SalvageAction::Deref { new_value: %ptr })
//! // After:  di.debug_value %ptr #[variable = x, expression = di.expression(DW_OP_deref)]
//! ```
use alloc::vec::Vec;

use midenc_hir::{
    Builder, FxHashMap, FxHashSet, Op, OpBuilder, OperationRef, ProgramPoint, Report, SmallVec,
    Spanned, ValueRef,
    dialects::debuginfo::attributes::ExpressionOp,
    patterns::{Rewriter, RewriterExt},
};

use super::{
    DIBuilder, DebugInfoMutation, DebugInfoUpdate,
    bindings::{SourceBinding, VisibilitySlot, enclosing_function},
    debug_record_variable, is_debug_record,
    ops::DebugValue,
};

/// Reconcile inline-call locations when structurally equivalent operations are merged.
///
/// Retain a chain only when both operations agree, including within corresponding regions.
/// A shared operation cannot accurately describe two distinct inline call sites. Debug records
/// are skipped: their inline chains identify source binding ownership, not instruction provenance.
pub fn merge_inline_call_locations(mut retained: OperationRef, discarded: OperationRef) {
    use super::attributes::INLINE_CALL_CHAIN_ATTR_NAME;

    if OperationRef::ptr_eq(&retained, &discarded) || is_debug_record(retained) {
        return;
    }
    let discarded = discarded.borrow();
    let mut retained = retained.borrow_mut();
    if retained.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME)
        != discarded.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME)
    {
        retained.remove_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
    }
    for (retained_region, discarded_region) in
        retained.regions().iter().zip(discarded.regions().iter())
    {
        for (retained_block, discarded_block) in
            retained_region.body().iter().zip(discarded_region.body().iter())
        {
            for (retained_op, discarded_op) in retained_block
                .body()
                .iter()
                .map(|op| op.as_operation_ref())
                .filter(|op| !is_debug_record(*op))
                .zip(
                    discarded_block
                        .body()
                        .iter()
                        .map(|op| op.as_operation_ref())
                        .filter(|op| !is_debug_record(*op)),
                )
            {
                merge_inline_call_locations(retained_op, discarded_op);
            }
        }
    }
}

/// Describes how to recover the original source-level value after a transformation.
///
/// When a transform changes a value's representation, it creates a [SalvageAction] describing the
/// inverse operation. The debuginfo framework then updates the `DIExpressionAttr` accordingly so
/// the debugger can still find the variable's value.
///
/// Transform authors only need to pick the right variant — the framework handles updating all
/// affected `di.debug_value` operations.
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
    /// provides expression operations describing how to recover the *old* IR value from the new
    /// one; they are prepended to any existing expression, which continues to describe how the
    /// source-level value is recovered from the old value.
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
    /// Use this as a last resort when the value cannot be recovered. This will emit a `di.debug_kill` for
    /// the affected variable.
    Undef,
}

/// Salvage debug info for all `di.debug_value` operations that use `old_value`.
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
    // Collect all debug value ops that use the old value.
    //
    // Each replacement op is created at the position of the debug op it replaces — a
    // `di.debug_value` records the variable's value *at a program point*, so salvaging must not
    // move that point. The caller's insertion point is restored afterwards.
    let ip = *builder.insertion_point();
    for mut debug_op in debug_value_users(old_value) {
        let _debug_update = DebugInfoMutation::new(debug_op, DebugInfoUpdate::Salvage);
        apply_salvage_action(&mut debug_op, action, builder);
    }
    builder.restore_insertion_point(ip);
}

/// Erase all `di.debug_value` operations that use `old_value`, marking each affected variable as
/// dead at that point.
///
/// Use this when a transform removes `old_value` and cannot preserve a meaningful source-level
/// location for it. A `di.debug_kill` is emitted in place of each erased operation, so that
/// debuggers report the variable as optimized out from that point on, rather than resurrecting a
/// stale earlier location. If the transform can recover the source value from another live SSA
/// value, use [`salvage_debug_info`] instead.
pub fn erase_debug_info(old_value: &ValueRef) {
    for debug_op in debug_value_users(old_value) {
        erase_debug_value(debug_op);
    }
}

/// Like [`erase_debug_info`], for use by a rewrite pattern.
///
/// The `di.debug_kill` operations are created, and the `di.debug_value` operations erased, through
/// `rewriter`, so that its listener learns about both.
pub fn erase_debug_info_with<R: ?Sized + Rewriter>(old_value: &ValueRef, rewriter: &mut R) {
    for debug_op in debug_value_users(old_value) {
        erase_debug_value_with(debug_op, rewriter);
    }
}

/// Like [`erase_debug_value`], for use by a rewrite pattern: the `di.debug_kill` is created, and
/// the `di.debug_value` erased, through `rewriter`.
pub fn erase_debug_value_with<R: ?Sized + Rewriter>(debug_op: OperationRef, rewriter: &mut R) {
    invalidate_debug_record_with(debug_op, rewriter);
}

/// Replace any location-bearing record with a kill at its original capture point.
/// A kill already represents unavailability and is left in place.
pub fn invalidate_debug_record_with<R: ?Sized + Rewriter>(
    debug_op: OperationRef,
    rewriter: &mut R,
) {
    if debug_op.name().is::<super::DebugKill>() {
        return;
    }
    let _debug_update = DebugInfoMutation::new(debug_op, DebugInfoUpdate::Invalidate);
    let (variable, span) = {
        let op = debug_op.borrow();
        let variable = debug_record_variable(&op).expect("expected a debug record");
        (variable, op.span())
    };
    {
        // The kill belongs to the inline frame of the debug value it replaces, which is not
        // necessarily the frame of the operation being rewritten.
        let mut scope = rewriter.with_inline_call_chain(debug_op);
        let ip = *scope.insertion_point();
        scope.set_insertion_point(ProgramPoint::before(debug_op));
        let _ = scope.debug_kill(variable, span);
        scope.restore_insertion_point(ip);
    }
    rewriter.erase_op(debug_op);
}

/// Replace one `di.debug_value` with a `di.debug_kill` at the same program point.
///
/// This is used by region-level DCE, which visits the transparent debug operation before the dead
/// producer whose value it references.
pub fn erase_debug_value(mut debug_op: OperationRef) {
    let _debug_update = DebugInfoMutation::new(debug_op, DebugInfoUpdate::Invalidate);
    use super::attributes::INLINE_CALL_CHAIN_ATTR_NAME;

    let (context, variable, span, inline_calls) = {
        let op = debug_op.borrow();
        let dv = op.downcast_ref::<DebugValue>().expect("expected di.debug_value");
        (
            op.context_rc(),
            dv.variable().as_value().clone(),
            op.span(),
            op.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME),
        )
    };
    let mut builder = OpBuilder::new(context);
    builder.set_insertion_point(ProgramPoint::before(debug_op));
    if let Ok(mut kill) = builder.debug_kill(variable, span)
        && let Some(inline_calls) = inline_calls
    {
        kill.borrow_mut()
            .as_operation_mut()
            .set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, inline_calls);
    }
    debug_op.borrow_mut().erase();
}

/// Apply a salvage action to a single debug value operation.
///
/// The replacement operation is created immediately before `debug_op`, which is then erased.
///
/// Inverse expression operations are always *prepended* to the existing expression: the existing
/// expression describes how to recover the source-level value from the old IR value, and the
/// salvage ops describe how to recover the old IR value from its replacement, so the salvage ops
/// must run first (`source = old_expr(salvage_ops(new_value))`).
fn apply_salvage_action<B: ?Sized + Builder>(
    debug_op: &mut OperationRef,
    action: &SalvageAction,
    builder: &mut B,
) {
    let span = debug_op.borrow().span();
    builder.set_insertion_point_before(*debug_op);

    let replacement = match action {
        SalvageAction::Deref { new_value } => {
            // Get existing expression and prepend deref
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            expr.operations.insert(0, ExpressionOp::Deref);

            // Create the replacement with updated value and expression, then erase the old op
            builder
                .debug_value_with_expr(*new_value, variable, Some(expr), span)
                .map(|replacement| replacement.as_operation_ref())
        }

        SalvageAction::OffsetBy { new_value, offset } => {
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            // To recover the old value: subtract the offset that was added, before any
            // pre-existing expression is applied
            expr.operations
                .splice(0..0, [ExpressionOp::ConstU64(*offset), ExpressionOp::Minus]);

            builder
                .debug_value_with_expr(*new_value, variable, Some(expr), span)
                .map(|replacement| replacement.as_operation_ref())
        }

        SalvageAction::WithExpression { new_value, ops } => {
            let (variable, mut expr) = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                (dv.variable().as_value().clone(), dv.expression().as_value().clone())
            };
            expr.operations.splice(0..0, ops.iter().cloned());

            builder
                .debug_value_with_expr(*new_value, variable, Some(expr), span)
                .map(|replacement| replacement.as_operation_ref())
        }

        SalvageAction::Constant { value } => {
            let variable = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                dv.variable().as_value().clone()
            };

            // Emit a kill since we can't create a di.debug_value without a live SSA operand for constants
            // — the constant value is encoded in the expression
            // TODO: in the future, could emit a di.debug_value with a materialized constant and a
            // ConstU64/StackValue expression pair
            let _ = value;
            builder
                .debug_kill(variable, span)
                .map(|replacement| replacement.as_operation_ref())
        }

        SalvageAction::Undef => {
            let variable = {
                let op = debug_op.borrow();
                let dv = op.downcast_ref::<DebugValue>().unwrap();
                dv.variable().as_value().clone()
            };

            builder
                .debug_kill(variable, span)
                .map(|replacement| replacement.as_operation_ref())
        }
    };
    if let Ok(mut replacement) = replacement {
        use super::attributes::INLINE_CALL_CHAIN_ATTR_NAME;

        let inline_calls = debug_op.borrow().get_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
        if let Some(inline_calls) = inline_calls {
            replacement
                .borrow_mut()
                .set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, inline_calls);
        } else {
            replacement.borrow_mut().remove_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
        }
        super::mutation::register_debug_record(replacement);
        debug_op.borrow_mut().erase();
    }
}

/// Collect all `di.debug_value` operations that reference the given value.
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

/// Recursively collect source-variable capture records within an operation.
pub fn collect_debug_ops(op: &OperationRef) -> Vec<OperationRef> {
    let mut debug_ops = Vec::new();
    if !op.context_ref().debug_info_mutations.borrow().has_observed_records()
        && !is_debug_record(*op)
    {
        return debug_ops;
    }
    collect_debug_ops_recursive(op, &mut debug_ops);
    debug_ops
}

fn collect_debug_ops_recursive(op: &OperationRef, debug_ops: &mut Vec<OperationRef>) {
    use midenc_hir::{Forward, RawWalk};

    op.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        if is_debug_record(op) {
            debug_ops.push(op);
        }
    });
}

/// Source binding identities known to a rewrite, scoped by function and inline frame.
///
/// This is an inventory, not a data-flow claim about which binding is currently live. Reuse it
/// across owner erasures when a transform does not introduce new source binding identities.
/// Keeping it separate from the audit avoids exposing capture snapshots or mutation authority.
pub struct DebugBindingInventory {
    bindings: Vec<SourceBinding>,
    visibility: FxHashMap<VisibilitySlot, Vec<usize>>,
}

impl DebugBindingInventory {
    pub fn new(root: OperationRef) -> Self {
        #[cfg(test)]
        tests::BUILDS.with(|count| count.set(count.get() + 1));
        let mut bindings = Vec::new();
        let mut visibility: FxHashMap<VisibilitySlot, Vec<usize>> = FxHashMap::default();
        let mut seen = FxHashSet::default();
        for record in collect_debug_ops(&root) {
            let binding = SourceBinding::of(record).expect("expected debug record");
            if seen.insert(binding.clone()) {
                visibility.entry(binding.visibility()).or_default().push(bindings.len());
                bindings.push(binding);
            }
        }
        Self {
            bindings,
            visibility,
        }
    }

    /// Invalidate captures at the original owner's program point before its erasure.
    ///
    /// Conditional capture effects cannot generally be hoisted. Invalidate every possible binding
    /// in each affected visibility slot, including a prior binding a removed shadow could expose.
    /// Each disappearing record has an exact origin/anchor mapping; additional kills only reduce
    /// availability. Resolve record disappearance here, independently of later result RAUW.
    pub fn invalidate_captures<R: ?Sized + Rewriter>(
        &self,
        owner: OperationRef,
        rewriter: &mut R,
    ) -> Result<(), Report> {
        self.invalidate_records(owner, collect_debug_ops(&owner), rewriter)
    }

    fn invalidate_records<R: ?Sized + Rewriter>(
        &self,
        owner: OperationRef,
        records: Vec<OperationRef>,
        rewriter: &mut R,
    ) -> Result<(), Report> {
        if records.is_empty() {
            return Ok(());
        }
        let mut originals: FxHashMap<SourceBinding, Vec<OperationRef>> = FxHashMap::default();
        let mut affected = Vec::new();
        let mut slots = FxHashSet::default();
        for &record in &records {
            let binding = SourceBinding::of(record).expect("expected debug record");
            if !originals.contains_key(&binding) {
                slots.insert(binding.visibility());
                affected.push(binding.clone());
            }
            originals.entry(binding).or_default().push(record);
        }
        // Preserve the old deterministic order: owner identities first, then inventory order.
        let mut additional = slots
            .iter()
            .filter_map(|slot| self.visibility.get(slot))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        additional.sort_unstable();
        for index in additional {
            let binding = &self.bindings[index];
            if !originals.contains_key(binding) {
                affected.push(binding.clone());
            }
        }
        // The owner scope also includes external records depending on its results. They remain
        // unchanged here, so use Salvage for the mixed scope; each disappearing nested capture
        // is still explicitly mapped to the exact kill we create below.
        let mut update = DebugInfoMutation::new(owner, DebugInfoUpdate::Salvage);
        for binding in affected {
            let span = owner.borrow().span();
            let ip = *rewriter.insertion_point();
            let previous_chain = rewriter.replace_inline_call_chain(
                binding.identity.inline_calls.clone().map(|chain| {
                    rewriter
                        .context_rc()
                        .create_attribute::<super::attributes::InlineCallChainAttr, _>(chain)
                        .as_attribute_ref()
                }),
            );
            rewriter.set_insertion_point_before(owner);
            let replacement = rewriter.debug_kill(binding.identity.variable.clone(), span);
            rewriter.restore_insertion_point(ip);
            rewriter.replace_inline_call_chain(previous_chain);
            let replacement = replacement?.as_operation_ref();
            if let Some(records) = originals.get(&binding) {
                for record in records {
                    update.replace_capture(*record, replacement, owner)?;
                }
            }
        }
        // Keep the anchor attached until every replacement has been inserted, including when
        // the owner itself is a record and its slot contains several shadow declarations.
        for record in records {
            rewriter.erase_op(record);
        }
        Ok(())
    }
}

/// Invalidate an owner's captures using all binding identities in its containing function.
/// Repeated erasures can reuse a [`DebugBindingInventory`] to avoid repeated function walks.
pub fn invalidate_debug_captures<R: ?Sized + Rewriter>(
    owner: OperationRef,
    rewriter: &mut R,
) -> Result<(), Report> {
    let records = collect_debug_ops(&owner);
    if records.is_empty() {
        return Ok(());
    }
    let root = enclosing_function(owner).unwrap_or(owner);
    let inventory = match rewriter.debug_binding_inventory_cache() {
        Some(cache) => cache.inventory(root),
        None => alloc::rc::Rc::new(DebugBindingInventory::new(root)),
    };
    inventory.invalidate_records(owner, records, rewriter)
}

/// Rewrite-local inventory storage. Reuse is enabled only by a driver with an explicit lifecycle.
/// Ordinary rewriters and arbitrary patterns always receive a fresh inventory.
#[derive(Default)]
pub struct DebugBindingInventoryCache {
    enabled: bool,
    preserving: bool,
    inventory: Option<(OperationRef, alloc::rc::Rc<DebugBindingInventory>)>,
}

impl DebugBindingInventoryCache {
    pub(crate) fn enable(&mut self) {
        self.enabled = true;
    }

    pub(crate) fn invalidate(&mut self) {
        self.inventory = None;
    }

    pub(crate) fn enter(&mut self, preserving: bool) -> bool {
        core::mem::replace(&mut self.preserving, preserving)
    }

    pub(crate) fn inventory(&mut self, root: OperationRef) -> alloc::rc::Rc<DebugBindingInventory> {
        if !self.enabled || !self.preserving {
            return alloc::rc::Rc::new(DebugBindingInventory::new(root));
        }
        if let Some((owner, inventory)) = &self.inventory
            && *owner == root
        {
            return inventory.clone();
        }
        let inventory = alloc::rc::Rc::new(DebugBindingInventory::new(root));
        self.inventory = Some((root, inventory.clone()));
        inventory
    }
}

#[cfg(test)]
#[path = "transform/tests.rs"]
mod tests;
