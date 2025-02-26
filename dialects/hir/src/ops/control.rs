use alloc::{boxed::Box, rc::Rc, vec::Vec};

use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

/// Returns from the enclosing function with the provided operands as its results.
#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}

/// Returns from the enclosing function with the provided immediate value as its result.
#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike)
)]
pub struct RetImm {
    #[attr]
    value: Immediate,
}

/// An unstructured control flow primitive representing an unconditional branch to `target`
#[operation(
    dialect = HirDialect,
    traits(Terminator),
    implements(BranchOpInterface)
)]
pub struct Br {
    #[successor]
    target: Successor,
}

impl Canonicalizable for Br {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites
            .push(crate::canonicalization::SimplifyBrToBlockWithSinglePred::new(context.clone()));
        rewrites.push(crate::canonicalization::SimplifyPassthroughBr::new(context));
    }
}

impl BranchOpInterface for Br {
    #[inline]
    fn get_successor_for_operands(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        Some(self.successors()[0])
    }
}

/// An unstructured control flow primitive representing a conditional branch to either `then_dest`
/// or `else_dest` depending on the value of `condition`, a boolean value.
#[operation(
    dialect = HirDialect,
    traits(Terminator),
    implements(BranchOpInterface)
)]
pub struct CondBr {
    #[operand]
    condition: Bool,
    #[successor]
    then_dest: Successor,
    #[successor]
    else_dest: Successor,
}

impl Canonicalizable for CondBr {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::SimplifyPassthroughCondBr::new(context));
    }
}

impl BranchOpInterface for CondBr {
    fn get_successor_for_operands(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        let value = operands[0].as_deref()?;
        let cond = value.as_bool().unwrap_or_else(|| {
            panic!("expected boolean immediate for '{}' condition, got: {:?}", self.name(), value)
        });

        Some(if cond {
            self.successors()[0]
        } else {
            self.successors()[1]
        })
    }
}

/// An unstructured control flow primitive that represents a multi-way branch to one of multiple
/// branch targets, depending on the value of `selector`.
///
/// If a specific selector value is matched by `cases`, the branch target corresponding to that
/// case is the one to which control is transferred. If no matching case is found for the selector,
/// then the `fallback` target is used instead.
///
/// A `fallback` successor must always be provided.
#[operation(
    dialect = HirDialect,
    traits(Terminator),
    implements(BranchOpInterface)
)]
pub struct Switch {
    #[operand]
    selector: UInt32,
    #[successors(keyed)]
    cases: SwitchCase,
    #[successor]
    fallback: Successor,
}

impl BranchOpInterface for Switch {
    #[inline]
    fn get_successor_for_operands(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        let value = operands[0].as_deref()?;
        let selector = if let Some(selector) = value.downcast_ref::<Immediate>() {
            selector.as_u32().expect("invalid selector value for 'hir.switch'")
        } else if let Some(selector) = value.downcast_ref::<u32>() {
            *selector
        } else if let Some(selector) = value.downcast_ref::<i32>() {
            u32::try_from(*selector).expect("invalid selector value for 'hir.switch'")
        } else if let Some(selector) = value.downcast_ref::<usize>() {
            u32::try_from(*selector).expect("invalid selector value for 'hir.switch': out of range")
        } else {
            panic!("unsupported selector value type for '{}', got: {:?}", self.name(), value)
        };

        for switch_case in self.cases().iter() {
            let key = *switch_case.key().unwrap();
            if selector == key {
                return Some(*switch_case.info());
            }
        }

        // If we reach here, no selector match was found, so use the fallback successor
        Some(self.successors().all().as_slice().last().copied().unwrap())
    }
}

/// Represents a single branch target by matching a specific selector value in a [Switch]
/// operation.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: BlockRef,
    pub arguments: Vec<ValueRef>,
}

#[doc(hidden)]
pub struct SwitchCaseRef<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRange<'a>,
}

#[doc(hidden)]
pub struct SwitchCaseMut<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRangeMut<'a>,
}

impl KeyedSuccessor for SwitchCase {
    type Key = u32;
    type Repr<'a> = SwitchCaseRef<'a>;
    type ReprMut<'a> = SwitchCaseMut<'a>;

    fn key(&self) -> &Self::Key {
        &self.value
    }

    fn into_parts(self) -> (Self::Key, BlockRef, Vec<ValueRef>) {
        (self.value, self.successor, self.arguments)
    }

    fn into_repr(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRange<'_>,
    ) -> Self::Repr<'_> {
        SwitchCaseRef {
            value: key,
            successor: block,
            arguments: operands,
        }
    }

    fn into_repr_mut(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRangeMut<'_>,
    ) -> Self::ReprMut<'_> {
        SwitchCaseMut {
            value: key,
            successor: block,
            arguments: operands,
        }
    }
}

