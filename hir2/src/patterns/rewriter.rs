#![allow(unused)]
use alloc::rc::Rc;
use core::ops::{Deref, DerefMut};

use crate::{
    BlockRef, Builder, Context, InsertionPoint, Listener, ListenerType, OpBuilder, OpOperand,
    OpResultRef, OperationRef, Pattern, RegionRef, Report, SourceSpan, Type, ValueRef,
};

/// A special type of `RewriterBase` that coordinates the application of a rewrite pattern on the
/// current IR being matched, providing a way to keep track of any mutations made.
///
/// This type should be used to perform all necessary IR mutations within a rewrite pattern, as
/// the pattern driver may be tracking various state that would be invalidated when a mutation takes
/// place.
pub struct PatternRewriter {
    rewriter: RewriterImpl,
    recoverable: bool,
}
impl PatternRewriter {
    pub fn new(builder: OpBuilder) -> Self {
        Self {
            rewriter: RewriterImpl::new(builder),
            recoverable: false,
        }
    }

    #[inline]
    pub const fn can_recover_from_rewrite_failure(&self) -> bool {
        self.recoverable
    }
}
impl Deref for PatternRewriter {
    type Target = RewriterImpl;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.rewriter
    }
}
impl DerefMut for PatternRewriter {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rewriter
    }
}

pub struct RewriterImpl {
    builder: OpBuilder,
    listener: Option<Box<dyn RewriterListener>>,
}

impl Listener for RewriterImpl {
    fn kind(&self) -> ListenerType {
        ListenerType::Rewriter
    }

    fn notify_block_inserted(
        &mut self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<InsertionPoint>,
    ) {
        if let Some(listener) = self.listener.as_deref_mut() {
            listener.notify_block_inserted(block, prev, ip);
        } else {
            self.builder.notify_block_inserted(block, prev, ip);
        }
    }

    fn notify_operation_inserted(&mut self, op: OperationRef, prev: Option<InsertionPoint>) {
        if let Some(listener) = self.listener.as_deref_mut() {
            listener.notify_operation_inserted(op, prev);
        } else {
            self.builder.notify_operation_inserted(op, prev);
        }
    }
}

impl Builder for RewriterImpl {
    #[inline(always)]
    fn context(&self) -> &Context {
        self.builder.context()
    }

    #[inline(always)]
    fn context_rc(&self) -> Rc<Context> {
        self.builder.context_rc()
    }

    #[inline(always)]
    fn insertion_point(&self) -> Option<&InsertionPoint> {
        self.builder.insertion_point()
    }

    #[inline(always)]
    fn clear_insertion_point(&mut self) -> Option<InsertionPoint> {
        self.builder.clear_insertion_point()
    }

    #[inline(always)]
    fn restore_insertion_point(&mut self, ip: Option<InsertionPoint>) {
        self.builder.restore_insertion_point(ip);
    }

    #[inline(always)]
    fn set_insertion_point(&mut self, ip: InsertionPoint) {
        self.builder.set_insertion_point(ip);
    }

    #[inline]
    fn create_block<P>(
        &mut self,
        parent: RegionRef,
        ip: Option<InsertionPoint>,
        args: P,
    ) -> BlockRef
    where
        P: IntoIterator<Item = Type>,
    {
        self.builder.create_block(parent, ip, args)
    }

    #[inline]
    fn create_block_before<P>(&mut self, before: BlockRef, args: P) -> BlockRef
    where
        P: IntoIterator<Item = Type>,
    {
        self.builder.create_block_before(before, args)
    }

    #[inline]
    fn insert(&mut self, op: OperationRef) {
        self.builder.insert(op);
    }
}

impl RewriterImpl {
    pub fn new(builder: OpBuilder) -> Self {
        Self {
            builder,
            listener: None,
        }
    }

    pub fn with_listener(mut self, listener: impl RewriterListener) -> Self {
        self.listener = Some(Box::new(listener));
        self
    }

    /// Move the blocks that belong to `region` before the given insertion point in another region,
    /// `ip`. The two regions must be different. The caller is responsible for creating or
    /// updating the operation transferring flow of control to the region, and passing it the
    /// correct block arguments.
    pub fn inline_region_before(&mut self, region: RegionRef, ip: InsertionPoint) {
        todo!()
    }

    /// Replace the results of the given operation with the specified list of values (replacements).
    ///
    /// The result types of the given op and the replacements must match. The original op is erased.
    pub fn replace_op_with_values<V>(&mut self, op: OperationRef, values: V)
    where
        V: IntoIterator<Item = ValueRef>,
    {
        todo!()
    }

    /// Replace the results of the given operation with the specified replacement op.
    ///
    /// The result types of the two ops must match. The original op is erased.
    pub fn replace_op(&mut self, op: OperationRef, new_op: OperationRef) {
        todo!()
    }

