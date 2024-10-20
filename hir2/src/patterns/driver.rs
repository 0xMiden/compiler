use alloc::{collections::BTreeSet, rc::Rc};
use core::cell::RefCell;

use smallvec::SmallVec;

use super::{
    ForwardingListener, FrozenRewritePatternSet, PatternApplicator, PatternRewriter, Rewriter,
    RewriterListener,
};
use crate::{
    traits::{ConstantLike, Foldable, IsolatedFromAbove},
    BlockRef, Builder, Context, InsertionGuard, Listener, OpFoldResult, OperationFolder,
    OperationRef, ProgramPoint, Region, RegionRef, Report, RewritePattern, SourceSpan, Spanned,
    Value, ValueRef, WalkResult, Walkable,
};

/// Rewrite ops in the given region, which must be isolated from above, by repeatedly applying the
/// highest benefit patterns in a greedy worklist driven manner until a fixpoint is reached.
///
/// The greedy rewrite may prematurely stop after a maximum number of iterations, which can be
/// configured using [GreedyRewriteConfig].
///
/// This function also performs folding and simple dead-code elimination before attempting to match
/// any of the provided patterns.
///
/// A region scope can be set using [GreedyRewriteConfig]. By default, the scope is set to the
/// specified region. Only in-scope ops are added to the worklist and only in-scope ops are allowed
/// to be modified by the patterns.
///
/// Returns `Ok(changed)` if the iterative process converged (i.e., fixpoint was reached) and no
/// more patterns can be matched within the region. The `changed` flag is set to `true` if the IR
/// was modified at all.
///
/// NOTE: This function does not apply patterns to the region's parent operation.
pub fn apply_patterns_and_fold_region_greedily(
    region: RegionRef,
    patterns: Rc<FrozenRewritePatternSet>,
    mut config: GreedyRewriteConfig,
) -> Result<bool, bool> {
    // The top-level operation must be known to be isolated from above to prevent performing
    // canonicalizations on operations defined at or above the region containing 'op'.
    let context = {
        let parent_op = region.borrow().parent().unwrap().borrow();
        assert!(
            parent_op.implements::<dyn IsolatedFromAbove>(),
            "patterns can only be applied to operations which are isolated from above"
        );
        parent_op.context_rc()
    };

    // Set scope if not specified
    if config.scope.is_none() {
        config.scope = Some(region.clone());
    }

    let mut driver = RegionPatternRewriteDriver::new(context, patterns, config, region);
    let converged = driver.simplify();
    if converged.is_err() {
        if let Some(max_iterations) = driver.driver.config.max_iterations {
            log::trace!("pattern rewrite did not converge after scanning {max_iterations} times");
        } else {
            log::trace!("pattern rewrite did not converge");
        }
    }
    converged
}