/// [If] is a structured control flow operation representing conditional execution.
///
/// An [If] takes a single condition as an argument, which chooses between one of its two regions
/// based on the condition. If the condition is true, then the `then_body` region is executed,
/// otherwise `else_body`.
///
/// Neither region allows any arguments, and both regions must be terminated with one of:
///
/// * [Return] to return from the enclosing function directly
/// * [Unreachable] to abort execution
/// * [Yield] to return from the enclosing [If]
#[operation(
    dialect = HirDialect,
    traits(SingleBlock, NoRegionArguments, HasRecursiveMemoryEffects),
    implements(RegionBranchOpInterface)
)]
pub struct If {
    #[operand]
    condition: Bool,
    #[region]
    then_body: Region,
    #[region]
    else_body: Region,
}

impl If {
    pub fn then_yield(&self) -> UnsafeIntrusiveEntityRef<Yield> {
        let terminator = self.then_body().entry().terminator().unwrap();
        let term = terminator
            .borrow()
            .downcast_ref::<Yield>()
            .expect("invalid hir.if then terminator: expected yield")
            as *const Yield;
        unsafe { UnsafeIntrusiveEntityRef::from_raw(term) }
    }

    pub fn else_yield(&self) -> UnsafeIntrusiveEntityRef<Yield> {
        let terminator = self.else_body().entry().terminator().unwrap();
        let term = terminator
            .borrow()
            .downcast_ref::<Yield>()
            .expect("invalid hir.if else terminator: expected yield")
            as *const Yield;
        unsafe { UnsafeIntrusiveEntityRef::from_raw(term) }
    }
}

impl Canonicalizable for If {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::ConvertTrivialIfToSelect::new(context.clone()));
        rewrites.push(crate::canonicalization::IfRemoveUnusedResults::new(context));
    }
}

impl RegionBranchOpInterface for If {
    fn get_entry_successor_regions(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> RegionSuccessorIter<'_> {
        let condition = operands[0].as_deref().and_then(|v| v.as_bool());
        let has_then = condition.is_none_or(|v| v);
        let else_possible = condition.is_none_or(|v| !v);
        let has_else = else_possible && !self.else_body().is_empty();

        let mut infos = SmallVec::<[RegionSuccessorInfo; 2]>::default();
        if has_then {
            infos.push(RegionSuccessorInfo::Entering(self.then_body().as_region_ref()));
        }

        if else_possible {
            if has_else {
                infos.push(RegionSuccessorInfo::Entering(self.else_body().as_region_ref()));
            } else {
                // Branching back to parent with `then` results
                infos.push(RegionSuccessorInfo::Returning(
                    self.results().all().iter().map(|v| v.borrow().as_value_ref()).collect(),
                ));
            }
        }

        RegionSuccessorIter::new(self.as_operation(), infos)
    }

    fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
        match point {
            RegionBranchPoint::Parent => {
                // Either branch is reachable on entry (unless `else` is empty, as it is optional)
                let mut infos: SmallVec<[_; 2]> =
                    smallvec![RegionSuccessorInfo::Entering(self.then_body().as_region_ref())];
                // Don't consider the else region if it is empty
                if !self.else_body().is_empty() {
                    infos.push(RegionSuccessorInfo::Entering(self.else_body().as_region_ref()));
                }
                RegionSuccessorIter::new(self.as_operation(), infos)
            }
            RegionBranchPoint::Child(_) => {
                // Only the parent If is reachable from then_body/else_body
                RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Returning(
                        self.results().all().iter().map(|v| v.borrow().as_value_ref()).collect(),
                    )],
                )
            }
        }
    }

    fn get_region_invocation_bounds(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        let condition = operands[0].as_deref().and_then(|v| v.as_bool());

        if let Some(condition) = condition {
            if condition {
                smallvec![InvocationBounds::Exact(1), InvocationBounds::Never]
            } else {
                smallvec![InvocationBounds::Never, InvocationBounds::Exact(1)]
            }
        } else {
            // Only one region is invoked, and no more than a single time
            smallvec![InvocationBounds::NoMoreThan(1); 2]
        }
    }

    #[inline(always)]
    fn is_repetitive_region(&self, _index: usize) -> bool {
        false
    }

    #[inline(always)]
    fn has_loop(&self) -> bool {
        false
    }
}

