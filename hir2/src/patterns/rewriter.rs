use alloc::rc::Rc;
use core::ops::{Deref, DerefMut};

use smallvec::SmallVec;

use crate::{
    Block, BlockRef, Builder, Context, EntityWithParent, InsertionGuard, InsertionPoint, Listener,
    ListenerType, OpBuilder, OpOperandImpl, Operation, OperationRef, Pattern, PostOrderBlockIter,
    ProgramPoint, RegionRef, Report, SourceSpan, Usable, ValueRef,
};

/// A [Rewriter] is a [Builder] extended with additional functionality that is of primary use when
/// rewriting the IR after it is initially constructed. It is the basis on which the pattern
/// rewriter infrastructure is built.
pub trait Rewriter: Builder + RewriterListener {
    /// Returns true if this rewriter has a listener attached.
    ///
    /// When no listener is present, fast paths can be taken when rewriting the IR, whereas a
    /// listener requires breaking mutations up into individual actions so that the listener can
    /// be made aware of all of them, in the order they occur.
    fn has_listener(&self) -> bool;

    /// Replace the results of the given operation with the specified list of values (replacements).
    ///
    /// The result types of the given op and the replacements must match. The original op is erased.
    fn replace_op_with_values(&mut self, op: OperationRef, values: &[ValueRef]) {
        assert_eq!(op.borrow().num_results(), values.len());

        // Replace all result uses, notifies listener of the modifications
        self.replace_all_op_uses_with_values(op.clone(), values);

        // Erase the op and notify the listener
        self.erase_op(op);
    }

    /// Replace the results of the given operation with the specified replacement op.
    ///
    /// The result types of the two ops must match. The original op is erased.
    fn replace_op(&mut self, op: OperationRef, new_op: OperationRef) {
        assert_eq!(op.borrow().num_results(), new_op.borrow().num_results());

        // Replace all result uses, notifies listener of the modifications
        self.replace_all_op_uses_with(op.clone(), new_op);

        // Erase the op and notify the listener
        self.erase_op(op);
    }

    /// This method erases an operation that is known to have no uses.
    fn erase_op(&mut self, mut op: OperationRef) {
        assert!(!op.borrow().is_used(), "expected op to have no uses");

        // If no listener is attached, the op can be dropped all at once.
        if self.has_listener() {
            op.borrow_mut().erase();
            return;
        }

        // Helper function that erases a single operation
        fn erase_single_op<R: ?Sized + RewriterListener>(
            mut op: OperationRef,
            rewrite_listener: &mut R,
        ) {
            let mut op_mut = op.borrow_mut();
            if cfg!(debug_assertions) {
                // All nested ops should have been erased already
                assert!(op_mut.regions().iter().all(|r| r.is_empty()), "expected empty regions");
                // All users should have been erased already if the op is in a region with SSA dominance
                if op_mut.is_used() {
                    if let Some(region) = op_mut.parent_region() {
                        assert!(
                            region.borrow().may_be_graph_region(),
                            "expected that op has no uses"
                        );
                    }
                }
            }

            rewrite_listener.notify_operation_erased(op);

            // Explicitly drop all uses in case the op is in a graph region
            op_mut.drop_all_uses();
            op_mut.erase();
        }

        // Nested ops must be erased one-by-one, so that listeners have a consistent view of the
        // IR every time a notification is triggered. Users must be erased before definitions, i.e.
        // in post-order, reverse dominance.
        fn erase_tree<R: ?Sized + Rewriter>(op_ref: OperationRef, rewriter: &mut R) {
            // Erase nested ops
            let op = op_ref.borrow();
            for region in op.regions() {
                // Erase all blocks in the right order. Successors should be erased before
                // predecessors because successor blocks may use values defined in predecessor
                // blocks. A post-order traversal of blocks within a region visits successors before
                // predecessors. Repeat the traversal until the region is empty. (The block graph
                // could be disconnected.)
                let mut erased_blocks = SmallVec::<[BlockRef; 4]>::default();
                while !region.is_empty() {
                    erased_blocks.clear();
                    for block_ref in PostOrderBlockIter::new(region.entry_block_ref().unwrap()) {
                        let block = block_ref.borrow();
                        let mut cursor = block.body().front();
                        while let Some(op) = cursor.as_pointer() {
                            erase_tree(op, rewriter);
                            cursor.move_next();
                        }
                        erased_blocks.push(block_ref);
                    }
                    for mut block in erased_blocks.drain(..) {
                        // Explicitly drop all uses in case there is a cycle in the block
                        // graph.
                        for arg in block.borrow_mut().arguments_mut() {
                            arg.borrow_mut().uses_mut().clear();
                        }
                        block.borrow_mut().drop_all_uses();
                        rewriter.erase_block(block);
                    }
                }
            }
            erase_single_op(op_ref, rewriter);
        }

        erase_tree(op, self);
    }