/// Rewrite ops nested under the given operation, which must be isolated from above, by repeatedly
/// applying the highest benefit patterns in a greedy worklist driven manner until a fixpoint is
/// reached.
///
/// The greedy rewrite may prematurely stop after a maximum number of iterations, which can be
/// configured using [GreedyRewriteConfig].
///
/// Also performs folding and simple dead-code elimination before attempting to match any of the
/// provided patterns.
///
/// This overload runs a separate greedy rewrite for each region of the specified op. A region
/// scope can be set in the configuration parameter. By default, the scope is set to the region of
/// the current greedy rewrite. Only in-scope ops are added to the worklist and only in-scope ops
/// and the specified op itself are allowed to be modified by the patterns.
///
/// NOTE: The specified op may be modified, but it may not be removed by the patterns.
///
/// Returns `Ok(changed)` if the iterative process converged (i.e., fixpoint was reached) and no
/// more patterns can be matched within the region. The `changed` flag is set to `true` if the IR
/// was modified at all.
///
/// NOTE: This function does not apply patterns to the given operation itself.
pub fn apply_patterns_and_fold_greedily(
    op: OperationRef,
    patterns: Rc<FrozenRewritePatternSet>,
    config: GreedyRewriteConfig,
) -> Result<bool, bool> {
    let mut any_region_changed = false;
    let mut failed = false;
    let op = op.borrow();
    let mut cursor = op.regions().front();
    while let Some(region) = cursor.as_pointer() {
        cursor.move_next();
        match apply_patterns_and_fold_region_greedily(region, patterns.clone(), config.clone()) {
            Ok(region_changed) => {
                any_region_changed |= region_changed;
            }
            Err(region_changed) => {
                any_region_changed |= region_changed;
                failed = true;
            }
        }
    }

    if failed {
        Err(any_region_changed)
    } else {
        Ok(any_region_changed)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum ApplyPatternsAndFoldEffect {
    /// No effect, the IR remains unchanged
    None,
    /// The IR was modified
    Changed,
    /// The input IR was erased
    Erased,
}

pub type ApplyPatternsAndFoldResult =
    Result<ApplyPatternsAndFoldEffect, ApplyPatternsAndFoldEffect>;

/// Rewrite the specified ops by repeatedly applying the highest benefit patterns in a greedy
/// worklist driven manner until a fixpoint is reached.
///
/// The greedy rewrite may prematurely stop after a maximum number of iterations, which can be
/// configured using [GreedyRewriteConfig].
///
/// This function also performs folding and simple dead-code elimination before attempting to match
/// any of the provided patterns.
///
/// Newly created ops and other pre-existing ops that use results of rewritten ops or supply
/// operands to such ops are also processed, unless such ops are excluded via `config.restrict`.
/// Any other ops remain unmodified (i.e., regardless of restrictions).
///
/// In addition to op restrictions, a region scope can be specified. Only ops within the scope are
/// simplified. This is similar to [apply_patterns_and_fold_greedily], where only ops within the
/// given region/op are simplified by default. If no scope is specified, it is assumed to be the
/// first common enclosing region of the given ops.
///
/// Note that ops in `ops` could be erased as result of folding, becoming dead, or via pattern
/// rewrites. If more far reaching simplification is desired, [apply_patterns_and_fold_greedily]
/// should be used.
///
/// Returns `Ok(effect)` if the iterative process converged (i.e., fixpoint was reached) and no more
/// patterns can be matched. `effect` is set to `Changed` if the IR was modified, but at least one
/// operation was not erased. It is set to `Erased` if all of the input ops were erased.
pub fn apply_patterns_and_fold(
    ops: &[OperationRef],
    patterns: Rc<FrozenRewritePatternSet>,
    mut config: GreedyRewriteConfig,
) -> ApplyPatternsAndFoldResult {
    if ops.is_empty() {
        return Ok(ApplyPatternsAndFoldEffect::None);
    }

    // Determine scope of rewrite
    if let Some(scope) = config.scope.as_ref() {
        // If a scope was provided, make sure that all ops are in scope.
        let all_ops_in_scope = ops.iter().all(|op| scope.borrow().find_ancestor_op(op).is_some());
        assert!(all_ops_in_scope, "ops must be within the specified scope");
    } else {
        // Compute scope if none was provided. The scope will remain `None` if there is a top-level
        // op among `ops`.
        config.scope = Region::find_common_ancestor(ops);
    }

    // Start the pattern driver
    let max_rewrites = config.max_rewrites.map(|max| max.get()).unwrap_or(u32::MAX);
    let context = ops[0].borrow().context_rc();
    let mut driver = MultiOpPatternRewriteDriver::new(context, patterns, config, ops);
    let converged = driver.simplify(ops);
    let changed = match converged.as_ref() {
        Ok(changed) | Err(changed) => *changed,
    };
    let erased = driver.inner.surviving_ops.borrow().is_empty();
    let effect = if erased {
        ApplyPatternsAndFoldEffect::Erased
    } else if changed {
        ApplyPatternsAndFoldEffect::Changed
    } else {
        ApplyPatternsAndFoldEffect::None
    };
    if converged.is_ok() {
        Ok(effect)
    } else {
        log::trace!("pattern rewrite did not converge after {max_rewrites} rewrites");
        Err(effect)
    }
}

/// This enum indicates which ops are put on the worklist during a greedy pattern rewrite
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum GreedyRewriteStrictness {
    /// No restrictions on which ops are processed.
    #[default]
    Any,
    /// Only pre-existing and newly created ops are processed.
    ///
    /// Pre-existing ops are those that were on the worklist at the very beginning.
    ExistingAndNew,
    /// Only pre-existing ops are processed.
    ///
    /// Pre-existing ops are those that were on the worklist at the very beginning.
    Existing,
}

/// This enum indicates the level of simplification to be applied to regions during a greedy
/// pattern rewrite.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum RegionSimplificationLevel {
    /// Disable simplification.
    None,
    /// Perform basic simplifications (e.g. dead argument elimination)
    #[default]
    Normal,
    /// Perform additional complex/expensive simplifications (e.g. block merging)
    Aggressive,
}

/// Configuration for [GreedyPatternRewriteDriver]
#[derive(Clone)]
pub struct GreedyRewriteConfig {
    listener: Option<Rc<dyn RewriterListener>>,
    /// If set, only ops within the given region are added to the worklist.
    ///
    /// If no scope is specified, and no specific region is given when starting the greedy rewrite,
    /// then the closest enclosing region of the initial list of operations is used.
    scope: Option<RegionRef>,
    /// If set, specifies the maximum number of times the rewriter will iterate between applying
    /// patterns and simplifying regions.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    max_iterations: Option<core::num::NonZeroU32>,
    /// If set, specifies the maximum number of rewrites within an iteration.
    max_rewrites: Option<core::num::NonZeroU32>,
    /// Perform control flow optimizations to the region tree after applying all patterns.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    region_simplification: RegionSimplificationLevel,
    /// The restrictions to apply, if any, to operations added to the worklist during the rewrite.
    restrict: GreedyRewriteStrictness,
    /// This flag specifies the order of initial traversal that populates the rewriter worklist.
    ///
    /// When true, operations are visited top-down, which is generally more efficient in terms of
    /// compilation time.
    ///
    /// When false, the initial traversal of the region tree is bottom up on each block, which may
    /// match larger patterns when given an ambiguous pattern set.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    use_top_down_traversal: bool,
}
impl Default for GreedyRewriteConfig {
    fn default() -> Self {
        Self {
            listener: None,
            scope: None,
            max_iterations: core::num::NonZeroU32::new(10),
            max_rewrites: None,
            region_simplification: Default::default(),
            restrict: Default::default(),
            use_top_down_traversal: false,
        }
    }
}
impl GreedyRewriteConfig {
    pub fn new_with_listener(listener: impl RewriterListener) -> Self {
        Self {
            listener: Some(Rc::new(listener)),
            ..Default::default()
        }
    }

    /// Scope rewrites to operations within `region`
    pub fn with_scope(&mut self, region: RegionRef) -> &mut Self {
        self.scope = Some(region);
        self
    }

    /// Set the maximum number of times the rewriter will iterate between applying patterns and
    /// simplifying regions.
    ///
    /// If `0` is given, the number of iterations is unlimited.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    pub fn with_max_iterations(&mut self, max: u32) -> &mut Self {
        self.max_iterations = core::num::NonZeroU32::new(max);
        self
    }

    /// Set the maximum number of rewrites per iteration.
    ///
    /// If `0` is given, the number of rewrites is unlimited.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    pub fn with_max_rewrites(&mut self, max: u32) -> &mut Self {
        self.max_rewrites = core::num::NonZeroU32::new(max);
        self
    }

    /// Set the level of control flow optimizations to apply to the region tree.
    ///
    /// NOTE: Only applicable when simplifying entire regions.
    pub fn with_region_simplification_level(
        &mut self,
        level: RegionSimplificationLevel,
    ) -> &mut Self {
        self.region_simplification = level;
        self
    }

    /// Set the level of restriction to apply to operations added to the worklist during the rewrite.
    pub fn with_restrictions(&mut self, level: GreedyRewriteStrictness) -> &mut Self {
        self.restrict = level;
        self
    }

    /// Specify whether or not to use a top-down traversal when initially adding operations to the
    /// worklist.
    pub fn with_top_down_traversal(&mut self, yes: bool) -> &mut Self {
        self.use_top_down_traversal = yes;
        self
    }
}

