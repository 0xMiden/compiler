use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    AsValueRange, BlockRef, EntityMut, FxHashMap, Operation, OperationName, OperationRef,
    RegionRef, Report, Rewriter, RewriterExt, SmallVec, ValueRef,
    adt::SmallDenseMap,
    cfg::Graph,
    dominance::{DomTreeNode, DominanceInfo, PostDominanceInfo},
    effects::MemoryEffect,
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, RewriterListener, TracingRewriterListener},
    traits::{IsolatedFromAbove, Terminator},
};

/// This transformation pass performs a simple common sub-expression elimination algorithm on
/// operations within a region.
pub struct CommonSubexpressionElimination;

impl Pass for CommonSubexpressionElimination {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "cse"
    }

    fn argument(&self) -> &'static str {
        "common-subexpression-elimination"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        // Run sparse constant propagation + dead code analysis
        let op = op.into_entity_ref();

        // Rewrite based on results of analysis
        let context = op.context_rc();
        let op = {
            let op_ref = op.as_operation_ref();
            drop(op);
            op_ref
        };

        let dominfo = state.analysis_manager().get_analysis::<DominanceInfo>()?;
        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);

        let mut driver = CSEDriver {
            rewriter: &mut rewriter,
            domtree: dominfo,
            ops_to_erase: Default::default(),
            mem_effects_cache: Default::default(),
        };

        let status = driver.simplify(op);

        if status.ir_changed() {
            // We currently don't remove region operations, so mark dominance as preserved.
            state.preserved_analyses_mut().preserve::<DominanceInfo>();
            state.preserved_analyses_mut().preserve::<PostDominanceInfo>();
        } else {
            // If there was no change to the IR, we mark all analyses as preserved.
            state.preserved_analyses_mut().preserve_all();
        }

        Ok(())
    }
}

/// Simple common sub-expression elimination.
struct CSEDriver<'a> {
    rewriter: &'a mut RewriterImpl<TracingRewriterListener>,
    /// Operations marked as dead and to be erased.
    ops_to_erase: Vec<OperationRef>,
    /// Dominance info provided by the current analysis state
    domtree: Rc<DominanceInfo>,
    /// Cache holding MemoryEffect information between two operations.
    ///
    /// The first operation is the key, the second operation is paired with whatever effect exists
    /// between the two operations. If the effect is None, then we assume there is no operation with
    /// `MemoryEffect::Write` between the two operations.
    mem_effects_cache: SmallDenseMap<OperationRef, (OperationRef, Option<MemoryEffect>)>,
}

type ScopedMap = FxHashMap<OpKey, OperationRef>;