    /// This method erases all operations in a block.
    fn erase_block(&mut self, mut block: BlockRef) {
        assert!(!block.borrow().is_used(), "expected 'block' to be unused");

        let mut blk = block.borrow_mut();
        let mut cursor = blk.body_mut().back_mut();
        while let Some(op) = cursor.remove() {
            assert!(!op.borrow().is_used(), "expected 'op' to be unused");
            self.erase_op(op);
        }

        // Notify the listener that the block is about to be removed.
        self.notify_block_erased(block.clone());

        // Remove block from parent region
        let mut region = blk.parent().expect("expected 'block' to have a parent region");
        let mut region_mut = region.borrow_mut();
        let mut cursor = unsafe { region_mut.body_mut().cursor_mut_from_ptr(block.clone()) };
        cursor.remove();
    }

    /// Move the blocks that belong to `region` before the given insertion point in another region,
    /// `ip`. The two regions must be different. The caller is responsible for creating or
    /// updating the operation transferring flow of control to the region, and passing it the
    /// correct block arguments.
    fn inline_region_before(&mut self, mut region: RegionRef, ip: BlockRef) {
        let mut parent = ip.borrow().parent().expect("invalid 'ip': must be attached to a region");
        assert!(!RegionRef::ptr_eq(&region, &parent), "cannot inline a region into itself");
        let mut region_body = region.borrow_mut().body_mut().take();
        if !self.has_listener() {
            {
                let mut region_cursor = region_body.front_mut();
                while let Some(block) = region_cursor.as_pointer() {
                    Block::on_inserted_into_parent(block, parent.clone());
                    region_cursor.move_next();
                }
            }
            let mut parent_region = parent.borrow_mut();
            let parent_body = parent_region.body_mut();
            let mut cursor = unsafe { parent_body.cursor_mut_from_ptr(ip.clone()) };
            cursor.splice_before(region_body);
        } else {
            // Move blocks from beginning of the region one-by-one
            for block in region_body {
                self.move_block_before(block, ip.clone());
            }
        }
    }

    /// Inline the operations of block `src` before the given insertion point.
    /// The source block will be deleted and must have no uses. The `args` values, if provided, are
    /// used to replace the block arguments of `src`.
    ///
    /// If the source block is inserted at the end of the dest block, the dest block must have no
    /// successors. Similarly, if the source block is inserted somewhere in the middle (or
    /// beginning) of the dest block, the source block must have no successors. Otherwise, the
    /// resulting IR would have unreachable operations.
    fn inline_block_before(&mut self, mut src: BlockRef, ip: OperationRef, args: &[ValueRef]) {
        assert!(
            args.len() == src.borrow().num_arguments(),
            "incorrect # of argument replacement values"
        );

        // The source block will be deleted, so it should not have any users (i.e., there should be
        // no predecessors).
        assert!(!src.borrow().has_predecessors(), "expected 'src' to have no predecessors");

        let mut dest = ip.borrow().parent().expect("expected 'ip' to belong to a block");
        let insert_at_block_end =
            OperationRef::ptr_eq(&ip, &dest.borrow().body().back().as_pointer().unwrap());
        if insert_at_block_end {
            // The source block will be inserted at the end of the dest block, so the
            // dest block should have no successors. Otherwise, the inserted operations
            // will be unreachable.
            assert!(!dest.borrow().has_successors(), "expected 'dest' to have no successors");
        } else {
            // The source block will be inserted in the middle of the dest block, so
            // the source block should have no successors. Otherwise, the remainder of
            // the dest block would be unreachable.
            assert!(!src.borrow().has_successors(), "expected 'src' to have no successors");
        }

        // Replace all of the successor arguments with the provided values.
        for (arg, replacement) in src.borrow().arguments().iter().zip(args) {
            self.replace_all_uses_of_value_with(arg.clone().upcast(), replacement.clone());
        }

        // Move operations from the source block to the dest block and erase the source block.
        if self.has_listener() {
            let mut src_ops = src.borrow_mut().body_mut().take();
            let mut src_cursor = src_ops.front_mut();
            while let Some(op) = src_cursor.remove() {
                self.move_op_before(op, ip.clone());
            }
        } else {
            // Fast path: If no listener is attached, move all operations at once.
            let mut dest_block = dest.borrow_mut();
            let dest_body = dest_block.body_mut();
            let mut src_ops = src.borrow_mut().body_mut().take();
            {
                let mut src_cursor = src_ops.front_mut();
                while let Some(op) = src_cursor.as_pointer() {
                    Operation::on_inserted_into_parent(op, dest.clone());
                    src_cursor.move_next();
                }
            }
            let mut cursor = unsafe { dest_body.cursor_mut_from_ptr(ip) };
            cursor.splice_before(src_ops);
        }

        // Erase the source block.
        assert!(src.borrow().body().is_empty(), "expected 'src' to be empty");
        self.erase_block(src);
    }