pub struct GreedyPatternRewriteDriver {
    context: Rc<Context>,
    worklist: RefCell<Worklist>,
    config: GreedyRewriteConfig,
    /// Not maintained when `config.restrict` is `GreedyRewriteStrictness::Any`
    filtered_ops: RefCell<BTreeSet<OperationRef>>,
    matcher: RefCell<PatternApplicator>,
}

impl GreedyPatternRewriteDriver {
    pub fn new(
        context: Rc<Context>,
        patterns: Rc<FrozenRewritePatternSet>,
        config: GreedyRewriteConfig,
    ) -> Self {
        // Apply a simple cost model based solely on pattern benefit
        let mut matcher = PatternApplicator::new(patterns);
        matcher.apply_default_cost_model();

        Self {
            context,
            worklist: Default::default(),
            config,
            filtered_ops: Default::default(),
            matcher: RefCell::new(matcher),
        }
    }
}

/// Worklist Managment
impl GreedyPatternRewriteDriver {
    /// Add the given operation to the worklist
    pub fn add_single_op_to_worklist(&self, op: OperationRef) {
        if matches!(self.config.restrict, GreedyRewriteStrictness::Any)
            || self.filtered_ops.borrow().contains(&op)
        {
            log::trace!("adding single op '{}' to worklist", op.borrow().name());
            self.worklist.borrow_mut().push(op);
        } else {
            log::trace!(
                "skipped adding single op '{}' to worklist due to strictness level",
                op.borrow().name()
            );
        }
    }