/// A while is a loop structure composed of two regions: a "before" region, and an "after" region.
///
/// The "before" region's entry block parameters correspond to the operands expected by the
/// operation, and can be used to compute the condition that determines whether the "after" body
/// is executed or not, or simply forwarded to the "after" region. The "before" region must
/// terminate with a [Condition] operation, which will be evaluated to determine whether or not
/// to continue the loop.
///
/// The "after" region corresponds to the loop body, and must terminate with a [Yield] operation,
/// whose operands must be of the same arity and type as the "before" region's argument list. In
/// this way, the "after" body can feed back input to the "before" body to determine whether to
/// continue the loop.
#[operation(
    dialect = HirDialect,
    traits(SingleBlock, HasRecursiveMemoryEffects),
    implements(RegionBranchOpInterface, LoopLikeOpInterface)
)]
pub struct While {
    #[operands]
    inits: AnyType,
    #[region]
    before: Region,
    #[region]
    after: Region,
}

impl While {
    pub fn condition_op(&self) -> UnsafeIntrusiveEntityRef<Condition> {
        let term = self
            .before()
            .entry()
            .terminator()
            .expect("expected before region to have a terminator");
        let cond = term
            .borrow()
            .downcast_ref::<Condition>()
            .expect("expected before region to terminate with hir.condition")
            as *const Condition;
        unsafe { UnsafeIntrusiveEntityRef::from_raw(cond) }
    }

    pub fn yield_op(&self) -> UnsafeIntrusiveEntityRef<Yield> {
        let term = self
            .after()
            .entry()
            .terminator()
            .expect("expected after region to have a terminator");
        let yield_op = term
            .borrow()
            .downcast_ref::<Yield>()
            .expect("expected after region to terminate with hir.yield")
            as *const Yield;
        unsafe { UnsafeIntrusiveEntityRef::from_raw(yield_op) }
    }
}

impl Canonicalizable for While {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::RemoveLoopInvariantArgsFromBeforeBlock::new(
            context.clone(),
        ));
        //rewrites.push(crate::canonicalization::RemoveLoopInvariantValueYielded::new(context.clone()));
        rewrites.push(crate::canonicalization::WhileConditionTruth::new(context.clone()));
        rewrites.push(crate::canonicalization::WhileUnusedResult::new(context.clone()));
        rewrites.push(crate::canonicalization::WhileRemoveDuplicatedResults::new(context.clone()));
        rewrites.push(crate::canonicalization::WhileRemoveUnusedArgs::new(context.clone()));
        //rewrites.push(crate::canonicalization::ConvertDoWhileToWhileTrue::new(context));
    }
}

impl LoopLikeOpInterface for While {
    fn get_region_iter_args(&self) -> Option<EntityRef<'_, [BlockArgumentRef]>> {
        let entry = self.before().entry_block_ref()?;
        Some(EntityRef::map(entry.borrow(), |block| block.arguments()))
    }

    fn get_loop_header_region(&self) -> RegionRef {
        self.before().as_region_ref()
    }

    fn get_loop_regions(&self) -> SmallVec<[RegionRef; 2]> {
        smallvec![self.before().as_region_ref(), self.after().as_region_ref()]
    }

    fn get_inits_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.inits_mut()
    }

    fn get_yielded_values_mut(&mut self) -> Option<EntityProjectionMut<'_, OpOperandRangeMut<'_>>> {
        let mut yield_op = self
            .after()
            .entry()
            .terminator()
            .expect("invalid `while`: expected loop body to be terminated");

        // The values which are yielded to each iteration
        Some(EntityMut::project(yield_op.borrow_mut(), |op| op.operands_mut().group_mut(0)))
    }
}

impl RegionBranchOpInterface for While {
    #[inline]
    fn get_entry_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        // Operands being forwarded to the `before` region from outside the loop
        SuccessorOperandRange::forward(self.operands().all())
    }

    fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
        match point {
            RegionBranchPoint::Parent => {
                // The only successor region when branching from outside the While op is the
                // `before` region.
                RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Entering(self.before().as_region_ref())],
                )
            }
            RegionBranchPoint::Child(region) => {
                let before_region = self.before().as_region_ref();
                let after_region = self.after().as_region_ref();
                assert!(region == before_region || region == after_region);

                // When branching from `before`, the only successor is `after` or the While itself,
                // otherwise, when branching from `after` the only successor is `before`.
                if region == after_region {
                    RegionSuccessorIter::new(
                        self.as_operation(),
                        [RegionSuccessorInfo::Entering(before_region)],
                    )
                } else {
                    RegionSuccessorIter::new(
                        self.as_operation(),
                        [
                            RegionSuccessorInfo::Returning(
                                self.results()
                                    .all()
                                    .iter()
                                    .map(|r| r.borrow().as_value_ref())
                                    .collect(),
                            ),
                            RegionSuccessorInfo::Entering(after_region),
                        ],
                    )
                }
            }
        }
    }

    #[inline]
    fn get_region_invocation_bounds(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        smallvec![InvocationBounds::Unknown; self.num_regions()]
    }

    #[inline(always)]
    fn is_repetitive_region(&self, _index: usize) -> bool {
        // Both regions are in the loop (`before` -> `after` -> `before` -> `after`)
        true
    }

    #[inline(always)]
    fn has_loop(&self) -> bool {
        true
    }
}

