use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;

use smallvec::SmallVec;

use crate::{
    BlockRef, BuildableOp, Builder, BuilderExt, Context, Listener, ListenerType, OperationRef,
    ProgramPoint, RegionRef, Report, SourceSpan, UnsafeIntrusiveEntityRef, ValueRef,
    patterns::{PatternRewriter, Rewriter, RewriterListener},
};

/// A recorded reason that a conversion pattern did not match.
pub struct MatchFailure {
    op: OperationRef,
    reason: Report,
}

impl MatchFailure {
    #[inline]
    pub const fn op(&self) -> OperationRef {
        self.op
    }

    #[inline]
    pub const fn reason(&self) -> &Report {
        &self.reason
    }
}

/// Mutations observed while applying a conversion pattern.
#[derive(Default)]
pub struct TrackedMutations {
    inserted_ops: Vec<OperationRef>,
    modified_ops: Vec<OperationRef>,
    replaced_ops: Vec<OperationRef>,
    erased_ops: Vec<OperationRef>,
    block_mutations: usize,
    mutation_count: usize,
}

impl TrackedMutations {
    #[inline]
    pub fn inserted_ops(&self) -> &[OperationRef] {
        &self.inserted_ops
    }

    #[inline]
    pub fn modified_ops(&self) -> &[OperationRef] {
        &self.modified_ops
    }

    #[inline]
    pub fn replaced_ops(&self) -> &[OperationRef] {
        &self.replaced_ops
    }

    #[inline]
    pub fn erased_ops(&self) -> &[OperationRef] {
        &self.erased_ops
    }

    #[inline]
    pub const fn block_mutations(&self) -> usize {
        self.block_mutations
    }

    #[inline]
    pub const fn mutation_count(&self) -> usize {
        self.mutation_count
    }

    #[inline]
    pub const fn has_mutations(&self) -> bool {
        self.mutation_count > 0
    }
}

struct ConversionTrackingListener {
    mutations: RefCell<TrackedMutations>,
}

impl ConversionTrackingListener {
    fn new() -> Self {
        Self {
            mutations: RefCell::new(TrackedMutations::default()),
        }
    }

    fn mutation_count(&self) -> usize {
        self.mutations.borrow().mutation_count()
    }

    fn take_mutations(&self) -> TrackedMutations {
        core::mem::take(&mut *self.mutations.borrow_mut())
    }

    fn record_inserted_op(&self, op: OperationRef) {
        let mut mutations = self.mutations.borrow_mut();
        push_unique(&mut mutations.inserted_ops, op);
        mutations.mutation_count += 1;
    }

    fn record_modified_op(&self, op: OperationRef) {
        let mut mutations = self.mutations.borrow_mut();
        push_unique(&mut mutations.modified_ops, op);
        mutations.mutation_count += 1;
    }

    fn record_replaced_op(&self, op: OperationRef) {
        let mut mutations = self.mutations.borrow_mut();
        push_unique(&mut mutations.replaced_ops, op);
        mutations.mutation_count += 1;
    }

    fn record_erased_op(&self, op: OperationRef) {
        let mut mutations = self.mutations.borrow_mut();
        push_unique(&mut mutations.erased_ops, op);
        mutations.mutation_count += 1;
    }

    fn record_block_mutation(&self) {
        let mut mutations = self.mutations.borrow_mut();
        mutations.block_mutations += 1;
        mutations.mutation_count += 1;
    }
}

impl Listener for ConversionTrackingListener {
    fn kind(&self) -> ListenerType {
        ListenerType::Rewriter
    }

    fn notify_operation_inserted(&self, op: OperationRef, _prev: ProgramPoint) {
        self.record_inserted_op(op);
    }

    fn notify_block_inserted(
        &self,
        _block: BlockRef,
        _prev: Option<RegionRef>,
        _ip: Option<BlockRef>,
    ) {
        self.record_block_mutation();
    }
}

impl RewriterListener for ConversionTrackingListener {
    fn notify_block_erased(&self, _block: BlockRef) {
        self.record_block_mutation();
    }

    fn notify_operation_modified(&self, op: OperationRef) {
        self.record_modified_op(op);
    }

    fn notify_operation_replaced(&self, op: OperationRef, _replacement: OperationRef) {
        self.record_replaced_op(op);
    }

    fn notify_operation_replaced_with_values(
        &self,
        op: OperationRef,
        _replacement: &[Option<ValueRef>],
    ) {
        self.record_replaced_op(op);
    }