    /// Add the given operation, and its ancestors, to the worklist
    pub fn add_to_worklist(&self, op: OperationRef) {
        // Gather potential ancestors while looking for a `scope` parent region
        let mut ancestors = SmallVec::<[OperationRef; 8]>::default();
        let mut op = Some(op);
        while let Some(ancestor_op) = op.take() {
            let region = ancestor_op.borrow().parent_region();
            if self.config.scope.as_ref() == region.as_ref() {
                ancestors.push(ancestor_op);
                for op in ancestors {
                    self.add_single_op_to_worklist(op);
                }
                return;
            } else {
                log::trace!(
                    "gathering ancestors of '{}' for worklist",
                    ancestor_op.borrow().name()
                );
                ancestors.push(ancestor_op);
            }
            if let Some(region) = region {
                op = region.borrow().parent();
            } else {
                log::trace!("reached top level op while searching for ancestors");
            }
        }
    }

    /// Process operations until the worklist is empty, or `config.max_rewrites` is reached.
    ///
    /// Returns true if the IR was changed.
    pub fn process_worklist(self: Rc<Self>) -> bool {
        log::debug!("starting processing of greedy pattern rewrite driver worklist");
        let mut rewriter =
            PatternRewriter::new_with_listener(self.context.clone(), Rc::clone(&self));

        let mut changed = false;
        let mut num_rewrites = 0u32;
        while self.config.max_rewrites.is_none_or(|max| num_rewrites < max.get()) {
            let Some(op) = self.worklist.borrow_mut().pop() else {
                // Worklist is empty, we've converged
                log::debug!("processing worklist complete, rewrites have converged");
                return changed;
            };

            if self.process_worklist_item(&mut rewriter, op) {
                changed = true;
                num_rewrites += 1;
            }
        }

        log::debug!(
            "processing worklist was canceled after {} rewrites without converging (reached max \
             rewrite limit)",
            self.config.max_rewrites.map(|max| max.get()).unwrap_or(u32::MAX)
        );

        changed
    }