    /// Inline the operations of block `src` into the end of block `dest`. The source block will be
    /// deleted and must have no uses. The `args` values, if present, are used to replace the block
    /// arguments of `src`.
    ///
    /// The dest block must have no successors. Otherwise, the resulting IR will have unreachable
    /// operations.
    fn merge_blocks(&mut self, src: BlockRef, dest: BlockRef, args: &[ValueRef]) {
        let ip = dest.borrow().body().back().as_pointer().unwrap();
        self.inline_block_before(src, ip, args);
    }

    /// Split the operations starting at `ip` (inclusive) out of the given block into a new block,
    /// and return it.
    fn split_block(&mut self, mut block: BlockRef, ip: OperationRef) -> BlockRef {
        // Fast path: if no listener is attached, split the block directly
        if !self.has_listener() {
            return block.borrow_mut().split_block(ip);
        }

        assert_eq!(
            block,
            ip.borrow().parent().expect("expected 'ip' to be attached to a block"),
            "expected 'ip' to be in 'block'"
        );

        let region = block
            .borrow()
            .parent()
            .expect("cannot split a block which is not attached to a region");

        // `create_block` sets the insertion point to the start of the new block
        let mut guard = InsertionGuard::new(self);
        let new_block = guard.create_block(region, Some(block.clone()), &[]);

        // If `ip` points to the end of the block, no ops should be moved
        if OperationRef::ptr_eq(&ip, &block.borrow().body().back().as_pointer().unwrap()) {
            return new_block;
        }

        // Move ops one-by-one from the end of `block` to the start of `new_block`.
        // Stop when the operation pointed to by `ip` has been moved.
        let mut block = block.borrow_mut();
        let mut cursor = block.body_mut().back_mut();
        while let Some(op) = cursor.remove() {
            let is_last_move = OperationRef::ptr_eq(&op, &ip);
            guard.move_op_before(op, new_block.borrow().body().front().as_pointer().unwrap());
            if is_last_move {
                break;
            }
        }

        new_block
    }

    /// Unlink this block and insert it right before `ip`.
    fn move_block_before(&mut self, mut block: BlockRef, ip: BlockRef) {
        let current_region = block.borrow().parent();
        block.borrow_mut().move_before(ip.clone());
        self.notify_block_inserted(block, current_region, Some(ip));
    }

    /// Unlink this operation from its current block and insert it right before `ip`, which
    /// may be in the same or another block in the same function.
    fn move_op_before(&mut self, mut op: OperationRef, ip: OperationRef) {
        let current_block = op.borrow().parent();
        let current_ip = current_block.map(|block| {
            let blk = block.borrow();
            let cursor = unsafe { blk.body().cursor_from_ptr(op.clone()) };
            if let Some(next_op) = cursor.peek_next().as_pointer() {
                InsertionPoint::before(next_op)
            } else if let Some(prev_op) = cursor.peek_prev().as_pointer() {
                InsertionPoint::after(prev_op)
            } else {
                InsertionPoint::after(block)
            }
        });
        op.borrow_mut().move_before(ProgramPoint::Op(ip.clone()));
        self.notify_operation_inserted(op, current_ip);
    }