/// The `hir.index_switch` is a control-flow operation that branches to one of the given regions
/// based on the values of the argument and the cases. The argument is always of type `u32`.
///
/// The operation always has a "default" region and any number of case regions denoted by integer
/// constants. Control-flow transfers to the case region whose constant value equals the value of
/// the argument. If the argument does not equal any of the case values, control-flow transfer to
/// the "default" region.
///
/// ## Example
///
/// ```text,ignore
/// %0 = hir.index_switch %arg0 : u32 -> i32
/// case 2 {
///   %1 = hir.constant 10 : i32
///   scf.yield %1 : i32
/// }
/// case 5 {
///   %2 = hir.constant 20 : i32
///   scf.yield %2 : i32
/// }
/// default {
///   %3 = hir.constant 30 : i32
///   scf.yield %3 : i32
/// }
/// ```
#[operation(
    dialect = HirDialect,
    traits(SingleBlock, HasRecursiveMemoryEffects),
    implements(RegionBranchOpInterface)
)]
pub struct IndexSwitch {
    #[operand]
    selector: UInt32,
    #[attr]
    cases: ArrayAttr<u32>,
    #[region]
    default_region: Region,
}

impl IndexSwitch {
    pub fn num_cases(&self) -> usize {
        self.cases().len()
    }

    pub fn get_default_block(&self) -> BlockRef {
        self.default_region().entry_block_ref().expect("default region has no blocks")
    }

    pub fn get_case_index_for_selector(&self, selector: u32) -> Option<usize> {
        self.cases().iter().position(|case| *case == selector)
    }

    #[track_caller]
    pub fn get_case_block(&self, index: usize) -> BlockRef {
        let block_ref = self.get_case_region(index).borrow().entry_block_ref();
        match block_ref {
            None => panic!("region for case {index} has no blocks"),
            Some(block) => block,
        }
    }

    #[track_caller]
    pub fn get_case_region(&self, mut index: usize) -> RegionRef {
        let mut next_region = self.regions().front().as_pointer();
        let mut current_index = 0;
        // Shift the requested index up by one to account for default region
        index += 1;
        while let Some(region) = next_region.take() {
            if index == current_index {
                return region;
            }
            next_region = region.next();
            current_index += 1;
        }

        panic!("invalid region index `{}`: out of bounds", index - 1)
    }
}

impl RegionBranchOpInterface for IndexSwitch {
    fn get_entry_successor_regions(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> RegionSuccessorIter<'_> {
        let selector = operands[0].as_deref().and_then(|v| v.as_u32());
        let selected = selector.map(|s| self.get_case_index_for_selector(s));

        match selected {
            None => {
                // All regions are possible successors
                let infos =
                    self.regions().iter().map(|r| RegionSuccessorInfo::Entering(r.as_region_ref()));
                RegionSuccessorIter::new(self.as_operation(), infos)
            }
            Some(Some(selected)) => {
                // A specific case was selected
                RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Entering(self.get_case_region(selected))],
                )
            }
            Some(None) => {
                // The fallback case should be used
                RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Entering(self.default_region().as_region_ref())],
                )
            }
        }
    }

    fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
        match point {
            RegionBranchPoint::Parent => {
                // Any region is reachable on entry
                let infos =
                    self.regions().iter().map(|r| RegionSuccessorInfo::Entering(r.as_region_ref()));
                RegionSuccessorIter::new(self.as_operation(), infos)
            }
            RegionBranchPoint::Child(_) => {
                // Only the parent op is reachable from its regions
                RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Returning(
                        self.results().all().iter().map(|v| v.borrow().as_value_ref()).collect(),
                    )],
                )
            }
        }
    }

    fn get_region_invocation_bounds(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        let selector = operands[0].as_deref().and_then(|v| v.as_u32());

        if let Some(selector) = selector {
            let mut bounds = smallvec![InvocationBounds::Never; self.num_cases()];
            let selected =
                self.get_case_index_for_selector(selector).map(|idx| idx + 1).unwrap_or(0);
            bounds[selected] = InvocationBounds::Exact(1);
            bounds
        } else {
            // Only one region is invoked, and no more than a single time
            smallvec![InvocationBounds::NoMoreThan(1); self.num_cases()]
        }
    }

    #[inline(always)]
    fn is_repetitive_region(&self, _index: usize) -> bool {
        false
    }

    #[inline(always)]
    fn has_loop(&self) -> bool {
        false
    }
}