    /// Process a single operation from the worklist.
    ///
    /// Returns true if the IR was changed.
    fn process_worklist_item(
        &self,
        rewriter: &mut PatternRewriter<Rc<Self>>,
        mut op_ref: OperationRef,
    ) -> bool {
        let op = op_ref.borrow_mut();
        log::trace!("processing operation '{}'", op.name());

        // If the operation is trivially dead - remove it.
        if op.is_trivially_dead() {
            drop(op);
            rewriter.erase_op(op_ref);
            log::trace!("processing complete: operation is trivially dead");
            return true;
        }

        // Try to fold this op, unless it is a constant op, as that would lead to an infinite
        // folding loop, since the folded result would be immediately materialized as a constant
        // op, and then revisited.
        if !op.implements::<dyn ConstantLike>() {
            let mut results = SmallVec::<[OpFoldResult; 1]>::default();
            log::trace!("attempting to fold operation..");
            if op.fold(&mut results).is_ok() {
                if results.is_empty() {
                    // Op was modified in-place
                    self.notify_operation_modified(op_ref.clone());
                    log::trace!("operation was succesfully folded/modified in-place");
                    return true;
                } else {
                    log::trace!(
                        "operation was succesfully folded away, to be replaced with: {}",
                        crate::formatter::DisplayValues::new(results.iter())
                    );
                }

                // Op results can be replaced with `results`
                assert_eq!(
                    results.len(),
                    op.num_results(),
                    "folder produced incorrect number of results"
                );
                let mut rewriter = InsertionGuard::new(&mut **rewriter);
                rewriter.set_insertion_point_before(ProgramPoint::Op(op_ref.clone()));

                log::trace!("replacing op with fold results..");
                let mut replacements = SmallVec::<[ValueRef; 2]>::default();
                let mut materialization_succeeded = true;
                for (fold_result, result_ty) in results
                    .into_iter()
                    .zip(op.results().all().iter().map(|r| r.borrow().ty().clone()))
                {
                    match fold_result {
                        OpFoldResult::Value(value) => {
                            assert_eq!(
                                value.borrow().ty(),
                                &result_ty,
                                "folder produced value of incorrect type"
                            );
                            replacements.push(value);
                        }
                        OpFoldResult::Attribute(attr) => {
                            // Materialize attributes as SSA values using a constant op
                            let span = op.span();
                            log::trace!(
                                "materializing constant for value '{}' and type '{result_ty}'",
                                attr.render()
                            );
                            let constant_op = op.dialect().materialize_constant(
                                &mut *rewriter,
                                attr,
                                &result_ty,
                                span,
                            );
                            match constant_op {
                                None => {
                                    log::trace!(
                                        "materialization failed: cleaning up any materialized ops \
                                         for {} previous results",
                                        replacements.len()
                                    );
                                    // If materialization fails, clean up any operations generated for the previous results
                                    let mut replacement_ops =
                                        SmallVec::<[OperationRef; 2]>::default();
                                    for replacement in replacements.iter() {
                                        let replacement = replacement.borrow();
                                        assert!(
                                            !replacement.is_used(),
                                            "folder reused existing op for one result, but \
                                             constant materialization failed for another result"
                                        );
                                        let replacement_op = replacement.get_defining_op().unwrap();
                                        if replacement_ops.contains(&replacement_op) {
                                            continue;
                                        }
                                        replacement_ops.push(replacement_op);
                                    }
                                    for replacement_op in replacement_ops {
                                        rewriter.erase_op(replacement_op);
                                    }
                                    materialization_succeeded = false;
                                    break;
                                }
                                Some(constant_op) => {
                                    let const_op = constant_op.borrow();
                                    assert!(
                                        const_op.implements::<dyn ConstantLike>(),
                                        "materialize_constant produced op that does not implement \
                                         ConstantLike"
                                    );
                                    let result: ValueRef =
                                        const_op.results().all()[0].clone().upcast();
                                    assert_eq!(
                                        result.borrow().ty(),
                                        &result_ty,
                                        "materialize_constant produced incorrect result type"
                                    );
                                    log::trace!(
                                        "successfully materialized constant as {}",
                                        result.borrow().id()
                                    );
                                    replacements.push(result);
                                }
                            }
                        }
                    }
                }

                if materialization_succeeded {
                    log::trace!(
                        "materialization of fold results was successful, performing replacement.."
                    );
                    drop(op);
                    rewriter.replace_op_with_values(op_ref, &replacements);
                    log::trace!(
                        "fold succeeded: operation was replaced with materialized constants"
                    );
                    return true;
                } else {
                    log::trace!(
                        "materialization of fold results failed, proceeding without folding"
                    );
                }
            }
        } else {
            log::trace!("operation could not be folded");
        }

        // Try to match one of the patterns.
        //
        // The rewriter is automatically notified of any necessary changes, so there is nothing
        // else to do here.
        // TODO(pauls): if self.config.listener.is_some() {
        //
        // We need to trigger `notify_pattern_begin` in `can_apply`, and `notify_pattern_end`
        // in `on_failure` and `on_success`, but we can't have multiple mutable aliases of
        // the listener captured by these closures.
        //
        // This is another aspect of the listener infra that needs to be handled
        log::trace!("attempting to match and rewrite one of the input patterns..");
        let result = if let Some(listener) = self.config.listener.as_deref() {
            let op_name = op.name();
            let can_apply = |pattern: &dyn RewritePattern| {
                log::trace!("applying pattern {} to op {}", pattern.name(), &op_name);
                listener.notify_pattern_begin(pattern, op_ref.clone());
                true
            };
            let on_failure = |pattern: &dyn RewritePattern| {
                log::trace!("pattern failed to match");
                listener.notify_pattern_end(pattern, false);
            };
            let on_success = |pattern: &dyn RewritePattern| {
                log::trace!("pattern applied successfully");
                listener.notify_pattern_end(pattern, true);
                Ok(())
            };
            drop(op);
            self.matcher.borrow_mut().match_and_rewrite(
                op_ref.clone(),
                &mut **rewriter,
                can_apply,
                on_failure,
                on_success,
            )
        } else {
            drop(op);
            self.matcher.borrow_mut().match_and_rewrite(
                op_ref.clone(),
                &mut **rewriter,
                |_| true,
                |_| {},
                |_| Ok(()),
            )
        };

        match result {
            Ok(_) => {
                log::trace!("processing complete: pattern matched and operation was rewritten");
                true
            }
            Err(crate::PatternApplicationError::NoMatchesFound) => {
                log::debug!("processing complete: exhausted all patterns without finding a match");
                false
            }
            Err(crate::PatternApplicationError::Report(report)) => {
                log::debug!(
                    "processing complete: error occurred during match and rewrite: {report}"
                );
                false
            }
        }
    }