    /// Unlink this operation from its current block and insert it right after `ip`, which may be
    /// in the same or another block in the same function.
    fn move_op_after(&mut self, mut op: OperationRef, ip: OperationRef) {
        let current_block = op.borrow().parent();
        let current_ip = current_block.map(|block| {
            let blk = block.borrow();
            let cursor = unsafe { blk.body().cursor_from_ptr(op.clone()) };
            if let Some(next_op) = cursor.peek_next().as_pointer() {
                InsertionPoint::before(next_op)
            } else if let Some(prev_op) = cursor.peek_prev().as_pointer() {
                InsertionPoint::after(prev_op)
            } else {
                InsertionPoint::after(block)
            }
        });
        op.borrow_mut().move_after(ProgramPoint::Op(ip.clone()));
        self.notify_operation_inserted(op, current_ip);
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    fn replace_all_uses_of_value_with(&mut self, mut from: ValueRef, mut to: ValueRef) {
        let mut from_val = from.borrow_mut();
        let from_uses = from_val.uses_mut();
        let mut cursor = from_uses.front_mut();
        while let Some(mut operand) = cursor.remove() {
            let to = &mut to;
            let op = operand.borrow().owner.clone();
            self.notify_operation_modification_started(&op);
            operand.borrow_mut().value = to.clone();
            to.borrow_mut().insert_use(operand);
            self.notify_operation_modified(op);
        }
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    fn replace_all_uses_of_block_with(&mut self, mut from: BlockRef, mut to: BlockRef) {
        let mut from_block = from.borrow_mut();
        let from_uses = from_block.uses_mut();
        let mut cursor = from_uses.front_mut();
        while let Some(mut operand) = cursor.remove() {
            let to = &mut to;
            let op = operand.borrow().owner.clone();
            self.notify_operation_modification_started(&op);
            operand.borrow_mut().block = to.clone();
            to.borrow_mut().insert_use(operand);
            self.notify_operation_modified(op);
        }
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    fn replace_all_uses_with(&mut self, from: &[ValueRef], to: &[ValueRef]) {
        assert_eq!(from.len(), to.len(), "incorrect number of replacements");
        for (from, to) in from.iter().cloned().zip(to.iter().cloned()) {
            self.replace_all_uses_of_value_with(from, to);
        }
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place modification (for every use that was replaced),
    /// and that the `from` operation is about to be replaced.
    fn replace_all_op_uses_with_values(&mut self, from: OperationRef, to: &[ValueRef]) {
        self.notify_operation_replaced_with_values(from.clone(), to);

        let results = from
            .borrow()
            .results()
            .all()
            .iter()
            .map(|result| result.borrow().as_value_ref())
            .collect::<SmallVec<[ValueRef; 2]>>();

        self.replace_all_uses_with(&results, to);
    }

    /// Find uses of `from` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place modification (for every use that was replaced),
    /// and that the `from` operation is about to be replaced.
    fn replace_all_op_uses_with(&mut self, from: OperationRef, to: OperationRef) {
        self.notify_operation_replaced(from.clone(), to.clone());

        let from_results = from
            .borrow()
            .results()
            .all()
            .iter()
            .map(|result| result.borrow().as_value_ref())
            .collect::<SmallVec<[ValueRef; 2]>>();

        let to_results = to
            .borrow()
            .results()
            .all()
            .iter()
            .map(|result| result.borrow().as_value_ref())
            .collect::<SmallVec<[ValueRef; 2]>>();

        self.replace_all_uses_with(&from_results, &to_results);
    }

    /// Find uses of `from` within `block` and replace them with `to`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    fn replace_op_uses_within_block(
        &mut self,
        from: OperationRef,
        to: &[ValueRef],
        block: BlockRef,
    ) -> bool {
        let parent_op = block.borrow().parent_op();
        self.maybe_replace_op_uses_with(from, to, |operand| {
            !parent_op
                .as_ref()
                .is_some_and(|op| op.borrow().is_proper_ancestor_of(&operand.owner))
        })
    }

    /// Find uses of `from` and replace them with `to`, except if the user is in `exceptions`.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    fn replace_all_uses_except(
        &mut self,
        from: ValueRef,
        to: ValueRef,
        exceptions: &[OperationRef],
    ) {
        self.maybe_replace_uses_of_value_with(from, to, |operand| {
            !exceptions.contains(&operand.owner)
        });
    }
}

/// An extension trait for [Rewriter] implementations.
///
/// This trait contains functionality that is not object safe, and would prevent using [Rewriter] as
/// a trait object. It is automatically implemented for all [Rewriter] impls.
pub trait RewriterExt: Rewriter {
    /// Find uses of `from` and replace them with `to`, if `should_replace` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    fn maybe_replace_uses_of_value_with<P>(
        &mut self,
        mut from: ValueRef,
        mut to: ValueRef,
        should_replace: P,
    ) -> bool
    where
        P: Fn(&OpOperandImpl) -> bool,
    {
        let mut all_replaced = true;
        let mut from = from.borrow_mut();
        let from_uses = from.uses_mut();
        let mut cursor = from_uses.front_mut();
        while let Some(user) = cursor.as_pointer() {
            if should_replace(&user.borrow()) {
                let owner = user.borrow().owner.clone();
                self.notify_operation_modification_started(&owner);
                let mut operand = cursor.remove().unwrap();
                {
                    operand.borrow_mut().value = to.clone();
                }
                to.borrow_mut().insert_use(operand);
                self.notify_operation_modified(owner);
            } else {
                all_replaced = false;
                cursor.move_next();
            }
        }
        all_replaced
    }

    /// Find uses of `from` and replace them with `to`, if `should_replace` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    fn maybe_replace_uses_with<P>(
        &mut self,
        from: &[ValueRef],
        to: &[ValueRef],
        should_replace: P,
    ) -> bool
    where
        P: Fn(&OpOperandImpl) -> bool,
    {
        assert_eq!(from.len(), to.len(), "incorrect number of replacements");
        let mut all_replaced = true;
        for (from, to) in from.iter().cloned().zip(to.iter().cloned()) {
            all_replaced &= self.maybe_replace_uses_of_value_with(from, to, &should_replace);
        }
        all_replaced
    }