    /// This method erases an operation that is known to have no uses.
    pub fn erase_op(&mut self, op: OperationRef) {
        todo!()
    }

    /// This method erases all operations in a block.
    pub fn erase_block(&mut self, block: BlockRef) {
        todo!()
    }

    /// Inline the operations of block `src` before the given insertion point.
    /// The source block will be deleted and must have no uses. The `args` values, if provided, are
    /// used to replace the block arguments of `src`.
    ///
    /// If the source block is inserted at the end of the dest block, the dest block must have no
    /// successors. Similarly, if the source block is inserted somewhere in the middle (or
    /// beginning) of the dest block, the source block must have no successors. Otherwise, the
    /// resulting IR would have unreachable operations.
    pub fn inline_block_before(
        &mut self,
        src: BlockRef,
        ip: InsertionPoint,
        args: Option<&[ValueRef]>,
    ) {
        todo!()
    }

    /// Inline the operations of block `src` into the end of block `dest`. The source block will be
    /// deleted and must have no uses. The `args` values, if present, are used to replace the block
    /// arguments of `src`.
    ///
    /// The dest block must have no successors. Otherwise, the resulting IR will have unreachable
    /// operations.
    pub fn merge_blocks(&mut self, src: BlockRef, dest: BlockRef, args: Option<&[ValueRef]>) {
        todo!()
    }

    /// Split the operations starting at `ip` (inclusive) out of the given block into a new block,
    /// and return it.
    pub fn split_block(&mut self, block: BlockRef, ip: InsertionPoint) -> BlockRef {
        todo!()
    }

    /// Unlink this operation from its current block and insert it right before `ip`, which
    /// may be in the same or another block in the same function.
    pub fn move_op_before(&mut self, op: OperationRef, ip: InsertionPoint) {
        todo!()
    }

    /// Unlink this operation from its current block and insert it right after `ip`, which may be
    /// in the same or another block in the same function.
    pub fn move_op_after(&mut self, op: OperationRef, ip: InsertionPoint) {
        todo!()
    }

    /// Unlink this block and insert it right before `ip`.
    pub fn move_block_before(&mut self, block: BlockRef, ip: InsertionPoint) {
        todo!()
    }

    /// This method is used to notify the rewriter that an in-place operation modification is about
    /// to happen.
    ///
    /// The returned guard can be used to access the rewriter, as well as finalize or cancel the
    /// in-place modification.
    pub fn start_in_place_modification(
        &mut self,
        op: OperationRef,
    ) -> InPlaceModificationGuard<'_> {
        InPlaceModificationGuard::new(self, op)
    }

    /// Performs an in-place modification of `root` using `callback`, taking care of notifying the
    /// rewriter of progress and outcome of the modification.
    pub fn modify_op_in_place<F>(&mut self, root: OperationRef, callback: F)
    where
        F: Fn(InPlaceModificationGuard<'_>),
    {
        let guard = self.start_in_place_modification(root);
        callback(guard);
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    pub fn replace_all_uses_of_value_with(&mut self, from: ValueRef, to: ValueRef) {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    pub fn replace_all_uses_of_block_with(&mut self, from: BlockRef, to: BlockRef) {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    pub fn replace_all_uses_with(&mut self, from: &[ValueRef], to: &[ValueRef]) {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place modification (for every use that was replaced),
    /// and that the `from` operation is about to be replaced.
    pub fn replace_all_op_uses_with_values(&mut self, from: OperationRef, to: &[ValueRef]) {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place modification (for every use that was replaced),
    /// and that the `from` operation is about to be replaced.
    pub fn replace_all_op_uses_with(&mut self, from: OperationRef, to: OperationRef) {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`, if `predicate` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    pub fn maybe_replace_uses_of_value_with<P>(
        &mut self,
        from: ValueRef,
        to: ValueRef,
        predicate: P,
    ) -> bool
    where
        P: Fn(OpOperand) -> bool,
    {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`, if `predicate` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    pub fn maybe_replace_uses_with<P>(
        &mut self,
        from: &[ValueRef],
        to: &[ValueRef],
        predicate: P,
    ) -> bool
    where
        P: Fn(OpOperand) -> bool,
    {
        todo!()
    }

    /// Find uses of `from` and replace them with `to`, if `predicate` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    pub fn maybe_replace_op_uses_with<P>(
        &mut self,
        from: OperationRef,
        to: &[ValueRef],
        predicate: P,
    ) -> bool
    where
        P: Fn(OpOperand) -> bool,
    {
        todo!()
    }

    /// Find uses of `from` within `block` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    pub fn replace_op_uses_within_block(
        &mut self,
        from: OperationRef,
        to: &[ValueRef],
        block: BlockRef,
    ) -> bool {
        let parent_op = block.borrow().parent_op();
        self.maybe_replace_op_uses_with(from, to, |operand| {
            let operand = operand.borrow();
            !parent_op
                .as_ref()
                .is_some_and(|op| op.borrow().is_proper_ancestor_of(operand.owner.clone()))
        })
    }

    /// Find uses of `from` and replace them with `to`, except if the user is in `exceptions`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    pub fn replace_all_uses_except(
        &mut self,
        from: ValueRef,
        to: ValueRef,
        exceptions: &[OperationRef],
    ) {
        self.maybe_replace_uses_of_value_with(from, to, |operand| {
            let operand = operand.borrow();
            !exceptions.contains(&operand.owner)
        });
    }

    pub fn notify_match_failure(&mut self, span: SourceSpan, report: Report) {
        if let Some(listener) = self.listener.as_mut() {
            listener.notify_match_failure(span, report);
        }
    }
}

/// Wraps an in-place modification of an [Operation] to ensure the rewriter is properly notified
/// about the progress and outcome of the in-place notification.
///
/// This is a minor efficiency win, as it avoids creating a new operation, and removing the old one,
/// but also often allows simpler code in the client.
pub struct InPlaceModificationGuard<'a> {
    rewriter: &'a mut RewriterImpl,
    op: OperationRef,
    canceled: bool,
}
impl<'a> InPlaceModificationGuard<'a> {
    fn new(rewriter: &'a mut RewriterImpl, op: OperationRef) -> Self {
        Self {
            rewriter,
            op,
            canceled: false,
        }
    }

    #[inline]
    pub fn rewriter(&mut self) -> &mut RewriterImpl {
        self.rewriter
    }

    #[inline]
    pub fn op(&self) -> &OperationRef {
        &self.op
    }

    /// Cancels the pending in-place modification.
    pub fn cancel(mut self) {
        self.canceled = true;
    }

    /// Signals the end of an in-place modification of the current operation.
    pub fn finalize(self) {}
}
impl core::ops::Deref for InPlaceModificationGuard<'_> {
    type Target = RewriterImpl;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.rewriter
    }
}
impl core::ops::DerefMut for InPlaceModificationGuard<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.rewriter
    }
}
impl Drop for InPlaceModificationGuard<'_> {
    fn drop(&mut self) {
        if self.canceled {
            //self.rewriter.cancel_op_modification(self.op.clone());
            todo!("cancel op modification")
        } else {
            //self.rewriter.finalize_op_modification(self.op.clone());
            todo!("finalize op modification")
        }
    }
}