    /// Look over the operands of the provided op for any defining operations that should be re-
    /// added to the worklist. This function sho9uld be called when an operation is modified or
    /// removed, as it may trigger further simplifications.
    fn add_operands_to_worklist(&self, op: OperationRef) {
        let current_op = op.borrow();
        for operand in current_op.operands().all() {
            // If this operand currently has at most 2 users, add its defining op to the worklist.
            // After the op is deleted, then the operand will have at most 1 user left. If it has
            // 0 users left, it can be deleted as well, and if it has 1 user left, there may be
            // further canonicalization opportunities.
            let operand = operand.borrow();
            let Some(def_op) = operand.value().get_defining_op() else {
                continue;
            };

            let mut other_user = None;
            let mut has_more_than_two_uses = false;
            for user in operand.value().iter_uses() {
                if user.owner == op || other_user.as_ref().is_some_and(|ou| ou == &user.owner) {
                    continue;
                }
                if other_user.is_none() {
                    other_user = Some(user.owner.clone());
                    continue;
                }
                has_more_than_two_uses = true;
                break;
            }
            if !has_more_than_two_uses {
                self.add_to_worklist(def_op);
            }
        }
    }
}

/// Notifications
impl Listener for GreedyPatternRewriteDriver {
    fn kind(&self) -> crate::ListenerType {
        crate::ListenerType::Rewriter
    }