    /// Find uses of `from` and replace them with `to`, if `should_replace` returns true.
    ///
    /// Notifies the listener about every in-place op modification (for every use that was replaced).
    ///
    /// Returns true if all uses were replaced, otherwise false.
    fn maybe_replace_op_uses_with<P>(
        &mut self,
        from: OperationRef,
        to: &[ValueRef],
        should_replace: P,
    ) -> bool
    where
        P: Fn(&OpOperandImpl) -> bool,
    {
        let results = SmallVec::<[ValueRef; 2]>::from_iter(
            from.borrow().results.all().iter().cloned().map(|result| result.upcast()),
        );
        self.maybe_replace_uses_with(&results, to, should_replace)
    }
}

impl<R: ?Sized + Rewriter> RewriterExt for R {}

#[allow(unused_variables)]
pub trait RewriterListener: Listener {
    /// Notify the listener that the specified block is about to be erased.
    ///
    /// At this point, the block has zero uses.
    fn notify_block_erased(&self, block: BlockRef) {}

    /// Notify the listener that an in-place modification of the specified operation has started
    fn notify_operation_modification_started(&self, op: &OperationRef) {}

    /// Notify the listener that an in-place modification of the specified operation was canceled
    fn notify_operation_modification_canceled(&self, op: &OperationRef) {}

    /// Notify the listener that the specified operation was modified in-place.
    fn notify_operation_modified(&self, op: OperationRef) {}

    /// Notify the listener that all uses of the specified operation's results are about to be
    /// replaced with the results of another operation. This is called before the uses of the old
    /// operation have been changed.
    ///
    /// By default, this function calls the "operation replaced with values" notification.
    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        let replacement = replacement.borrow();
        let values = replacement
            .results()
            .all()
            .iter()
            .cloned()
            .map(|result| result.upcast())
            .collect::<SmallVec<[ValueRef; 2]>>();
        self.notify_operation_replaced_with_values(op, &values);
    }

    /// Notify the listener that all uses of the specified operation's results are about to be
    /// replaced with the given range of values, potentially produced by other operations. This is
    /// called before the uses of the operation have been changed.
    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {}

    /// Notify the listener that the specified operation is about to be erased. At this point, the
    /// operation has zero uses.
    ///
    /// NOTE: This notification is not triggered when unlinking an operation.
    fn notify_operation_erased(&self, op: OperationRef) {}

    /// Notify the listener that the specified pattern is about to be applied at the specified root
    /// operation.
    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {}

    /// Notify the listener that a pattern application finished with the specified status.
    ///
    /// `true` indicates that the pattern was applied successfully. `false` indicates that the
    /// pattern could not be applied. The pattern may have communicated the reason for the failure
    /// with `notify_match_failure`
    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {}

    /// Notify the listener that the pattern failed to match, and provide a diagnostic explaining
    /// the reason why the failure occurred.
    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {}
}