    fn notify_operation_erased(&self, op: OperationRef) {
        self.record_erased_op(op);
    }
}

fn push_unique(ops: &mut Vec<OperationRef>, op: OperationRef) {
    if !ops.contains(&op) {
        ops.push(op);
    }
}

/// Rewriter used by conversion patterns.
pub struct ConversionPatternRewriter {
    inner: PatternRewriter<Rc<ConversionTrackingListener>>,
    tracking: Rc<ConversionTrackingListener>,
    match_failures: Vec<MatchFailure>,
}

impl ConversionPatternRewriter {
    #[inline]
    pub fn new(context: Rc<Context>) -> Self {
        let tracking = Rc::new(ConversionTrackingListener::new());
        let inner = PatternRewriter::new_with_listener(context, Rc::clone(&tracking));
        Self {
            inner,
            tracking,
            match_failures: Vec::new(),
        }
    }

    /// Set the insertion point before `op`.
    #[inline]
    pub fn set_insertion_point_before(&mut self, op: OperationRef) {
        self.inner.set_insertion_point_before(op);
    }

    /// Set the insertion point after `op`.
    #[inline]
    pub fn set_insertion_point_after(&mut self, op: OperationRef) {
        self.inner.set_insertion_point_after(op);
    }

    /// Create a new operation at the current insertion point.
    pub fn create_op<T, Args>(
        &mut self,
        span: SourceSpan,
        args: Args,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, Report>
    where
        Args: core::marker::Tuple,
        T: BuildableOp<Args>,
    {
        let builder = self.inner.create::<T, Args>(span);
        core::ops::FnOnce::call_once(builder, args)
    }

    /// Replace `op` with the provided same-type replacement values.
    pub fn replace_op(
        &mut self,
        op: OperationRef,
        replacement_values: &[ValueRef],
    ) -> Result<(), Report> {
        if op.borrow().num_results() != replacement_values.len() {
            return Err(Report::msg("replacement value count does not match operation results"));
        }

        let values = replacement_values
            .iter()
            .copied()
            .map(Some)
            .collect::<SmallVec<[Option<ValueRef>; 2]>>();
        self.inner.replace_op_with_values(op, &values);
        Ok(())
    }

    /// Create a new operation before `op`, then replace `op` with the new operation's results.
    pub fn replace_op_with_new_op<T, Args>(
        &mut self,
        op: OperationRef,
        span: SourceSpan,
        args: Args,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, Report>
    where
        Args: core::marker::Tuple,
        T: BuildableOp<Args>,
    {
        self.set_insertion_point_before(op);
        let replacement = self.create_op::<T, Args>(span, args)?;
        self.inner.replace_op(op, replacement.as_operation_ref());
        Ok(replacement)
    }

    /// Erase an operation with no remaining uses.
    #[inline]
    pub fn erase_op(&mut self, op: OperationRef) -> Result<(), Report> {
        self.inner.erase_op(op);
        Ok(())
    }

    pub fn notify_match_failure(&mut self, op: OperationRef, reason: Report) {
        self.match_failures.push(MatchFailure { op, reason });
    }

    #[inline]
    pub fn match_failures(&self) -> &[MatchFailure] {
        &self.match_failures
    }

    #[inline]
    pub fn clear_match_failures(&mut self) {
        self.match_failures.clear();
    }

    pub fn take_match_failures(&mut self) -> Vec<MatchFailure> {
        core::mem::take(&mut self.match_failures)
    }

    #[inline]
    pub fn mutation_count(&self) -> usize {
        self.tracking.mutation_count()
    }

    pub fn take_tracked_mutations(&mut self) -> TrackedMutations {
        self.tracking.take_mutations()
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use crate::{
        Context, OpRegistration, Report, conversion::ConversionPatternRewriter,
        dialects::test::Constant,
    };

    #[test]
    fn records_match_failures() {
        let context = Rc::new(Context::default());
        let op = context
            .get_or_register_dialect::<<Constant as OpRegistration>::Dialect>()
            .expect_registered_name::<Constant>()
            .alloc_default(context.clone());
        let mut rewriter = ConversionPatternRewriter::new(context);

        rewriter.notify_match_failure(op, Report::msg("no match"));

        assert_eq!(rewriter.match_failures().len(), 1);
        rewriter.clear_match_failures();
        assert!(rewriter.match_failures().is_empty());
    }
}