    /// Notify the driver that the given block was inserted
    fn notify_block_inserted(
        &self,
        block: crate::BlockRef,
        prev: Option<RegionRef>,
        ip: Option<crate::BlockRef>,
    ) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_block_inserted(block, prev, ip);
        }
    }

    /// Notify the driver that the specified operation was inserted.
    ///
    /// Update the worklist as needed: the operation is enqueued depending on scope and strictness
    fn notify_operation_inserted(&self, op: OperationRef, prev: Option<crate::InsertionPoint>) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_operation_inserted(op.clone(), prev.clone());
        }
        if matches!(self.config.restrict, GreedyRewriteStrictness::ExistingAndNew) {
            self.filtered_ops.borrow_mut().insert(op.clone());
        }
        self.add_to_worklist(op);
    }
}
impl RewriterListener for GreedyPatternRewriteDriver {
    /// Notify the driver that the given block is about to be removed.
    fn notify_block_erased(&self, block: BlockRef) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_block_erased(block);
        }
    }

    /// Notify the driver that the sepcified operation may have been modified in-place. The
    /// operation is added to the worklist.
    fn notify_operation_modified(&self, op: OperationRef) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_operation_modified(op.clone());
        }
        self.add_to_worklist(op);
    }

    /// Notify the driver that the specified operation was removed.
    ///
    /// Update the worklist as needed: the operation and its children are removed from the worklist
    fn notify_operation_erased(&self, op: OperationRef) {
        // Only ops that are within the configured scope are added to the worklist of the greedy
        // pattern rewriter.
        //
        // A greedy pattern rewrite is not allowed to erase the parent op of the scope region, as
        // that would break the worklist handling and some sanity checks.
        if let Some(scope) = self.config.scope.as_ref() {
            assert!(
                scope.borrow().parent().is_some_and(|parent_op| parent_op != op),
                "scope region must not be erased during greedy pattern rewrite"
            );
        }

        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_operation_erased(op.clone());
        }

        self.add_operands_to_worklist(op.clone());
        self.worklist.borrow_mut().remove(&op);

        if self.config.restrict != GreedyRewriteStrictness::Any {
            self.filtered_ops.borrow_mut().remove(&op);
        }
    }

    /// Notify the driver that the specified operation was replaced.
    ///
    /// Update the worklist as needed: new users are enqueued
    fn notify_operation_replaced_with_values(&self, op: OperationRef, replacement: &[ValueRef]) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_operation_replaced_with_values(op, replacement);
        }
    }

    fn notify_match_failure(&self, span: SourceSpan, reason: Report) {
        if let Some(listener) = self.config.listener.as_deref() {
            listener.notify_match_failure(span, reason);
        }
    }
}

pub struct RegionPatternRewriteDriver {
    driver: Rc<GreedyPatternRewriteDriver>,
    region: RegionRef,
}
impl RegionPatternRewriteDriver {
    pub fn new(
        context: Rc<Context>,
        patterns: Rc<FrozenRewritePatternSet>,
        config: GreedyRewriteConfig,
        region: RegionRef,
    ) -> Self {
        use crate::Walkable;
        let mut driver = GreedyPatternRewriteDriver::new(context, patterns, config);
        // Populate strict mode ops, if applicable
        if driver.config.restrict != GreedyRewriteStrictness::Any {
            let filtered_ops = driver.filtered_ops.get_mut();
            region.borrow().postwalk(|op| {
                filtered_ops.insert(op);
            });
        }
        Self {
            driver: Rc::new(driver),
            region,
        }
    }

    /// Simplify ops inside `self.region`, and simplify the region itself.
    ///
    /// Returns `Ok(changed)` if the transformation converged, with `changed` indicating whether or
    /// not the IR was changed. Otherwise, `Err(changed)` is returned.
    pub fn simplify(&mut self) -> Result<bool, bool> {
        use crate::matchers::Matcher;

        let mut continue_rewrites = false;
        let mut iteration = 0;

        while self.driver.config.max_iterations.is_none_or(|max| iteration < max.get()) {
            log::trace!("starting iteration {iteration} of region pattern rewrite driver");
            iteration += 1;

            // New iteration: start with an empty worklist
            self.driver.worklist.borrow_mut().clear();

            // `OperationFolder` CSE's constant ops (and may move them into parents regions to
            // enable more aggressive CSE'ing).
            let context = self.driver.context.clone();
            let mut folder = OperationFolder::new(context, Rc::clone(&self.driver));
            let mut insert_known_constant = |op: OperationRef| {
                // Check for existing constants when populating the worklist. This avoids
                // accidentally reversing the constant order during processing.
                if let Some(const_value) = crate::matchers::constant().matches(&op.borrow()) {
                    if !folder.insert_known_constant(op, Some(const_value)) {
                        return true;
                    }
                }
                false
            };

            if !self.driver.config.use_top_down_traversal {
                // Add operations to the worklist in postorder.
                log::trace!("adding operations in postorder");
                self.region.borrow().postwalk(|op| {
                    if !insert_known_constant(op.clone()) {
                        self.driver.add_to_worklist(op);
                    }
                });
            } else {
                // Add all nested operations to the worklist in preorder.
                log::trace!("adding operations in preorder");
                self.region
                    .borrow()
                    .prewalk_interruptible(|op| {
                        if !insert_known_constant(op.clone()) {
                            self.driver.add_to_worklist(op);
                            WalkResult::<Report>::Continue(())
                        } else {
                            WalkResult::Skip
                        }
                    })
                    .into_result()
                    .expect("unexpected error occurred while walking region");

                // Reverse the list so our loop processes them in-order
                self.driver.worklist.borrow_mut().reverse();
            }

            continue_rewrites = self.driver.clone().process_worklist();
            log::trace!(
                "processing of worklist for this iteration has completed, \
                 changed={continue_rewrites}"
            );

            // After applying patterns, make sure that the CFG of each of the regions is kept up to
            // date.
            if self.driver.config.region_simplification != RegionSimplificationLevel::None {
                let mut rewriter = PatternRewriter::new_with_listener(
                    self.driver.context.clone(),
                    Rc::clone(&self.driver),
                );
                continue_rewrites |= Region::simplify_all(
                    &[self.region.clone()],
                    &mut *rewriter,
                    self.driver.config.region_simplification,
                )
                .is_ok();
            } else {
                log::debug!("region simplification was disabled, skipping simplification rewrites");
            }

            if !continue_rewrites {
                log::trace!("region pattern rewrites have converged");
                break;
            }
        }

        // If `continue_rewrites` is false, then the rewrite converged, i.e. the IR wasn't changed
        // in the last iteration.
        if !continue_rewrites {
            Ok(iteration > 1)
        } else {
            Err(iteration > 1)
        }
    }
}