impl<L: RewriterListener> RewriterListener for Option<L> {
    fn notify_block_erased(&self, block: BlockRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_block_erased(block);
        }
    }

    fn notify_operation_modification_started(&self, op: &OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_modification_started(op);
        }
    }

    fn notify_operation_modification_canceled(&self, op: &OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_modification_canceled(op);
        }
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_modified(op);
        }
    }

    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_replaced(op, replacement);
        }
    }

    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_replaced_with_values(op, replacement);
        }
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_operation_erased(op);
        }
    }

    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {
        if let Some(listener) = self.as_ref() {
            listener.notify_pattern_begin(pattern, op);
        }
    }

    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {
        if let Some(listener) = self.as_ref() {
            listener.notify_pattern_end(pattern, success);
        }
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        if let Some(listener) = self.as_ref() {
            listener.notify_match_failure(span, reason);
        }
    }
}

impl<L: ?Sized + RewriterListener> RewriterListener for Box<L> {
    fn notify_block_erased(&self, block: BlockRef) {
        (**self).notify_block_erased(block);
    }

    fn notify_operation_modification_started(&self, op: &OperationRef) {
        (**self).notify_operation_modification_started(op);
    }

    fn notify_operation_modification_canceled(&self, op: &OperationRef) {
        (**self).notify_operation_modification_canceled(op);
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        (**self).notify_operation_modified(op);
    }

    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        (**self).notify_operation_replaced(op, replacement);
    }

    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        (**self).notify_operation_replaced_with_values(op, replacement);
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        (**self).notify_operation_erased(op)
    }

    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {
        (**self).notify_pattern_begin(pattern, op);
    }

    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {
        (**self).notify_pattern_end(pattern, success);
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        (**self).notify_match_failure(span, reason);
    }
}

impl<L: ?Sized + RewriterListener> RewriterListener for Rc<L> {
    fn notify_block_erased(&self, block: BlockRef) {
        (**self).notify_block_erased(block);
    }

    fn notify_operation_modification_started(&self, op: &OperationRef) {
        (**self).notify_operation_modification_started(op);
    }

    fn notify_operation_modification_canceled(&self, op: &OperationRef) {
        (**self).notify_operation_modification_canceled(op);
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        (**self).notify_operation_modified(op);
    }

    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        (**self).notify_operation_replaced(op, replacement);
    }

    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        (**self).notify_operation_replaced_with_values(op, replacement);
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        (**self).notify_operation_erased(op)
    }

    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {
        (**self).notify_pattern_begin(pattern, op);
    }

    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {
        (**self).notify_pattern_end(pattern, success);
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        (**self).notify_match_failure(span, reason);
    }
}

/// A listener of kind `Rewriter` that does nothing
pub struct NoopRewriterListener;
impl Listener for NoopRewriterListener {
    #[inline]
    fn kind(&self) -> ListenerType {
        ListenerType::Rewriter
    }

    #[inline(always)]
    fn notify_operation_inserted(&self, _op: OperationRef, _prev: Option<InsertionPoint>) {}

    #[inline(always)]
    fn notify_block_inserted(
        &self,
        _block: BlockRef,
        _prev: Option<RegionRef>,
        _ip: Option<BlockRef>,
    ) {
    }
}
impl RewriterListener for NoopRewriterListener {
    fn notify_operation_replaced(&self, _op: OperationRef, _replacement: OperationRef) {}
}

pub struct ForwardingListener<Base, Derived> {
    base: Base,
    derived: Derived,
}
impl<Base, Derived> ForwardingListener<Base, Derived> {
    pub fn new(base: Base, derived: Derived) -> Self {
        Self { base, derived }
    }
}
impl<Base: Listener, Derived: Listener> Listener for ForwardingListener<Base, Derived> {
    fn kind(&self) -> ListenerType {
        self.derived.kind()
    }