impl CSEDriver<'_> {
    pub fn simplify(&mut self, op: OperationRef) -> PostPassStatus {
        // Simplify all regions.
        let mut known_values = ScopedMap::default();
        let mut next_region = op.borrow().regions().front().as_pointer();
        let mut status = PostPassStatus::Unchanged;
        while let Some(region) = next_region.take() {
            next_region = region.next();

            status |= self.simplify_region(&mut known_values, region);
        }

        // Erase any operations that were marked as dead during simplification.
        status |= PostPassStatus::from(!self.ops_to_erase.is_empty());
        for op in self.ops_to_erase.drain(..) {
            self.rewriter.erase_op(op);
        }

        status
    }

    /// Attempt to eliminate a redundant operation.
    ///
    /// Returns success if the operation was marked for removal, failure otherwise.
    fn simplify_operation(
        &mut self,
        known_values: &mut ScopedMap,
        op: OperationRef,
        has_ssa_dominance: bool,
    ) -> PostPassStatus {
        // Don't simplify terminator operations.
        let operation = op.borrow();
        if operation.implements::<dyn Terminator>() {
            return PostPassStatus::Unchanged;
        }

        // If the operation is already trivially dead just add it to the erase list.
        if operation.is_trivially_dead() {
            self.ops_to_erase.push(op);
            return PostPassStatus::Changed;
        }

        // Don't simplify operations with regions that have multiple blocks.
        // TODO: We need additional tests to verify that we handle such IR correctly.
        if !operation.regions().iter().all(|r| r.is_empty() || r.has_one_block()) {
            return PostPassStatus::Unchanged;
        }

        // Some simple use case of operation with memory side-effect are dealt with here.
        // Operations with no side-effect are done after.
        if !operation.is_memory_effect_free() {
            // TODO: Only basic use case for operations with MemoryEffects::Read can be
            // eleminated now. More work needs to be done for more complicated patterns
            // and other side-effects.
            if !operation.has_single_memory_effect(MemoryEffect::Read) {
                return PostPassStatus::Unchanged;
            }

            // Look for an existing definition for the operation.
            if let Some(existing) = known_values.get(&OpKey(op)).copied()
                && existing.parent() == op.parent()
                && !self.has_other_side_effecting_op_in_between(existing, op)
            {
                // The operation that can be deleted has been reach with no
                // side-effecting operations in between the existing operation and
                // this one so we can remove the duplicate.
                self.replace_uses_and_delete(known_values, op, existing, has_ssa_dominance);
                return PostPassStatus::Changed;
            }
            known_values.insert(OpKey(op), op);
            return PostPassStatus::Unchanged;
        }

        // Look for an existing definition for the operation.
        if let Some(existing) = known_values.get(&OpKey(op)).copied() {
            self.replace_uses_and_delete(known_values, op, existing, has_ssa_dominance);
            PostPassStatus::Changed
        } else {
            // Otherwise, we add this operation to the known values map.
            known_values.insert(OpKey(op), op);
            PostPassStatus::Unchanged
        }
    }

    fn simplify_block(
        &mut self,
        known_values: &mut ScopedMap,
        block: BlockRef,
        has_ssa_dominance: bool,
    ) -> PostPassStatus {
        let mut changed = PostPassStatus::Unchanged;
        let mut next_op = block.borrow().body().front().as_pointer();
        while let Some(op) = next_op.take() {
            next_op = op.next();

            // Most operations don't have regions, so fast path that case.
            let operation = op.borrow();
            if operation.has_regions() {
                // If this operation is isolated above, we can't process nested regions with the
                // given 'known_values' map. This would cause the insertion of implicit captures in
                // explicit capture only regions.
                if operation.implements::<dyn IsolatedFromAbove>() {
                    let mut nested_known_values = ScopedMap::default();
                    let mut next_region = operation.regions().front().as_pointer();
                    while let Some(region) = next_region.take() {
                        next_region = region.next();

                        changed |= self.simplify_region(&mut nested_known_values, region);
                    }
                } else {
                    // Otherwise, process nested regions normally.
                    let mut next_region = operation.regions().front().as_pointer();
                    while let Some(region) = next_region.take() {
                        next_region = region.next();

                        changed |= self.simplify_region(known_values, region);
                    }
                }
            }

            changed |= self.simplify_operation(known_values, op, has_ssa_dominance);
        }

        // Clear the MemoryEffect cache since its usage is by block only.
        self.mem_effects_cache.clear();

        changed
    }

    fn simplify_region(
        &mut self,
        known_values: &mut ScopedMap,
        region: RegionRef,
    ) -> PostPassStatus {
        // If the region is empty there is nothing to do.
        let region = region.borrow();
        if region.is_empty() {
            return PostPassStatus::Unchanged;
        }

        let has_ssa_dominance = self.domtree.has_ssa_dominance(region.as_region_ref());

        // If the region only contains one block, then simplify it directly.
        if region.has_one_block() {
            let mut scope = known_values.clone();
            let block = region.entry_block_ref().unwrap();
            drop(region);
            return self.simplify_block(&mut scope, block, has_ssa_dominance);
        }

        // If the region does not have dominanceInfo, then skip it.
        // TODO: Regions without SSA dominance should define a different traversal order which is
        // appropriate and can be used here.
        if !has_ssa_dominance {
            return PostPassStatus::Unchanged;
        }

        // Process the nodes of the dom tree for this region.
        let mut stack = Vec::<CfgStackNode>::with_capacity(16);
        let dominfo = self.domtree.dominance(region.as_region_ref());
        stack.push(CfgStackNode::new(known_values.clone(), dominfo.root_node().unwrap()));

        let mut changed = PostPassStatus::Unchanged;
        while let Some(current_node) = stack.last_mut() {
            // Check to see if we need to process this node.
            if !current_node.processed {
                current_node.processed = true;
                changed |= self.simplify_block(
                    &mut current_node.scope,
                    current_node.node.block().unwrap(),
                    has_ssa_dominance,
                );
            }

            // Otherwise, check to see if we need to process a child node.
            if let Some(next_child) = current_node.children.next() {
                let scope = current_node.scope.clone();
                stack.push(CfgStackNode::new(scope, next_child));
            } else {
                // Finally, if the node and all of its children have been processed then we delete
                // the node.
                stack.pop();
            }
        }
        changed
    }

    fn replace_uses_and_delete(
        &mut self,
        known_values: &ScopedMap,
        op: OperationRef,
        mut existing: OperationRef,
        has_ssa_dominance: bool,
    ) {
        // If we find one then replace all uses of the current operation with the existing one and
        // mark it for deletion. We can only replace an operand in an operation if it has not been
        // visited yet.
        if has_ssa_dominance {
            // If the region has SSA dominance, then we are guaranteed to have not visited any use
            // of the current operation.
            self.rewriter.notify_operation_replaced(op, existing);
            // Replace all uses, but do not remove the operation yet. This does not notify the
            // listener because the original op is not erased.
            let operation = op.borrow();
            let existing = existing.borrow();
            let op_results = operation.results().as_value_range().into_smallvec();
            let existing_results = existing
                .results()
                .iter()
                .copied()
                .map(|result| Some(result as ValueRef))
                .collect::<SmallVec<[_; 2]>>();
            self.rewriter.replace_all_uses_with(&op_results, &existing_results);
            self.ops_to_erase.push(op);
        } else {
            // When the region does not have SSA dominance, we need to check if we have visited a
            // use before replacing any use.
            let was_visited = |operand: &midenc_hir::OpOperandImpl| {
                !known_values.contains_key(&OpKey(operand.owner))
            };

            let op_results = op.borrow().results().as_value_range().into_smallvec();
            let should_replace_op = op_results.iter().all(|v| {
                let v = v.borrow();
                v.iter_uses().all(|user| was_visited(&user))
            });
            if should_replace_op {
                self.rewriter.notify_operation_replaced(op, existing);
            }

            // Replace all uses, but do not remove the operation yet. This does not notify the
            // listener because the original op is not erased.
            let existing_results = existing.borrow().results().as_value_range().into_smallvec();
            self.rewriter
                .maybe_replace_uses_with(&op_results, &existing_results, was_visited);

            // There may be some remaining uses of the operation.
            if !op.borrow().is_used() {
                self.ops_to_erase.push(op);
            }
        }

        // If the existing operation has an unknown location and the current operation doesn't,
        // then set the existing op's location to that of the current op.
        let mut existing = existing.borrow_mut();
        let op_span = op.borrow().span;
        if existing.span.is_unknown() && !op_span.is_unknown() {
            existing.set_span(op_span);
        }
    }

    /// Check if there is side-effecting operations other than the given effect between the two
    /// operations.
    fn has_other_side_effecting_op_in_between(
        &mut self,
        from: OperationRef,
        to: OperationRef,
    ) -> bool {
        assert_eq!(from.parent(), to.parent(), "expected operations to be in the same block");
        let from_op = from.borrow();
        assert!(
            from_op.has_memory_effect(MemoryEffect::Read),
            "expected read effect on `from` op"
        );
        assert!(
            to.borrow().has_memory_effect(MemoryEffect::Read),
            "expected read effect on `to` op"
        );

        let result = self.mem_effects_cache.entry(from).or_insert((from, None));
        let mut next_op = if result.1.is_none() {
            // No `MemoryEffect::Write` has been detected until the cached operation, continue
            // looking from the cached operation to `to`.
            Some(result.0)
        } else {
            // MemoryEffects::Write has been detected before so there is no need to check
            // further.
            return true;
        };

        while let Some(next) = next_op.take()
            && next != to
        {
            next_op = next.next();

            let effects = next.borrow().get_effects_recursively::<MemoryEffect>();
            if let Some(effects) = effects.as_deref() {
                for effect in effects {
                    if effect.effect() == MemoryEffect::Write {
                        *result = (next, Some(MemoryEffect::Write));
                        return true;
                    }
                }
            } else {
                // TODO: Do we need to handle other effects generically?
                // If the operation does not implement the MemoryEffectOpInterface we conservatively
                // assume it writes.
                *result = (next, Some(MemoryEffect::Write));
                return true;
            }
        }

        *result = (to, None);
        false
    }
}