pub struct MultiOpPatternRewriteDriver {
    driver: Rc<GreedyPatternRewriteDriver>,
    inner: Rc<MultiOpPatternRewriteDriverImpl>,
}

struct MultiOpPatternRewriteDriverImpl {
    surviving_ops: RefCell<BTreeSet<OperationRef>>,
}

impl MultiOpPatternRewriteDriver {
    pub fn new(
        context: Rc<Context>,
        patterns: Rc<FrozenRewritePatternSet>,
        mut config: GreedyRewriteConfig,
        ops: &[OperationRef],
    ) -> Self {
        let surviving_ops = BTreeSet::from_iter(ops.iter().cloned());
        let inner = Rc::new(MultiOpPatternRewriteDriverImpl {
            surviving_ops: RefCell::new(surviving_ops),
        });
        let listener = Rc::new(ForwardingListener::new(config.listener.take(), Rc::clone(&inner)));
        config.listener = Some(listener);

        let mut driver = GreedyPatternRewriteDriver::new(context.clone(), patterns, config);
        if driver.config.restrict != GreedyRewriteStrictness::Any {
            driver.filtered_ops.get_mut().extend(ops.iter().cloned());
        }

        Self {
            driver: Rc::new(driver),
            inner,
        }
    }

    pub fn simplify(&mut self, ops: &[OperationRef]) -> Result<bool, bool> {
        // Populate the initial worklist
        for op in ops {
            self.driver.add_single_op_to_worklist(op.clone());
        }

        // Process ops on the worklist
        let changed = self.driver.clone().process_worklist();
        if self.driver.worklist.borrow().is_empty() {
            Ok(changed)
        } else {
            Err(changed)
        }
    }
}

impl Listener for MultiOpPatternRewriteDriverImpl {
    fn kind(&self) -> crate::ListenerType {
        crate::ListenerType::Rewriter
    }
}
impl RewriterListener for MultiOpPatternRewriteDriverImpl {
    fn notify_operation_erased(&self, op: OperationRef) {
        self.surviving_ops.borrow_mut().remove(&op);
    }
}

#[derive(Default)]
struct Worklist(Vec<OperationRef>);
impl Worklist {
    /// Clear all operations from the worklist
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// Returns true if the worklist is empty
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Push an operation to the end of the worklist, unless it is already in the worklist.
    pub fn push(&mut self, op: OperationRef) {
        if self.0.contains(&op) {
            return;
        }
        self.0.push(op);
    }

    /// Pop the next operation from the worklist
    #[inline]
    pub fn pop(&mut self) -> Option<OperationRef> {
        self.0.pop()
    }

    /// Remove `op` from the worklist
    pub fn remove(&mut self, op: &OperationRef) {
        if let Some(index) = self.0.iter().position(|o| o == op) {
            self.0.remove(index);
        }
    }

    /// Reverse the worklist
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
}