    fn notify_block_inserted(
        &self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<BlockRef>,
    ) {
        self.base.notify_block_inserted(block.clone(), prev.clone(), ip.clone());
        self.derived.notify_block_inserted(block, prev, ip);
    }

    fn notify_operation_inserted(&self, op: OperationRef, prev: Option<InsertionPoint>) {
        self.base.notify_operation_inserted(op.clone(), prev.clone());
        self.derived.notify_operation_inserted(op, prev);
    }
}
impl<Base: RewriterListener, Derived: RewriterListener> RewriterListener
    for ForwardingListener<Base, Derived>
{
    fn notify_block_erased(&self, block: BlockRef) {
        self.base.notify_block_erased(block.clone());
        self.derived.notify_block_erased(block);
    }

    fn notify_operation_modification_started(&self, op: &OperationRef) {
        self.base.notify_operation_modification_started(op);
        self.derived.notify_operation_modification_started(op);
    }

    fn notify_operation_modification_canceled(&self, op: &OperationRef) {
        self.base.notify_operation_modification_canceled(op);
        self.derived.notify_operation_modification_canceled(op);
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        self.base.notify_operation_modified(op.clone());
        self.derived.notify_operation_modified(op);
    }

    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        self.base.notify_operation_replaced(op.clone(), replacement.clone());
        self.derived.notify_operation_replaced(op, replacement);
    }

    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        self.base.notify_operation_replaced_with_values(op.clone(), replacement);
        self.derived.notify_operation_replaced_with_values(op, replacement);
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        self.base.notify_operation_erased(op.clone());
        self.derived.notify_operation_erased(op);
    }

    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {
        self.base.notify_pattern_begin(pattern, op.clone());
        self.derived.notify_pattern_begin(pattern, op);
    }

    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {
        self.base.notify_pattern_end(pattern, success);
        self.derived.notify_pattern_end(pattern, success);
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        let err = Report::msg(format!("{reason}"));
        self.base.notify_match_failure(span, reason);
        self.derived.notify_match_failure(span, err);
    }
}

/// Wraps an in-place modification of an [Operation] to ensure the rewriter is properly notified
/// about the progress and outcome of the in-place notification.
///
/// This is a minor efficiency win, as it avoids creating a new operation, and removing the old one,
/// but also often allows simpler code in the client.
pub struct InPlaceModificationGuard<'a, R: ?Sized + Rewriter> {
    rewriter: &'a mut R,
    op: OperationRef,
    canceled: bool,
}
impl<'a, R> InPlaceModificationGuard<'a, R>
where
    R: ?Sized + Rewriter,
{
    pub fn new(rewriter: &'a mut R, op: OperationRef) -> Self {
        rewriter.notify_operation_modification_started(&op);
        Self {
            rewriter,
            op,
            canceled: false,
        }
    }

    #[inline]
    pub fn rewriter(&mut self) -> &mut R {
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
impl<R: ?Sized + Rewriter> core::ops::Deref for InPlaceModificationGuard<'_, R> {
    type Target = R;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.rewriter
    }
}
impl<R: ?Sized + Rewriter> core::ops::DerefMut for InPlaceModificationGuard<'_, R> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.rewriter
    }
}
impl<R: ?Sized + Rewriter> Drop for InPlaceModificationGuard<'_, R> {
    fn drop(&mut self) {
        if self.canceled {
            self.rewriter.notify_operation_modification_canceled(&self.op);
        } else {
            self.rewriter.notify_operation_modified(self.op.clone());
        }
    }
}

/// A special type of `RewriterBase` that coordinates the application of a rewrite pattern on the
/// current IR being matched, providing a way to keep track of any mutations made.
///
/// This type should be used to perform all necessary IR mutations within a rewrite pattern, as
/// the pattern driver may be tracking various state that would be invalidated when a mutation takes
/// place.
pub struct PatternRewriter<L = NoopRewriterListener> {
    rewriter: RewriterImpl<L>,
    recoverable: bool,
}

impl PatternRewriter {
    pub fn new(context: Rc<Context>) -> Self {
        let rewriter = RewriterImpl::new(context);
        Self {
            rewriter,
            recoverable: false,
        }
    }

    pub fn from_builder(builder: OpBuilder) -> Self {
        let (context, _, ip) = builder.into_parts();
        let mut rewriter = RewriterImpl::new(context);
        rewriter.restore_insertion_point(ip);
        Self {
            rewriter,
            recoverable: false,
        }
    }
}