impl Canonicalizable for IndexSwitch {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::FoldConstantIndexSwitch::new(context));
    }
}

/// The [Condition] op is used in conjunction with [While] as the terminator of its `before` region.
///
/// This op represents a choice between continuing the loop, or exiting the [While] loop and
/// continuing execution after the loop.
///
/// NOTE: Attempting to use this op in any other context than the one described above is invalid,
/// and the implementation of various interfaces by this op will panic if that assumption is
/// violated.
#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike),
    implements(RegionBranchTerminatorOpInterface)
)]
pub struct Condition {
    #[operand]
    condition: Bool,
    #[operands]
    forwarded: AnyType,
}

impl RegionBranchTerminatorOpInterface for Condition {
    #[inline]
    fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        SuccessorOperandRange::forward(self.forwarded())
    }

    #[inline]
    fn get_mutable_successor_operands(
        &mut self,
        _point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_> {
        SuccessorOperandRangeMut::forward(self.forwarded_mut())
    }

    fn get_successor_regions(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        // A [While] loop has two regions: `before` (containing this op), and `after`, which this
        // op branches to when the condition is true. If the condition is false, control is
        // transferred back to the parent [While] operation, with the forwarded operands of the
        // condition used as the results of the [While] operation.
        //
        // We can return a single statically-known region if we were given a constant condition
        // value, otherwise we must return both possible regions.
        let cond = operands[0].as_deref().and_then(|v| v.as_bool());
        let mut regions = SmallVec::<[RegionSuccessorInfo; 2]>::default();

        let parent_op = self.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        let while_op = parent_op
            .downcast_ref::<While>()
            .expect("expected `Condition` op to be a child of a `While` op");
        let after_region = while_op.after().as_region_ref();

        // We can't know the condition until runtime, so both the parent `while` op and
        if cond.is_none_or(|v| v) {
            regions.push(RegionSuccessorInfo::Entering(after_region));
        }
        if cond.is_none_or(|v| !v) {
            regions.push(RegionSuccessorInfo::Returning(
                while_op.results().all().iter().map(|r| r.borrow().as_value_ref()).collect(),
            ));
        }

        regions
    }
}

/// The [Yield] op is used in conjunction with [If] and [While] ops as a return-like terminator.
///
/// * With [If], its regions must be terminated with either a [Yield] or an [Unreachable] op.
/// * With [While], a [Yield] is only valid in the `after` region, and the yielded operands must
///   match the region arguments of the `before` region. Thus to return values from the body of a
///   loop, one must first yield them from the `after` region to the `before` region using [Yield],
///   and then yield them from the `before` region by passsing them as forwarded operands of the
///   [Condition] op.
///
/// Any number of operands can be yielded at the same time. However, when [Yield] is used in
/// conjunction with [While], the arity and type of the operands must match the region arguments
/// of the `before` region. When used in conjunction with [If], both the `if_true` and `if_false`
/// regions must yield the same arity and types.
#[operation(
    dialect = HirDialect,
    traits(Terminator, ReturnLike),
    implements(RegionBranchTerminatorOpInterface)
)]
pub struct Yield {
    #[operands]
    yielded: AnyType,
}

impl RegionBranchTerminatorOpInterface for Yield {
    #[inline]
    fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        SuccessorOperandRange::forward(self.yielded())
    }

    fn get_mutable_successor_operands(
        &mut self,
        _point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_> {
        SuccessorOperandRangeMut::forward(self.yielded_mut())
    }

    fn get_successor_regions(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        // Depending on the type of operation containing this yield, the set of successor regions
        // is always known.
        //
        // * [While] may only have a yield to its `before` region
        // * [If] may only yield to its parent
        let parent_op = self.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        if parent_op.is::<If>() {
            smallvec![RegionSuccessorInfo::Returning(
                parent_op.results().all().iter().map(|v| v.borrow().as_value_ref()).collect()
            )]
        } else if let Some(while_op) = parent_op.downcast_ref::<While>() {
            let before_region = while_op.before().as_region_ref();
            smallvec![RegionSuccessorInfo::Entering(before_region)]
        } else {
            panic!("unsupported parent operation for '{}': '{}'", self.name(), parent_op.name())
        }
    }
}
