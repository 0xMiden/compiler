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

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, string::ToString, sync::Arc};

    use litcheck_filecheck::filecheck;
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        AbiParam, Context, Ident, OpBuilder, OpPrinter, PointerType, Signature, SourceSpan, Type,
        dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder, FunctionRef},
        pass::PassManager,
    };

    use super::*;

    #[test]
    fn simple_constant() {
        let mut test = Test::new("simple_constant", &[], &[Type::I32, Type::I32]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.i32(1, SourceSpan::UNKNOWN);
            let v1 = builder.i32(1, SourceSpan::UNKNOWN);
            builder.ret([v0, v1], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @simple_constant() -> (i32, i32) {
^block0:
    // CHECK: [[V0:v\d+]] = arith.constant 1 : i32;
    v0 = arith.constant 1 : i32;

    // CHECK-NEXT: builtin.ret [[V0]], [[V0]];
    v1 = arith.constant 1 : i32;
    builtin.ret v0, v1;
};
            "#
        );
    }

    #[test]
    fn basic() {
        let mut test = Test::new("basic", &[], &[Type::I32, Type::I32]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.i32(0, SourceSpan::UNKNOWN);
            let v1 = builder.i32(0, SourceSpan::UNKNOWN);
            let v2 = builder.i32(1, SourceSpan::UNKNOWN);
            let v3 = builder.mul(v0, v2, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.mul(v1, v2, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v3, v4], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @basic() -> (i32, i32) {
^block0:
    // CHECK: [[V0:v\d+]] = arith.constant 0 : i32;
    v0 = arith.constant 0 : i32;
    v1 = arith.constant 0 : i32;

    // CHECK-NEXT: [[V2:v\d+]] = arith.constant 1 : i32;
    v2 = arith.constant 1 : i32;

    // CHECK-NEXT: [[V3:v\d+]] = arith.mul [[V0]], [[V2]] : i32 #[overflow = checked];
    v3 = arith.mul v0, v2 : i32 #[overflow = checked];
    v4 = arith.mul v1, v2 : i32 #[overflow = checked];

    // CHECK-NEXT: builtin.ret [[V3]], [[V3]];
    builtin.ret v3, v3;
};
            "#
        );
    }

    #[test]
    fn many() {
        let mut test = Test::new("many", &[Type::I32, Type::I32], &[Type::I32]);
        {
            let mut builder = test.function_builder();
            let [v0, v1] = *builder.entry_block().borrow().arguments()[0..2].as_array().unwrap();
            let v0 = v0 as ValueRef;
            let v1 = v1 as ValueRef;
            let v2 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v4 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v5 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v6 = builder.add(v2, v3, SourceSpan::UNKNOWN).unwrap();
            let v7 = builder.add(v4, v5, SourceSpan::UNKNOWN).unwrap();
            let v8 = builder.add(v2, v4, SourceSpan::UNKNOWN).unwrap();
            let v9 = builder.add(v6, v7, SourceSpan::UNKNOWN).unwrap();
            let v10 = builder.add(v7, v8, SourceSpan::UNKNOWN).unwrap();
            let v11 = builder.add(v9, v10, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v11], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @many(v0: i32, v1: i32) -> i32 {
^block0(v0: i32, v1: i32):
    // CHECK: [[V2:v\d+]] = arith.add v{{\d+}}, v{{\d+}} : i32 #[overflow = checked];
    v2 = arith.add v0 v1 : i32 #[overflow = checked];
    v3 = arith.add v0 v1 : i32 #[overflow = checked];
    v4 = arith.add v0 v1 : i32 #[overflow = checked];
    v5 = arith.add v0 v1 : i32 #[overflow = checked];

    // CHECK-NEXT: [[V6:v\d+]] = arith.add [[V2]], [[V2]] : i32 #[overflow = checked];
    v6 = arith.add v2, v3 : i32 #[overflow = checked];
    v7 = arith.add v4, v5 : i32 #[overflow = checked];
    v8 = arith.add v2, v4 : i32 #[overflow = checked];

    // CHECK-NEXT: [[V9:v\d+]] = arith.add [[V6]], [[V6]] : i32 #[overflow = checked];
    v9 = arith.add v6, v7 : i32 #[overflow = checked];
    v10 = arith.add v7, v8 : i32 #[overflow = checked];

    // CHECK-NEXT: [[V11:v\d+]] = arith.add [[V9]], [[V9]] : i32 #[overflow = checked];
    v11 = arith.add v9, v10 : i32 #[overflow = checked];

    // CHECK-NEXT: builtin.ret [[V11]];
    builtin.ret v11;
};
            "#
        );
    }

    /// Check that operations are not eliminated if they have different operands.
    #[test]
    fn ops_with_different_operands_are_not_elimited() {
        let mut test = Test::new("different_operands", &[], &[Type::I32, Type::I32]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.i32(0, SourceSpan::UNKNOWN);
            let v1 = builder.i32(1, SourceSpan::UNKNOWN);
            builder.ret([v0, v1], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @different_operands() -> (i32, i32) {
^block0:
    // CHECK: [[V0:v\d+]] = arith.constant 0 : i32;
    // CHECK-NEXT: [[V1:v\d+]] = arith.constant 1 : i32;
    v0 = arith.constant 0 : i32;
    v1 = arith.constant 1 : i32;

    // CHECK-NEXT: builtin.ret [[V0]], [[V1]];
    builtin.ret v0, v1;
};
            "#
        );
    }

    /// Check that operations are not eliminated if they have different result types.
    #[test]
    fn ops_with_different_result_types_are_not_elimited() {
        let mut test = Test::new("different_results", &[Type::I32], &[Type::I64, Type::I128]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.sext(v0, Type::I64, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.sext(v0, Type::I128, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v1, v2], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @different_results(v0: i32) -> (i64, i128) {
^block0(v0: i32):
    // CHECK: [[V1:v\d+]] = arith.sext v0 : i64;
    // CHECK-NEXT: [[V2:v\d+]] = arith.sext v0 : i128;
    v1 = arith.cast v0 : i64;
    v2 = arith.cast v0 : i128;

    // CHECK-NEXT: builtin.ret [[V1]], [[V2]];
    builtin.ret v1, v2;
};
            "#
        );
    }

    /// Check that operations are not eliminated if they have different attributes.
    #[test]
    fn ops_with_different_attributes_are_not_elimited() {
        let mut test = Test::new("different_attributes", &[Type::I32], &[Type::I32, Type::I32]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.i32(1, SourceSpan::UNKNOWN);
            let v2 = builder.add(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.add_unchecked(v0, v1, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v2, v3], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @different_attributes(v0: i32) -> (i32, i32) {
^block0(v0: i32):
    // CHECK: [[V1:v\d+]] = arith.constant 1 : i32;
    v1 = arith.constant 1 : i32;

    // CHECK-NEXT: [[V2:v\d+]] = arith.add v0, v1 : i32 #[overflow = checked];
    v2 = arith.add v0, v1 : i32 #[overflow = checked];

    // CHECK-NEXT: [[V3:v\d+]] = arith.add v0, v1 : i32 #[overflow = unchecked];
    v3 = arith.add v0, v1 : i32 #[overflow = unchecked];

    // CHECK-NEXT: builtin.ret [[V2]], [[V3]];
    builtin.ret v2, v3;
};
            "#
        );
    }

    /// Check that operations with side effects are not eliminated.
    #[test]
    fn ops_with_side_effects_are_not_elimited() {
        use midenc_dialect_hir::HirOpBuilder;

        let byte_ptr = Type::Ptr(Arc::new(PointerType::new(Type::U8)));
        let mut test =
            Test::new("side_effect", core::slice::from_ref(&byte_ptr), &[Type::U8, Type::U8]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let v1 = builder.u8(1, SourceSpan::UNKNOWN);
            builder.store(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v2 = builder.load(v0, SourceSpan::UNKNOWN).unwrap();
            builder.store(v0, v1, SourceSpan::UNKNOWN).unwrap();
            let v3 = builder.load(v0, SourceSpan::UNKNOWN).unwrap();
            builder.ret([v2, v3], SourceSpan::UNKNOWN).unwrap();
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        filecheck!(
            output,
            r#"
builtin.function @side_effect(v0: ptr<u8, byte>) -> (u8, u8) {
^block0(v0: ptr<u8, byte>):
    // CHECK: [[V1:v\d+]] = arith.constant 1 : u8;
    v1 = arith.constant 1 : u8;

    // CHECK-NEXT: hir.store v0, v1;
    // CHECK-NEXT: [[V2:v\d+]] = hir.load v0 : u8;
    hir.store v0, v1;
    v2 = hir.load v0 : u8;

    // CHECK-NEXT: hir.store v0, v1;
    // CHECK-NEXT: [[V3:v\d+]] = hir.load v0 : u8;
    hir.store v0, v1;
    v3 = hir.load v0 : u8;

    // CHECK-NEXT: builtin.ret [[V2]], [[V3]];
    builtin.ret v2, v3;
};
            "#
        );
    }

    /// Check that operation definitions are properly propagated down the dominance tree.
    #[test]
    fn proper_propagation_of_ops_down_dominance_tree() {
        use midenc_dialect_scf::StructuredControlFlowOpBuilder;

        let mut test = Test::new("down_propagate_while", &[], &[]);
        {
            let mut builder = test.function_builder();
            let v0 = builder.i32(0, SourceSpan::UNKNOWN);
            let v1 = builder.i32(1, SourceSpan::UNKNOWN);
            let v2 = builder.i32(4, SourceSpan::UNKNOWN);
            let while_op = builder.r#while([v0, v1], &[], SourceSpan::UNKNOWN).unwrap();
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
            {
                let before_block = while_op.borrow().before().entry().as_block_ref();
                let after_block = while_op.borrow().after().entry().as_block_ref();
                builder.switch_to_block(before_block);
                let [v3, v4] = *before_block.borrow().arguments()[0..2].as_array().unwrap();
                let v3 = v3 as ValueRef;
                let v4 = v4 as ValueRef;
                let v5 = builder.i32(1, SourceSpan::UNKNOWN);
                let v6 = builder.add(v3, v5, SourceSpan::UNKNOWN).unwrap();
                let v7 = builder.lt(v6, v2, SourceSpan::UNKNOWN).unwrap();
                builder.condition(v7, [v6, v4], SourceSpan::UNKNOWN).unwrap();

                builder.switch_to_block(after_block);
                let v8 = builder.append_block_param(after_block, Type::I32, SourceSpan::UNKNOWN);
                let v9 = builder.append_block_param(after_block, Type::I32, SourceSpan::UNKNOWN);
                builder.r#yield([v8 as ValueRef, v9 as ValueRef], SourceSpan::UNKNOWN).unwrap();
            }
        }

        test.run_cse(true).expect("invalid ir");

        let output =
            format!("{}", test.function().borrow().print(&Default::default(), &test.context));
        std::println!("output: {output}");
        filecheck!(
            output,
            r#"
builtin.function @down_propagate_while() {
^block0:
    // CHECK: [[V0:v\d+]] = arith.constant 0 : i32;
    v0 = arith.constant 0 : i32;
    // CHECK: [[V1:v\d+]] = arith.constant 1 : i32;
    v1 = arith.constant 1 : i32;
    // CHECK: [[V2:v\d+]] = arith.constant 4 : i32;
    v2 = arith.constant 4 : i32;

    // CHECK-NEXT: scf.while v0, v1 {
    // CHECK-NEXT: ^block{{\d}}([[V3:v\d+]]: i32, [[V4:v\d+]]: i32):
    scf.while v0, v1 {
    ^block1(v3: i32, v4: i32):
        // CHECK-NEXT: [[V6:v\d+]] = arith.add [[V3]], [[V1]] : i32 #[overflow = checked];
        v5 = arith.constant 1 : i32;
        v6 = arith.add v3, v5 : i32 #[overflow = checked];
        // CHECK-NEXT: [[V7:v\d+]] = arith.lt [[V6]], [[V2]] : i1;
        v7 = arith.lt v6, v2 : i1;
        // CHECK-NEXT: scf.condition [[V7]], [[V6]], [[V4]];
        scf.condition v7, v6, v4;
    } do {
    ^block2(v8: i32, v9: i32):
        // CHECK-NEXT: } do {
        // CHECK-NEXT: ^block{{\d}}([[V8:v\d+]]: i32, [[V9:v\d+]]: i32):
        // CHECK-NEXT: scf.yield [[V8]], [[V9]];
        scf.yield v8, v9;
    }

    // CHECK: builtin.ret ;
    builtin.ret;
};
            "#
        );
    }

    fn enable_compiler_instrumentation() {
        let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
            .format_timestamp(None)
            .is_test(true)
            .try_init();
    }

    struct Test {
        context: Rc<Context>,
        builder: OpBuilder,
        function: FunctionRef,
    }

    impl Test {
        pub fn new(name: &'static str, params: &[Type], results: &[Type]) -> Self {
            enable_compiler_instrumentation();

            let context = Rc::new(Context::default());
            let mut builder = OpBuilder::new(context.clone());
            let function = builder
                .create_function(
                    Ident::with_empty_span(name.into()),
                    Signature::new(
                        params.iter().cloned().map(AbiParam::new),
                        results.iter().cloned().map(AbiParam::new),
                    ),
                )
                .unwrap();

            Self {
                context,
                builder,
                function,
            }
        }

        pub fn function(&self) -> FunctionRef {
            self.function
        }

        pub fn function_builder(&mut self) -> FunctionBuilder<'_, OpBuilder> {
            FunctionBuilder::new(self.function, &mut self.builder)
        }

        pub fn run_cse(&self, verify: bool) -> Result<(), Report> {
            let mut pm = PassManager::on::<Function>(
                self.context.clone(),
                midenc_hir::pass::Nesting::Explicit,
            );
            pm.add_pass(Box::new(CommonSubexpressionElimination));
            pm.enable_verifier(verify);
            pm.run(self.function.as_operation_ref())
        }
    }
}

//------------ TESTS TO FINISH TRANSLATING
/*

// CHECK-LABEL: @down_propagate
func.func @down_propagate() -> i32 {
  // CHECK-NEXT: %[[VAR_c1_i32:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
  %0 = arith.constant 1 : i32

  // CHECK-NEXT: %[[VAR_true:[0-9a-zA-Z_]+]] = arith.constant true
  %cond = arith.constant true

  // CHECK-NEXT: cf.cond_br %[[VAR_true]], ^bb1, ^bb2(%[[VAR_c1_i32]] : i32)
  cf.cond_br %cond, ^bb1, ^bb2(%0 : i32)

^bb1: // CHECK: ^bb1:
  // CHECK-NEXT: cf.br ^bb2(%[[VAR_c1_i32]] : i32)
  %1 = arith.constant 1 : i32
  cf.br ^bb2(%1 : i32)

^bb2(%arg : i32):
  return %arg : i32
}

// -----

/// Check that operation definitions are NOT propagated up the dominance tree.
// CHECK-LABEL: @up_propagate_for
func.func @up_propagate_for() -> i32 {
  // CHECK: affine.for {{.*}} = 0 to 4 {
  affine.for %i = 0 to 4 {
    // CHECK-NEXT: %[[VAR_c1_i32_0:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
    // CHECK-NEXT: "foo"(%[[VAR_c1_i32_0]]) : (i32) -> ()
    %0 = arith.constant 1 : i32
    "foo"(%0) : (i32) -> ()
  }

  // CHECK: %[[VAR_c1_i32:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
  // CHECK-NEXT: return %[[VAR_c1_i32]] : i32
  %1 = arith.constant 1 : i32
  return %1 : i32
}

// -----

// CHECK-LABEL: func @up_propagate
func.func @up_propagate() -> i32 {
  // CHECK-NEXT:  %[[VAR_c0_i32:[0-9a-zA-Z_]+]] = arith.constant 0 : i32
  %0 = arith.constant 0 : i32

  // CHECK-NEXT: %[[VAR_true:[0-9a-zA-Z_]+]] = arith.constant true
  %cond = arith.constant true

  // CHECK-NEXT: cf.cond_br %[[VAR_true]], ^bb1, ^bb2(%[[VAR_c0_i32]] : i32)
  cf.cond_br %cond, ^bb1, ^bb2(%0 : i32)

^bb1: // CHECK: ^bb1:
  // CHECK-NEXT: %[[VAR_c1_i32:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
  %1 = arith.constant 1 : i32

  // CHECK-NEXT: cf.br ^bb2(%[[VAR_c1_i32]] : i32)
  cf.br ^bb2(%1 : i32)

^bb2(%arg : i32): // CHECK: ^bb2
  // CHECK-NEXT: %[[VAR_c1_i32_0:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
  %2 = arith.constant 1 : i32

  // CHECK-NEXT: %[[VAR_1:[0-9a-zA-Z_]+]] = arith.addi %{{.*}}, %[[VAR_c1_i32_0]] : i32
  %add = arith.addi %arg, %2 : i32

  // CHECK-NEXT: return %[[VAR_1]] : i32
  return %add : i32
}

// -----

/// The same test as above except that we are testing on a cfg embedded within
/// an operation region.
// CHECK-LABEL: func @up_propagate_region
func.func @up_propagate_region() -> i32 {
  // CHECK-NEXT: {{.*}} "foo.region"
  %0 = "foo.region"() ({
    // CHECK-NEXT:  %[[VAR_c0_i32:[0-9a-zA-Z_]+]] = arith.constant 0 : i32
    // CHECK-NEXT: %[[VAR_true:[0-9a-zA-Z_]+]] = arith.constant true
    // CHECK-NEXT: cf.cond_br

    %1 = arith.constant 0 : i32
    %true = arith.constant true
    cf.cond_br %true, ^bb1, ^bb2(%1 : i32)

  ^bb1: // CHECK: ^bb1:
    // CHECK-NEXT: %[[VAR_c1_i32:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
    // CHECK-NEXT: cf.br

    %c1_i32 = arith.constant 1 : i32
    cf.br ^bb2(%c1_i32 : i32)

  ^bb2(%arg : i32): // CHECK: ^bb2(%[[VAR_1:.*]]: i32):
    // CHECK-NEXT: %[[VAR_c1_i32_0:[0-9a-zA-Z_]+]] = arith.constant 1 : i32
    // CHECK-NEXT: %[[VAR_2:[0-9a-zA-Z_]+]] = arith.addi %[[VAR_1]], %[[VAR_c1_i32_0]] : i32
    // CHECK-NEXT: "foo.yield"(%[[VAR_2]]) : (i32) -> ()

    %c1_i32_0 = arith.constant 1 : i32
    %2 = arith.addi %arg, %c1_i32_0 : i32
    "foo.yield" (%2) : (i32) -> ()
  }) : () -> (i32)
  return %0 : i32
}

// -----

/// This test checks that nested regions that are isolated from above are
/// properly handled.
// CHECK-LABEL: @nested_isolated
func.func @nested_isolated() -> i32 {
  // CHECK-NEXT: arith.constant 1
  %0 = arith.constant 1 : i32

  // CHECK-NEXT: builtin.module
  // CHECK-NEXT: @nested_func
  builtin.module {
    func.func @nested_func() {
      // CHECK-NEXT: arith.constant 1
      %foo = arith.constant 1 : i32
      "foo.yield"(%foo) : (i32) -> ()
    }
  }

  // CHECK: "foo.region"
  "foo.region"() ({
    // CHECK-NEXT: arith.constant 1
    %foo = arith.constant 1 : i32
    "foo.yield"(%foo) : (i32) -> ()
  }) : () -> ()

  return %0 : i32
}

// -----

/// This test is checking that CSE gracefully handles values in graph regions
/// where the use occurs before the def, and one of the defs could be CSE'd with
/// the other.
// CHECK-LABEL: @use_before_def
func.func @use_before_def() {
  // CHECK-NEXT: test.graph_region
  test.graph_region {
    // CHECK-NEXT: arith.addi
    %0 = arith.addi %1, %2 : i32

    // CHECK-NEXT: arith.constant 1
    // CHECK-NEXT: arith.constant 1
    %1 = arith.constant 1 : i32
    %2 = arith.constant 1 : i32

    // CHECK-NEXT: "foo.yield"(%{{.*}}) : (i32) -> ()
    "foo.yield"(%0) : (i32) -> ()
  }
  return
}

// -----

/// This test is checking that CSE is removing duplicated read op that follow
/// other.
// CHECK-LABEL: @remove_direct_duplicated_read_op
func.func @remove_direct_duplicated_read_op() -> i32 {
  // CHECK-NEXT: %[[READ_VALUE:.*]] = "test.op_with_memread"() : () -> i32
  %0 = "test.op_with_memread"() : () -> (i32)
  %1 = "test.op_with_memread"() : () -> (i32)
  // CHECK-NEXT: %{{.*}} = arith.addi %[[READ_VALUE]], %[[READ_VALUE]] : i32
  %2 = arith.addi %0, %1 : i32
  return %2 : i32
}

// -----

/// This test is checking that CSE is removing duplicated read op that follow
/// other.
// CHECK-LABEL: @remove_multiple_duplicated_read_op
func.func @remove_multiple_duplicated_read_op() -> i64 {
  // CHECK: %[[READ_VALUE:.*]] = "test.op_with_memread"() : () -> i64
  %0 = "test.op_with_memread"() : () -> (i64)
  %1 = "test.op_with_memread"() : () -> (i64)
  // CHECK-NEXT: %{{.*}} = arith.addi %{{.*}}, %[[READ_VALUE]] : i64
  %2 = arith.addi %0, %1 : i64
  %3 = "test.op_with_memread"() : () -> (i64)
  // CHECK-NEXT: %{{.*}} = arith.addi %{{.*}}, %{{.*}} : i64
  %4 = arith.addi %2, %3 : i64
  %5 = "test.op_with_memread"() : () -> (i64)
  // CHECK-NEXT: %{{.*}} = arith.addi %{{.*}}, %{{.*}} : i64
  %6 = arith.addi %4, %5 : i64
  // CHECK-NEXT: return %{{.*}} : i64
  return %6 : i64
}

// -----

/// This test is checking that CSE is not removing duplicated read op that
/// have write op in between.
// CHECK-LABEL: @dont_remove_duplicated_read_op_with_sideeffecting
func.func @dont_remove_duplicated_read_op_with_sideeffecting() -> i32 {
  // CHECK-NEXT: %[[READ_VALUE0:.*]] = "test.op_with_memread"() : () -> i32
  %0 = "test.op_with_memread"() : () -> (i32)
  "test.op_with_memwrite"() : () -> ()
  // CHECK: %[[READ_VALUE1:.*]] = "test.op_with_memread"() : () -> i32
  %1 = "test.op_with_memread"() : () -> (i32)
  // CHECK-NEXT: %{{.*}} = arith.addi %[[READ_VALUE0]], %[[READ_VALUE1]] : i32
  %2 = arith.addi %0, %1 : i32
  return %2 : i32
}

// -----

// Check that an operation with a single region can CSE.
func.func @cse_single_block_ops(%a : tensor<?x?xf32>, %b : tensor<?x?xf32>)
  -> (tensor<?x?xf32>, tensor<?x?xf32>) {
  %0 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32):
    test.region_yield %arg0 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  %1 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32):
    test.region_yield %arg0 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  return %0, %1 : tensor<?x?xf32>, tensor<?x?xf32>
}
// CHECK-LABEL: func @cse_single_block_ops
//       CHECK:   %[[OP:.+]] = test.cse_of_single_block_op
//   CHECK-NOT:   test.cse_of_single_block_op
//       CHECK:   return %[[OP]], %[[OP]]

// -----

// Operations with different number of bbArgs dont CSE.
func.func @no_cse_varied_bbargs(%a : tensor<?x?xf32>, %b : tensor<?x?xf32>)
  -> (tensor<?x?xf32>, tensor<?x?xf32>) {
  %0 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    test.region_yield %arg0 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  %1 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32):
    test.region_yield %arg0 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  return %0, %1 : tensor<?x?xf32>, tensor<?x?xf32>
}
// CHECK-LABEL: func @no_cse_varied_bbargs
//       CHECK:   %[[OP0:.+]] = test.cse_of_single_block_op
//       CHECK:   %[[OP1:.+]] = test.cse_of_single_block_op
//       CHECK:   return %[[OP0]], %[[OP1]]

// -----

// Operations with different regions dont CSE
func.func @no_cse_region_difference_simple(%a : tensor<?x?xf32>, %b : tensor<?x?xf32>)
  -> (tensor<?x?xf32>, tensor<?x?xf32>) {
  %0 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    test.region_yield %arg0 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  %1 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    test.region_yield %arg1 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  return %0, %1 : tensor<?x?xf32>, tensor<?x?xf32>
}
// CHECK-LABEL: func @no_cse_region_difference_simple
//       CHECK:   %[[OP0:.+]] = test.cse_of_single_block_op
//       CHECK:   %[[OP1:.+]] = test.cse_of_single_block_op
//       CHECK:   return %[[OP0]], %[[OP1]]

// -----

// Operation with identical region with multiple statements CSE.
func.func @cse_single_block_ops_identical_bodies(%a : tensor<?x?xf32>, %b : tensor<?x?xf32>, %c : f32, %d : i1)
  -> (tensor<?x?xf32>, tensor<?x?xf32>) {
  %0 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    %1 = arith.divf %arg0, %arg1 : f32
    %2 = arith.remf %arg0, %c : f32
    %3 = arith.select %d, %1, %2 : f32
    test.region_yield %3 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  %1 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    %1 = arith.divf %arg0, %arg1 : f32
    %2 = arith.remf %arg0, %c : f32
    %3 = arith.select %d, %1, %2 : f32
    test.region_yield %3 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  return %0, %1 : tensor<?x?xf32>, tensor<?x?xf32>
}
// CHECK-LABEL: func @cse_single_block_ops_identical_bodies
//       CHECK:   %[[OP:.+]] = test.cse_of_single_block_op
//   CHECK-NOT:   test.cse_of_single_block_op
//       CHECK:   return %[[OP]], %[[OP]]

// -----

// Operation with non-identical regions dont CSE.
func.func @no_cse_single_block_ops_different_bodies(%a : tensor<?x?xf32>, %b : tensor<?x?xf32>, %c : f32, %d : i1)
  -> (tensor<?x?xf32>, tensor<?x?xf32>) {
  %0 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    %1 = arith.divf %arg0, %arg1 : f32
    %2 = arith.remf %arg0, %c : f32
    %3 = arith.select %d, %1, %2 : f32
    test.region_yield %3 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  %1 = test.cse_of_single_block_op inputs(%a, %b) {
    ^bb0(%arg0 : f32, %arg1 : f32):
    %1 = arith.divf %arg0, %arg1 : f32
    %2 = arith.remf %arg0, %c : f32
    %3 = arith.select %d, %2, %1 : f32
    test.region_yield %3 : f32
  } : tensor<?x?xf32>, tensor<?x?xf32> -> tensor<?x?xf32>
  return %0, %1 : tensor<?x?xf32>, tensor<?x?xf32>
}
// CHECK-LABEL: func @no_cse_single_block_ops_different_bodies
//       CHECK:   %[[OP0:.+]] = test.cse_of_single_block_op
//       CHECK:   %[[OP1:.+]] = test.cse_of_single_block_op
//       CHECK:   return %[[OP0]], %[[OP1]]

// -----

func.func @failing_issue_59135(%arg0: tensor<2x2xi1>, %arg1: f32, %arg2 : tensor<2xi1>) -> (tensor<2xi1>, tensor<2xi1>) {
  %false_2 = arith.constant false
  %true_5 = arith.constant true
  %9 = test.cse_of_single_block_op inputs(%arg2) {
  ^bb0(%out: i1):
    %true_144 = arith.constant true
    test.region_yield %true_144 : i1
  } : tensor<2xi1> -> tensor<2xi1>
  %15 = test.cse_of_single_block_op inputs(%arg2) {
  ^bb0(%out: i1):
    %true_144 = arith.constant true
    test.region_yield %true_144 : i1
  } : tensor<2xi1> -> tensor<2xi1>
  %93 = arith.maxsi %false_2, %true_5 : i1
  return %9, %15 : tensor<2xi1>, tensor<2xi1>
}
// CHECK-LABEL: func @failing_issue_59135
//       CHECK:   %[[TRUE:.+]] = arith.constant true
//       CHECK:   %[[OP:.+]] = test.cse_of_single_block_op
//       CHECK:     test.region_yield %[[TRUE]]
//       CHECK:   return %[[OP]], %[[OP]]

// -----

func.func @cse_multiple_regions(%c: i1, %t: tensor<5xf32>) -> (tensor<5xf32>, tensor<5xf32>) {
  %r1 = scf.if %c -> (tensor<5xf32>) {
    %0 = tensor.empty() : tensor<5xf32>
    scf.yield %0 : tensor<5xf32>
  } else {
    scf.yield %t : tensor<5xf32>
  }
  %r2 = scf.if %c -> (tensor<5xf32>) {
    %0 = tensor.empty() : tensor<5xf32>
    scf.yield %0 : tensor<5xf32>
  } else {
    scf.yield %t : tensor<5xf32>
  }
  return %r1, %r2 : tensor<5xf32>, tensor<5xf32>
}
// CHECK-LABEL: func @cse_multiple_regions
//       CHECK:   %[[if:.*]] = scf.if {{.*}} {
//       CHECK:     tensor.empty
//       CHECK:     scf.yield
//       CHECK:   } else {
//       CHECK:     scf.yield
//       CHECK:   }
//   CHECK-NOT:   scf.if
//       CHECK:   return %[[if]], %[[if]]

// -----

// CHECK-LABEL: @cse_recursive_effects_success
func.func @cse_recursive_effects_success() -> (i32, i32, i32) {
  // CHECK-NEXT: %[[READ_VALUE:.*]] = "test.op_with_memread"() : () -> i32
  %0 = "test.op_with_memread"() : () -> (i32)

  // do something with recursive effects, containing no side effects
  %true = arith.constant true
  // CHECK-NEXT: %[[TRUE:.+]] = arith.constant true
  // CHECK-NEXT: %[[IF:.+]] = scf.if %[[TRUE]] -> (i32) {
  %1 = scf.if %true -> (i32) {
    %c42 = arith.constant 42 : i32
    scf.yield %c42 : i32
    // CHECK-NEXT: %[[C42:.+]] = arith.constant 42 : i32
    // CHECK-NEXT: scf.yield %[[C42]]
    // CHECK-NEXT: } else {
  } else {
    %c24 = arith.constant 24 : i32
    scf.yield %c24 : i32
    // CHECK-NEXT: %[[C24:.+]] = arith.constant 24 : i32
    // CHECK-NEXT: scf.yield %[[C24]]
    // CHECK-NEXT: }
  }

  // %2 can be removed
  // CHECK-NEXT: return %[[READ_VALUE]], %[[READ_VALUE]], %[[IF]] : i32, i32, i32
  %2 = "test.op_with_memread"() : () -> (i32)
  return %0, %2, %1 : i32, i32, i32
}

// -----

// CHECK-LABEL: @cse_recursive_effects_failure
func.func @cse_recursive_effects_failure() -> (i32, i32, i32) {
  // CHECK-NEXT: %[[READ_VALUE:.*]] = "test.op_with_memread"() : () -> i32
  %0 = "test.op_with_memread"() : () -> (i32)

  // do something with recursive effects, containing a write effect
  %true = arith.constant true
  // CHECK-NEXT: %[[TRUE:.+]] = arith.constant true
  // CHECK-NEXT: %[[IF:.+]] = scf.if %[[TRUE]] -> (i32) {
  %1 = scf.if %true -> (i32) {
    "test.op_with_memwrite"() : () -> ()
    // CHECK-NEXT: "test.op_with_memwrite"() : () -> ()
    %c42 = arith.constant 42 : i32
    scf.yield %c42 : i32
    // CHECK-NEXT: %[[C42:.+]] = arith.constant 42 : i32
    // CHECK-NEXT: scf.yield %[[C42]]
    // CHECK-NEXT: } else {
  } else {
    %c24 = arith.constant 24 : i32
    scf.yield %c24 : i32
    // CHECK-NEXT: %[[C24:.+]] = arith.constant 24 : i32
    // CHECK-NEXT: scf.yield %[[C24]]
    // CHECK-NEXT: }
  }

  // %2 can not be be removed because of the write
  // CHECK-NEXT: %[[READ_VALUE2:.*]] = "test.op_with_memread"() : () -> i32
  // CHECK-NEXT: return %[[READ_VALUE]], %[[READ_VALUE2]], %[[IF]] : i32, i32, i32
  %2 = "test.op_with_memread"() : () -> (i32)
  return %0, %2, %1 : i32, i32, i32
}
*/