impl<L: RewriterListener> PatternRewriter<L> {
    pub fn new_with_listener(context: Rc<Context>, listener: L) -> Self {
        let rewriter = RewriterImpl::<NoopRewriterListener>::new(context).with_listener(listener);
        Self {
            rewriter,
            recoverable: false,
        }
    }

    #[inline]
    pub const fn can_recover_from_rewrite_failure(&self) -> bool {
        self.recoverable
    }
}
impl<L> Deref for PatternRewriter<L> {
    type Target = RewriterImpl<L>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.rewriter
    }
}
impl<L> DerefMut for PatternRewriter<L> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rewriter
    }
}

pub struct RewriterImpl<L = NoopRewriterListener> {
    context: Rc<Context>,
    listener: Option<L>,
    ip: Option<InsertionPoint>,
}

impl<L> RewriterImpl<L> {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            listener: None,
            ip: None,
        }
    }

    pub fn with_listener<L2>(self, listener: L2) -> RewriterImpl<L2>
    where
        L2: Listener,
    {
        RewriterImpl {
            context: self.context,
            listener: Some(listener),
            ip: self.ip,
        }
    }
}

impl<L: RewriterListener> From<OpBuilder<L>> for RewriterImpl<L> {
    #[inline]
    fn from(builder: OpBuilder<L>) -> Self {
        let (context, listener, ip) = builder.into_parts();
        Self {
            context,
            listener,
            ip,
        }
    }
}

impl<L: Listener> Builder for RewriterImpl<L> {
    #[inline(always)]
    fn context(&self) -> &Context {
        &self.context
    }

    #[inline(always)]
    fn context_rc(&self) -> Rc<Context> {
        self.context.clone()
    }

    #[inline(always)]
    fn insertion_point(&self) -> Option<&InsertionPoint> {
        self.ip.as_ref()
    }

    #[inline(always)]
    fn clear_insertion_point(&mut self) -> Option<InsertionPoint> {
        self.ip.take()
    }

    #[inline(always)]
    fn restore_insertion_point(&mut self, ip: Option<InsertionPoint>) {
        self.ip = ip;
    }

    #[inline(always)]
    fn set_insertion_point(&mut self, ip: InsertionPoint) {
        self.ip = Some(ip);
    }
}

impl<L: RewriterListener> Rewriter for RewriterImpl<L> {
    #[inline(always)]
    fn has_listener(&self) -> bool {
        self.listener.is_some()
    }
}

impl<L: Listener> Listener for RewriterImpl<L> {
    fn kind(&self) -> ListenerType {
        ListenerType::Rewriter
    }

    fn notify_operation_inserted(&self, op: OperationRef, prev: Option<InsertionPoint>) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_inserted(op, prev);
        }
    }

    fn notify_block_inserted(
        &self,
        block: BlockRef,
        prev: Option<RegionRef>,
        ip: Option<BlockRef>,
    ) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_block_inserted(block, prev, ip);
        }
    }
}

impl<L: RewriterListener> RewriterListener for RewriterImpl<L> {
    fn notify_block_erased(&self, block: BlockRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_block_erased(block);
        }
    }

    fn notify_operation_modification_started(&self, op: &OperationRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_modification_started(op);
        }
    }

    fn notify_operation_modification_canceled(&self, op: &OperationRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_modification_canceled(op);
        }
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_modified(op);
        }
    }

    fn notify_operation_replaced(&self, op: OperationRef, replacement: OperationRef) {
        if self.listener.is_some() {
            let replacement = replacement.borrow();
            let values = replacement
                .results()
                .all()
                .iter()
                .cloned()
                .map(|result| result.upcast())
                .collect::<SmallVec<[ValueRef; 2]>>();
            self.notify_operation_replaced_with_values(op, &values);
        }
    }

    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_replaced_with_values(op, replacement);
        }
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_operation_erased(op);
        }
    }

    fn notify_pattern_begin(&self, pattern: &dyn Pattern, op: OperationRef) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_pattern_begin(pattern, op);
        }
    }

    fn notify_pattern_end(&self, pattern: &dyn Pattern, success: bool) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_pattern_end(pattern, success);
        }
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        if let Some(listener) = self.listener.as_ref() {
            listener.notify_match_failure(span, reason);
        }
    }
}
