use midenc_hir_macros::operation;
use midenc_session::diagnostics::Severity;
use smallvec::{smallvec, SmallVec};

use crate::{dialects::hir::HirDialect, traits::*, *};

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

impl BranchOpInterface for Br {
    #[inline]
    fn get_successor_for_operands(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<BlockRef> {
        Some(self.target().dest.borrow().block.clone())
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
impl BranchOpInterface for CondBr {
    fn get_successor_for_operands(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<BlockRef> {
        let value = operands[0].as_deref()?;
        let cond = if let Some(imm) = value.downcast_ref::<Immediate>() {
            imm.as_bool().expect("invalid boolean condition for 'hir.if'")
        } else if let Some(yes) = value.downcast_ref::<bool>() {
            *yes
        } else {
            panic!("expected boolean immediate for '{}' condition, got: {:?}", self.name(), value)
        };

        Some(if cond {
            self.then_dest().dest.borrow().block.clone()
        } else {
            self.else_dest().dest.borrow().block.clone()
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
    ) -> Option<BlockRef> {
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
            let key = switch_case.key().unwrap();
            if selector == key.value {
                return Some(switch_case.block());
            }
        }

        // If we reach here, no selector match was found, so use the fallback successor
        Some(self.fallback().dest.borrow().block.clone())
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

    fn into_parts(self) -> (Self::Key, BlockRef, Vec<ir::ValueRef>) {
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
    traits(SingleBlock, NoRegionArguments),
    implements(RegionBranchOpInterface, InferTypeOpInterface)
)]
pub struct If {
    #[operand]
    condition: Bool,
    #[region]
    then_body: Region,
    #[region]
    else_body: Region,
}

impl InferTypeOpInterface for If {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let then_region = self.then_body();
        if then_region.is_empty() {
            return Err(context
                .session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid `if` operation")
                .with_primary_label(self.span(), "empty `then` body, unable to infer return types")
                .into_report());
        }

        let then_block = then_region.entry();
        if then_block.body().is_empty() {
            return Err(context
                .session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid `if` operation")
                .with_primary_label(self.span(), "empty `then` body, unable to infer return types")
                .into_report());
        }

        if let Some(terminator) = then_block.terminator() {
            drop(then_block);
            drop(then_region);
            let terminator = terminator.borrow();
            if let Some(yield_op) = terminator.downcast_ref::<Yield>() {
                let types = yield_op
                    .yielded()
                    .iter()
                    .map(|operand| operand.borrow().ty())
                    .collect::<SmallVec<[_; 2]>>();

                let span = self.span();
                let owner = self.as_operation().as_operation_ref();
                self.results_mut().extend(types.into_iter().enumerate().map(|(index, ty)| {
                    context.make_result(
                        span,
                        ty,
                        owner.clone(),
                        index.try_into().expect("too many results"),
                    )
                }));

                Ok(())
            } else {
                Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid `if` operation")
                    .with_primary_label(terminator.span(), "expected `yield` op here")
                    .with_help("The `if` operation blocks must be terminated by a `yield`")
                    .into_report())
            }
        } else {
            Err(context
                .session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid `if` operation")
                .with_primary_label(self.span(), "`if` blocks require a terminator")
                .with_help("The `if` operation blocks must be terminated by a `yield`")
                .into_report())
        }
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
        use smallvec::smallvec;

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
    traits(SingleBlock),
    implements(RegionBranchOpInterface)
)]
pub struct While {
    #[region]
    before: Region,
    #[region]
    after: Region,
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