/// Represents a single entry in the depth first traversal of a CFG.
struct CfgStackNode {
    /// Scope for the known values.
    scope: ScopedMap,
    node: Rc<DomTreeNode>,
    children: <Rc<DomTreeNode> as Graph>::ChildIter,
    /// If this node has been fully processed yet or not.
    processed: bool,
}

impl CfgStackNode {
    pub fn new(scope: ScopedMap, node: Rc<DomTreeNode>) -> Self {
        let children = <Rc<DomTreeNode> as Graph>::children(node.clone());
        Self {
            scope,
            node,
            children,
            processed: false,
        }
    }
}

/// A wrapper type for [OperationRef] which hashes/compares using operation equivalence flags that
/// ignore locations and result values, considering only operands and properties of the operation
/// itself
#[derive(Copy, Clone)]
struct OpKey(OperationRef);

impl core::hash::Hash for OpKey {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use midenc_hir::equivalence::{
            DefaultValueHasher, IgnoreValueHasher, OperationEquivalenceFlags,
        };
        self.0.borrow().hash_with_options(
            OperationEquivalenceFlags::IGNORE_LOCATIONS,
            DefaultValueHasher,
            IgnoreValueHasher,
            state,
        );
    }
}

impl Eq for OpKey {}
impl PartialEq for OpKey {
    fn eq(&self, other: &Self) -> bool {
        use midenc_hir::equivalence::OperationEquivalenceFlags;

        if self.0 == other.0 {
            return true;
        }
        let lhs = self.0.borrow();
        let rhs = other.0.borrow();

        lhs.is_equivalent_with_options(&rhs, OperationEquivalenceFlags::IGNORE_LOCATIONS, |l, r| {
            core::ptr::addr_eq(l, r)
        })
    }
}