pub trait RewriterListener: Listener {
    /// Notify the listener that the specified block is about to be erased.
    ///
    /// At this point, the block has zero uses.
    fn notify_block_erased(&mut self, block: BlockRef) {}

    /// Notify the listener that the specified operation was modified in-place.
    fn notify_operation_modified(&mut self, op: OperationRef) {}

    /// Notify the listener that all uses of the specified operation's results are about to be
    /// replaced with the results of another operation. This is called before the uses of the old
    /// operation have been changed.
    ///
    /// By default, this function calls the "operation replaced with values" notification.
    fn notify_operation_replaced(&mut self, op: OperationRef, replacement: OperationRef) {
        let replacement = replacement.borrow();
        self.notify_operation_replaced_with_values(op, replacement.results().all().as_slice());
    }

    /// Notify the listener that all uses of the specified operation's results are about to be
    /// replaced with the given range of values, potentially produced by other operations. This is
    /// called before the uses of the operation have been changed.
    fn notify_operation_replaced_with_values(
        &mut self,
        op: OperationRef,
        replacement: &[OpResultRef],
    ) {
    }

    /// Notify the listener that the specified operation is about to be erased. At this point, the
    /// operation has zero uses.
    ///
    /// NOTE: This notification is not triggered when unlinking an operation.
    fn notify_operation_erased(&mut self, op: OperationRef) {}

    /// Notify the listener that the specified pattern is about to be applied at the specified root
    /// operation.
    fn notify_pattern_begin(&mut self, pattern: &Pattern, op: OperationRef) {}

    /// Notify the listener that a pattern application finished with the specified status.
    ///
    /// `Ok` indicates that the pattern was applied successfully. `Err` indicates that the pattern
    /// could not be applied. The pattern may have communicated the reason for the failure with
    /// `notify_match_failure`
    fn notify_pattern_end(&mut self, pattern: &Pattern, status: Result<(), Report>) {}

    /// Notify the listener that the pattern failed to match, and provide a diagnostic explaining
    /// the reason why the failure occurred.
    fn notify_match_failure(&mut self, span: SourceSpan, reason: Report) {}
}

struct RewriterListenerBase {
    kind: ListenerType,
}
impl Listener for RewriterListenerBase {
    #[inline(always)]
    fn kind(&self) -> ListenerType {
        ListenerType::Rewriter
    }

    fn notify_block_inserted(
        &mut self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<InsertionPoint>,
    ) {
        todo!()
    }

    fn notify_operation_inserted(&mut self, op: OperationRef, prev: Option<InsertionPoint>) {
        todo!()
    }
}
