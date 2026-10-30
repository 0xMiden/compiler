use alloc::{format, rc::Rc, string::String, vec::Vec};

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    diagnostics::Severity,
    dialects::builtin::attributes::{U32ArrayAttr, U32Attr},
    effects::*,
    parse::ParserExt,
    patterns::RewritePatternSet,
    print::AsmPrinter,
    traits::*,
    *,
};

use crate::ScfDialect;

/// [If] is a structured control flow operation representing conditional execution.
///
/// An [If] takes a single condition as an argument, which chooses between one of its two regions
/// based on the condition. If the condition is true, then the `then_body` region is executed,
/// otherwise `else_body`.
///
/// Neither region allows any arguments, and both regions must be terminated with one of:
///
/// * [midenc_hir::dialects::builtin::Ret] to return from the enclosing function directly
/// * `midenc_dialect_ub::Unreachable` to abort execution
/// * [Yield] to return from the enclosing [If]
///
/// The op itself touches no memory and no advice resource — selecting a region is not a read or a
/// write — so it declares no effects OF ITS OWN. What it does to either is whatever the taken
/// region does, which is what [`HasRecursiveMemoryEffects`] and [`HasRecursiveAdviceEffects`] say;
/// the declarations are complementary, and all of them are needed. An empty declaration without a
/// marker would claim the whole `scf.if` is effect-free on that channel no matter what its regions
/// hold — and the two markers are registered separately, so declaring one says nothing about the
/// other.
#[derive(OpPrinter, OpParser, EffectOpInterface)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(
        SingleBlock,
        NoRegionArguments,
        HasRecursiveMemoryEffects,
        HasRecursiveAdviceEffects,
        PropagateControlFlowBreak
    ),
    implements(RegionBranchOpInterface, MemoryEffectOpInterface, AdviceEffectOpInterface, OpPrinter)
)]
pub struct If {
    #[operand]
    condition: Bool,
    #[region(name = "then")]
    then_body: Region,
    #[region(name = "else")]
    else_body: Region,
    #[results]
    returns: AnyType,
}

impl If {
    /// The [Yield] terminating the `then` region, if it is terminated by one.
    ///
    /// Returns `None` when the branch is terminated by something else — an `scf.break`, an
    /// `scf.continue`, a `builtin.ret` or a `ub.unreachable`. **This is fallible on purpose**: a
    /// break-terminated branch is legal IR, and this accessor used to `expect` a yield, so every
    /// caller that assumed "the branch yields" is now a caller that has to say what it does when
    /// the branch leaves the `scf.if` entirely instead.
    pub fn then_yield(&self) -> Option<UnsafeIntrusiveEntityRef<Yield>> {
        self.then_body().entry().terminator()?.try_downcast_op::<Yield>().ok()
    }

    /// The [Yield] terminating the `else` region, if it is terminated by one. See
    /// [Self::then_yield].
    pub fn else_yield(&self) -> Option<UnsafeIntrusiveEntityRef<Yield>> {
        self.else_body().entry().terminator()?.try_downcast_op::<Yield>().ok()
    }
}

impl Canonicalizable for If {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::ConvertTrivialIfToSelect::new(context.clone()));
        rewrites.push(crate::canonicalization::IfRemoveUnusedResults::new(context.clone()));
        rewrites.push(crate::canonicalization::FoldRedundantYields::new(context));
    }
}

impl RegionBranchOpInterface for If {
    fn get_entry_successor_regions(
        &self,
        operands: &[Option<AttributeRef>],
    ) -> RegionSuccessorIter<'_> {
        let condition = operands[0].as_ref().and_then(|v| v.borrow().as_bool());
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
        operands: &[Option<AttributeRef>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        let condition = operands[0].as_ref().and_then(|v| v.borrow().as_bool());

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
///
/// No effects of its own; its memory and advice effects are its regions', per
/// [`HasRecursiveMemoryEffects`] and [`HasRecursiveAdviceEffects`].
/// See [`If`] for why both declarations are required.
#[derive(OpPrinter, OpParser, EffectOpInterface)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(
        SingleBlock,
        HasRecursiveMemoryEffects,
        HasRecursiveAdviceEffects,
        PropagateControlFlowBreak
    ),
    implements(
        RegionBranchOpInterface,
        HasBreakingControlFlowOpInterface,
        LoopLikeOpInterface,
        MemoryEffectOpInterface,
        AdviceEffectOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct While {
    #[operands]
    inits: AnyType,
    #[region]
    before: Region,
    #[region]
    after: Region,
    #[results]
    returns: AnyType,
}

impl OperandRangeRequirementOpInterface for While {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // Initial values are forwarded to the loop header without interpreting their range.
        OperandRangeRequirement::None
    }
}

impl While {
    /// The [Condition] terminating the `before` region, if it is terminated by one.
    ///
    /// Returns `None` when the header is terminated by an `scf.break` instead — which is legal,
    /// and equivalent to `scf.condition(false, args)`.
    pub fn condition_op(&self) -> Option<UnsafeIntrusiveEntityRef<Condition>> {
        self.before().entry().terminator()?.try_downcast_op::<Condition>().ok()
    }

    /// The [Yield] terminating the `after` region, if it is terminated by one.
    ///
    /// Returns `None` when the body's entry block ends in an `scf.break` or `scf.continue`: such
    /// a body has no yielded values, and its operands are the *target's* results or iter_args,
    /// not this loop's next iteration.
    pub fn yield_op(&self) -> Option<UnsafeIntrusiveEntityRef<Yield>> {
        self.after().entry().terminator()?.try_downcast_op::<Yield>().ok()
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
        // A body whose entry block ends in `scf.break`/`scf.continue` has NO yielded values: the
        // exit's operands are the target's results or iter_args, and projecting them here would
        // hand a caller an operand group that means something else entirely.
        let mut yield_op = self.yield_op()?.as_operation_ref();

        // The values which are yielded to each iteration
        Some(EntityMut::project(yield_op.borrow_mut(), |op| op.operands_mut().group_mut(0)))
    }
}

impl While {
    /// Returns true if some op nested in this loop addresses it with an early exit of `kind`
    /// (or of any kind, when `kind` is `None`).
    ///
    /// The walk is over the loop's own regions, and resolution is the shared `resolve_exit`, so a
    /// `break [2]` in a nested loop is correctly *not* counted here: it addresses the outer loop,
    /// and the outer loop is the one whose successors change.
    fn has_nested_exit(&self, kind: Option<RegionExitKind>) -> bool {
        let me = self.as_operation().as_operation_ref();
        let region_has_exit = |region: &Region| {
            region
                .prewalk(|op: &Operation| {
                    if let Some(exit) = op.as_trait::<dyn RegionExitTerminatorOpInterface>()
                        && kind.is_none_or(|kind| kind == exit.get_exit_kind())
                        && exit.get_exit_target() == Some(me)
                    {
                        return WalkResult::<()>::Break(());
                    }
                    WalkResult::Continue(())
                })
                .was_interrupted()
        };
        region_has_exit(&self.before()) || region_has_exit(&self.after())
    }
}

impl HasBreakingControlFlowOpInterface for While {
    #[inline]
    fn has_nested_predecessors(&self) -> bool {
        self.has_nested_exit(None)
    }

    #[inline]
    fn get_continue_target_region(&self) -> Option<RegionRef> {
        // A continue re-enters the header, which for `scf.while` is the `before` region — the
        // same region `scf.yield` feeds on the ordinary back edge.
        Some(self.before().as_region_ref())
    }
}

/// The structural rules of [While]:
///
/// * The entry block of the `before` region terminates with [Condition], and the entry block of
///   the `after` region terminates with [Yield] — or either with an exit that leaves the region:
///   a return-like terminator that is neither of the two ([Break] or [Continue], addressing this
///   loop or an enclosing one, or a function return). Such an exit's operands belong to what it
///   addresses, so they are not compared with this loop's lists. `ub.unreachable` is not an exit: the loop
///   asks a region's terminator for the operands of its edge, which an unreachable cannot answer.
/// * The init operands and the `before` block arguments agree in number and, position by
///   position, in type, and so do the [Yield] operands, when the body yields.
/// * The operands forwarded by [Condition], when the header has one, the `after` block arguments
///   and the results agree in number and, position by position, in type.
impl Verify<dyn LoopLikeOpInterface> for While {
    fn verify(&self, context: &Context) -> Result<(), Report> {
        let op = self.as_operation();
        let invalid = |label: String| {
            context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message(format!("invalid operation {}", op.name()))
                .with_primary_label(op.span(), label)
                .into_report()
        };

        let before = self.before();
        let before_block = before
            .entry_block_ref()
            .ok_or_else(|| invalid("the before region has no block".into()))?;
        let before_block = before_block.borrow();
        let after = self.after();
        let after_block = after
            .entry_block_ref()
            .ok_or_else(|| invalid("the after region has no block".into()))?;
        let after_block = after_block.borrow();

        // An exit leaves the region: return-like, and not the terminator of the other region.
        let is_exit = |op: &Operation| {
            op.implements::<dyn ReturnLike>() && !op.is::<Condition>() && !op.is::<Yield>()
        };
        let before_term = before_block
            .terminator()
            .ok_or_else(|| invalid("the before block has no terminator".into()))?;
        let before_term = before_term.borrow();
        let condition = before_term.downcast_ref::<Condition>();
        if condition.is_none() && !is_exit(&before_term) {
            return Err(invalid(format!(
                "the before block must end in scf.condition or in an exit, but it ends in {}",
                before_term.name()
            )));
        }
        let after_term = after_block
            .terminator()
            .ok_or_else(|| invalid("the after block has no terminator".into()))?;
        let after_term = after_term.borrow();
        let yield_op = after_term.downcast_ref::<Yield>();
        if yield_op.is_none() && !is_exit(&after_term) {
            return Err(invalid(format!(
                "the after block must end in scf.yield or in an exit, but it ends in {}",
                after_term.name()
            )));
        }

        let operand_types =
            |range: OpOperandRange<'_>| range.iter().map(|o| o.borrow().ty()).collect::<Vec<_>>();
        let argument_types = |block: &Block| {
            block.arguments().iter().map(|a| a.borrow().ty().clone()).collect::<Vec<_>>()
        };
        let inits = ("init operand", operand_types(self.inits()));
        let before_args = ("before block argument", argument_types(&before_block));
        let yielded =
            yield_op.map(|yield_op| ("scf.yield operand", operand_types(yield_op.yielded())));
        let forwarded = condition.map(|condition| {
            ("scf.condition forwarded operand", operand_types(condition.forwarded()))
        });
        let after_args = ("after block argument", argument_types(&after_block));
        let results = (
            "result",
            self.results().all().iter().map(|r| r.borrow().ty().clone()).collect::<Vec<_>>(),
        );

        let mut pairs = Vec::from([(&inits, &before_args)]);
        if let Some(yielded) = &yielded {
            pairs.push((yielded, &inits));
        }
        if let Some(forwarded) = &forwarded {
            pairs.push((forwarded, &after_args));
            pairs.push((&results, forwarded));
        }
        for ((lhs_name, lhs), (rhs_name, rhs)) in pairs {
            if lhs.len() != rhs.len() {
                return Err(invalid(format!(
                    "{} {lhs_name}(s), but {} {rhs_name}(s)",
                    lhs.len(),
                    rhs.len()
                )));
            }
            if let Some((index, (lhs_ty, rhs_ty))) =
                lhs.iter().zip(rhs.iter()).enumerate().find(|(_, (lhs, rhs))| lhs != rhs)
            {
                return Err(invalid(format!(
                    "{lhs_name} {index} has type '{lhs_ty}', but {rhs_name} {index} has type \
                     '{rhs_ty}'"
                )));
            }
        }

        Ok(())
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
                // otherwise, when branching from `after` the only successor is `before` — UNLESS
                // the body contains a break addressing this loop, in which case the body also
                // exits the loop directly, without passing through `scf.condition`.
                //
                // **This is the edge the early-exit design exists to add, and omitting it is the
                // silent failure.** Nothing crashes without it; every analysis simply believes
                // the only way out of the loop is the condition's false edge, and concludes that
                // code reachable only via the break is dead.
                //
                // The question asked here is narrower than `has_nested_predecessors`: a
                // `continue` is also a nested predecessor, but it re-enters `before`, an edge
                // this arm already reports. Only a `break` adds an exit.
                if region == after_region {
                    let mut infos: SmallVec<[_; 2]> =
                        smallvec![RegionSuccessorInfo::Entering(before_region)];
                    if self.has_nested_exit(Some(RegionExitKind::Break)) {
                        infos.push(RegionSuccessorInfo::Returning(
                            self.results()
                                .all()
                                .iter()
                                .map(|r| r.borrow().as_value_ref())
                                .collect(),
                        ));
                    }
                    RegionSuccessorIter::new(self.as_operation(), infos)
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
        _operands: &[Option<AttributeRef>],
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
///
/// No effects of its own; its memory and advice effects are its regions', per
/// [`HasRecursiveMemoryEffects`] and [`HasRecursiveAdviceEffects`].
/// See [`If`] for why both declarations are required.
#[derive(EffectOpInterface)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(
        SingleBlock,
        HasRecursiveMemoryEffects,
        HasRecursiveAdviceEffects,
        PropagateControlFlowBreak
    ),
    implements(RegionBranchOpInterface, MemoryEffectOpInterface, AdviceEffectOpInterface, OpPrinter)
)]
pub struct IndexSwitch {
    #[operand]
    selector: UInt32,
    #[attr]
    cases: U32ArrayAttr,
    #[region]
    default_region: Region,
}

impl OpPrinter for IndexSwitch {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use alloc::borrow::Cow;

        use formatter::*;

        printer.print_space();
        printer.print_value_uses(ValueRange::<1>::Operands(&[self.selector().as_operand_ref()]));
        printer.print_space();

        for case in self.cases().iter() {
            let index = self.get_case_index_for_selector(*case).unwrap();
            let region = self.get_case_region(index);
            *printer += nl() + const_text("case ") + display(*case) + const_text(" ");
            printer.print_region(&region.borrow());
        }

        *printer += nl() + const_text("default ");
        printer.print_region(&self.default_region());

        if self.op.has_attributes() {
            printer.print_space();
            printer.print_attribute_dictionary(
                self.op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }

        printer.print_space();
        printer.print_colon_type_list(
            self.results().iter().map(|r| Cow::Owned(r.borrow().ty().clone())),
        );
    }
}

impl OpParser for IndexSwitch {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use alloc::{format, vec};

        use midenc_hir::{
            diagnostics::{LabeledSpan, RelatedError, Report, Severity, miette::diagnostic},
            dialects::builtin::attributes::Array,
            parse::ParserError,
        };

        let selector = parser.parse_operand(/*allow_result_number=*/ true)?;
        let selector = parser.resolve_operand(selector, Type::U32)?;
        state.add_operand(selector);

        let mut cases = Array::<u32>::default();
        let mut regions = SmallVec::<[RegionRef; 2]>::default();
        while parser.parse_optional_custom_keyword("case")?.is_some() {
            let case_value = parser.parse_decimal_integer::<u32>()?;
            if cases.contains(&case_value) {
                return Err(ParserError::Report(RelatedError::new(Report::from(diagnostic!(
                    severity = Severity::Error,
                    labels = vec![LabeledSpan::at(
                        case_value.span(),
                        "this case selector has already been used"
                    )],
                    "invalid scf.index_switch operation"
                )))));
            }

            let region = parser.context().create_region();
            parser.parse_region(region, &[], false)?;

            cases.push(case_value.into_inner());
            regions.push(region);
        }

        parser.parse_custom_keyword("default")?;
        let fallback_region = parser.context().create_region();
        parser.parse_region(fallback_region, &[], false)?;

        state
            .add_attribute("cases", parser.context_rc().create_attribute::<U32ArrayAttr, _>(cases));
        // The default region is declared first on the operation; case regions follow it (see
        // `get_case_region`).
        state.add_region(fallback_region);
        for region in regions {
            state.add_region(region);
        }

        parser.parse_optional_attribute_dict(&mut state.attrs)?;
        parser.parse_colon_type_list(&mut state.results)?;

        Ok(())
    }
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
        operands: &[Option<AttributeRef>],
    ) -> RegionSuccessorIter<'_> {
        let selector = operands[0].as_ref().and_then(|v| v.borrow().as_u32());
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
        operands: &[Option<AttributeRef>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        let selector = operands[0].as_ref().and_then(|v| v.borrow().as_u32());

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
        rewrites.push(crate::canonicalization::FoldConstantIndexSwitch::new(context.clone()));
        rewrites.push(crate::canonicalization::FoldRedundantYields::new(context.clone()));
        rewrites.push(crate::canonicalization::IndexSwitchRemoveUnusedResults::new(context));
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(Terminator, ReturnLike),
    implements(
        RegionBranchTerminatorOpInterface,
        MemoryEffectOpInterface,
        AdviceEffectOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct Condition {
    #[operand]
    condition: Bool,
    #[operands]
    forwarded: AnyType,
}

impl OperandRangeRequirementOpInterface for Condition {
    fn operand_range_requirement(&self, operand_index: usize) -> OperandRangeRequirement {
        match operand_index {
            0 => OperandRangeRequirement::Required(ValueRangeConstraint::Type(Type::I1)),
            _ => OperandRangeRequirement::None,
        }
    }
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
        operands: &[Option<AttributeRef>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        // A [While] loop has two regions: `before` (containing this op), and `after`, which this
        // op branches to when the condition is true. If the condition is false, control is
        // transferred back to the parent [While] operation, with the forwarded operands of the
        // condition used as the results of the [While] operation.
        //
        // We can return a single statically-known region if we were given a constant condition
        // value, otherwise we must return both possible regions.
        let cond = operands[0].as_ref().and_then(|v| v.borrow().as_bool());
        let mut regions = SmallVec::<[RegionSuccessorInfo; 2]>::default();

        let parent_op = self.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        let while_op = parent_op
            .downcast_ref::<While>()
            .expect("expected `Condition` op to be a child of a `While` op");
        let after_region = while_op.after().as_region_ref();

        // We can't know the condition until runtime, so both the parent `while` op and
        match cond {
            None => {
                regions.push(RegionSuccessorInfo::Entering(after_region));
                regions.push(RegionSuccessorInfo::Returning(
                    while_op.results().all().iter().map(|r| r.borrow().as_value_ref()).collect(),
                ));
            }
            Some(true) => {
                regions.push(RegionSuccessorInfo::Entering(after_region));
            }
            Some(false) => {
                regions.push(RegionSuccessorInfo::Returning(
                    while_op.results().all().iter().map(|r| r.borrow().as_value_ref()).collect(),
                ));
            }
        }

        regions
    }
}

/// The [Yield] op is used in conjunction with [If] and [While] ops as a return-like terminator.
///
/// * With [If], its regions must be terminated with either a [Yield] or an `Unreachable` op.
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(Terminator, ReturnLike, Pure, AlwaysSpeculatable),
    implements(
        RegionBranchTerminatorOpInterface,
        MemoryEffectOpInterface,
        AdviceEffectOpInterface,
        OperandRangeRequirementOpInterface,
        ConditionallySpeculatable,
        OpPrinter,
    )
)]
pub struct Yield {
    #[operands]
    yielded: AnyType,
}

impl OperandRangeRequirementOpInterface for Yield {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
    }
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
        _operands: &[Option<AttributeRef>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        // Depending on the type of operation containing this yield, the set of successor regions
        // is always known.
        //
        // * [While] may only have a yield to its `before` region
        // * [If] may only yield to its parent
        // * [IndexSwitch] may only yield to its parent
        let parent_op = self.parent_op().unwrap();
        let parent_op = parent_op.borrow();
        if parent_op.is::<If>() || parent_op.is::<IndexSwitch>() {
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

impl ConditionallySpeculatable for Yield {
    fn speculatability(&self) -> Speculatability {
        Speculatability::Speculatable
    }
}

/// Resolve the operation an early exit in `op` addresses, walking outward and counting receivers.
///
/// This is the whole of the `levels` mechanism, and it is deliberately one function shared by the
/// verifier, the successor-region computation and the interpreter: three answers to "which op does
/// this exit leave?" that disagreed would be worse than one that is wrong. It is also the
/// implementation of [RegionExitTerminatorOpInterface::resolve_exit] for both exit ops, which is
/// what lets `hir` verify rules 2-7 over the ANSWER without knowing that `levels` exists.
///
/// # RULE 1 (`levels >= 1`) IS DETECTED HERE AND NOWHERE ELSE — THE HOIST'S HONEST BOUNDARY
///
/// Every other rule of [RegionExitTerminatorOpInterface]'s contract is verified in `hir`, for
/// every implementor. Rule 1 cannot be: *"the name is well-formed"* is a property of THIS
/// encoding — a token operand or a label would be malformed in some other way — and the interface
/// has no vocabulary for it. What the interface can do is require the malformedness to be
/// **reported**, which is what [ExitDefectKind::Malformed] below is; what it cannot do is detect
/// one. So a resolver that silently treated `levels = 0` as "not found" would produce the wrong
/// diagnostic and nothing would catch it. The rule stays here, and it stays stated.
fn resolve_levels(op: &Operation, levels: u32, kind: RegionExitKind) -> ExitResolution {
    // Rule 1. The wording is this encoding's own, and `hir` renders it verbatim.
    if levels == 0 {
        return ExitResolution::Unresolved(ExitDefect {
            kind: ExitDefectKind::Malformed,
            detail: alloc::format!(
                "`levels` is 1-based and counts enclosing loops, so it must be at least 1, but \
                 this {kind} names level 0"
            ),
        });
    }

    let not_found = |found: u32| {
        ExitResolution::Unresolved(ExitDefect {
            kind: ExitDefectKind::NotFound,
            detail: alloc::format!(
                "this {kind} names enclosing loop {levels}, but only {found} enclosing loop(s) \
                 were found before the enclosing function body was reached"
            ),
        })
    };

    let mut remaining = levels;
    let mut from_region = match op.parent_region() {
        Some(region) => region,
        None => return not_found(0),
    };
    loop {
        let Some(parent_ref) = from_region.borrow().parent() else {
            return not_found(levels - remaining);
        };
        let parent = parent_ref.borrow();

        // A break may not escape a function: the enclosing callable body is the outer bound of
        // the search, and it is checked BEFORE the receiver test so that a callable which is also
        // somehow loop-like cannot be addressed from outside its own body.
        if parent.implements::<dyn CallableOpInterface>() {
            return not_found(levels - remaining);
        }

        if parent.implements::<dyn HasBreakingControlFlowOpInterface>() {
            remaining -= 1;
            if remaining == 0 {
                return ExitResolution::Resolved(ResolvedExit {
                    target: parent_ref,
                    from_region,
                    // VACUOUS UNDER `levels`, AND `None` IS THE HONEST ANSWER. The search above
                    // stops at the first `CallableOpInterface` ancestor BEFORE testing for a
                    // receiver, so a resolved target is inside this exit's own callable by
                    // construction — there is no fact here the verifier does not already have,
                    // and paying an O(depth) walk on every `get_exit_target()` call to restate a
                    // structural one would be a cost with no check behind it. See
                    // [ResolvedExit::callable] for when the field stops being vacuous.
                    callable: None,
                });
            }
        }

        // Every op the exit passes THROUGH must have opted in to forwarding it. This is the
        // safety property: an op that has not declared transparency makes the IR *invalid* rather
        // than silently mis-analyzed, and it is what makes the set of ops an early exit can cross
        // a closed, declared set rather than whatever happens to be nested at the time.
        if !parent.implements::<dyn PropagateControlFlowBreak>() {
            return ExitResolution::Unresolved(ExitDefect {
                detail: alloc::format!(
                    "this {kind} would have to cross '{}', which has not declared that it \
                     forwards breaking control flow",
                    parent.name()
                ),
                kind: ExitDefectKind::Opaque {
                    crossing: parent_ref,
                },
            });
        }

        let Some(next) = parent.parent_region() else {
            return not_found(levels - remaining);
        };
        from_region = next;
    }
}

/// The target of an exit in `op`, or `None` if it does not resolve — the successor computations'
/// half of [resolve_levels].
fn exit_target(op: &Operation, levels: u32, kind: RegionExitKind) -> Option<OperationRef> {
    match resolve_levels(op, levels, kind) {
        ExitResolution::Resolved(resolved) => Some(resolved.target),
        ExitResolution::Unresolved(_) => None,
    }
}

/// [Break] transfers control out of the `levels`-th enclosing loop, past every intermediate op.
///
/// `levels` is 1-based and counts enclosing ops which receive breaking control flow (i.e.
/// [While]): `levels = 1` names the innermost enclosing loop, `levels = 2` the one outside it.
///
/// The operands become the target loop's **results**, so their types must equal the target's
/// result types.
///
/// # A [Break] IN A LOOP'S `before` REGION — WHAT EACH CONSUMER ACTUALLY DOES
///
/// The three consumers do not agree, and the disagreement is stated here rather than left for
/// someone to discover at the end of a pipeline:
///
/// * **The verifier ACCEPTS it.** It is equivalent to `scf.condition(false, args)`, and rule 6 —
///   which rejects a [Continue] in its own target's header — deliberately does not extend to a
///   break. `v6_rejects_continue_in_the_targets_header`'s accept side is the witness.
/// * **The interpreter RUNS it, correctly**, unwinding to the named ancestor exactly as it does
///   for a break in the body.
/// * **MASM codegen REFUSES it, loudly.** `LowerEarlyExits` rewrites the condition region itself
///   — that is where it installs the header guard — so an exit already living there would have to
///   be spliced into the very code being generated for it. No frontend produces the shape, so the
///   pass reports rather than attempting it (`an_exit_inside_a_loop_condition_is_fenced`).
///
/// So the shape is legal to *write* and legal to *interpret*, and it does not survive to MASM.
/// It costs nothing to allow at the IR level, which is why the verifier still allows it.
///
/// Effect-free: unwinding to an enclosing op and carrying values is neither a read nor a write.
/// Where it may appear is a matter of its terminator-ness — see the nested-terminator clause in
/// `Operation::would_be_trivially_dead_even_if_terminator`, without which this declaration
/// deletes the `scf.if` that carries the exit, and the exit with it.
#[derive(EffectOpInterface)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(Terminator, ReturnLike),
    implements(
        RegionBranchTerminatorOpInterface,
        RegionExitTerminatorOpInterface,
        MemoryEffectOpInterface,
        AdviceEffectOpInterface,
        OpPrinter
    )
)]
pub struct Break {
    #[attr]
    levels: U32Attr,
    #[operands]
    args: AnyType,
}

/// [Continue] transfers control to the `levels`-th enclosing loop's header, past every
/// intermediate op.
///
/// `levels` is 1-based and counts enclosing loops, exactly as [Break]'s does.
///
/// The operands become the target loop's **next iter_args**, so their types must equal the
/// `before` region's entry-block argument types — i.e. exactly what [Yield] carries in the
/// `after` region.
///
/// Effect-free, for the same reason [`Break`] is.
#[derive(EffectOpInterface)]
#[effects(MemoryEffect())]
#[effects(AdviceEffect())]
#[operation(
    dialect = ScfDialect,
    traits(Terminator, ReturnLike),
    implements(
        RegionBranchTerminatorOpInterface,
        RegionExitTerminatorOpInterface,
        MemoryEffectOpInterface,
        AdviceEffectOpInterface,
        OpPrinter
    )
)]
pub struct Continue {
    #[attr]
    levels: U32Attr,
    #[operands]
    args: AnyType,
}

/// Print `scf.break`/`scf.continue` as `<op> <levels> <operands> : (<types>)`.
///
/// The level count is printed POSITIONALLY rather than in the attribute dictionary, because a
/// `U32Attr` cannot be written in `.hir` text at all: an attribute dictionary parses its values
/// with `Type::Unknown` (`Parser::parse_attribute_dict`), for which an integer literal falls
/// through to "invalid attribute value". `scf.index_switch`'s `case <n>` has the same shape for
/// the same reason.
fn print_exit(op: &Operation, levels: u32, printer: &mut AsmPrinter<'_>) {
    use alloc::borrow::Cow;

    use formatter::*;

    printer.print_space();
    *printer += display(levels);

    let operands = op.operands().all();
    let types = operands
        .iter()
        .map(|o| Cow::Owned(o.borrow().as_value_ref().borrow().ty().clone()))
        .collect::<SmallVec<[_; 2]>>();
    if !operands.is_empty() {
        printer.print_space();
        printer.print_value_uses(ValueRange::<2>::from(operands));
    }
    printer.print_space();
    printer.print_colon_type_list(types);
}

/// Parse `<levels> [<operands>] : (<types>)` into `state`, the inverse of [print_exit].
fn parse_exit(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
    let levels = parser.parse_decimal_integer::<u32>()?;
    state.add_attribute(
        "levels",
        parser.context_rc().create_attribute::<U32Attr, _>(levels.into_inner()),
    );

    let mut operands = SmallVec::default();
    parser.parse_operand_list(
        &mut operands,
        midenc_hir::parse::Delimiter::None,
        /*allow_result_number=*/ true,
        None,
    )?;
    let mut types = SmallVec::<[Type; 4]>::default();
    parser.parse_colon_type_list(&mut types)?;
    for (operand, ty) in operands.into_iter().zip(types) {
        state.add_operand(parser.resolve_operand(operand, ty)?);
    }

    Ok(())
}

impl OpPrinter for Break {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        print_exit(self.as_operation(), *self.get_levels(), printer)
    }
}

impl OpParser for Break {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        parse_exit(state, parser)
    }
}

impl OpPrinter for Continue {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        print_exit(self.as_operation(), *self.get_levels(), printer)
    }
}

impl OpParser for Continue {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        parse_exit(state, parser)
    }
}

impl RegionExitTerminatorOpInterface for Break {
    #[inline]
    fn resolve_exit(&self) -> ExitResolution {
        resolve_levels(self.as_operation(), *self.get_levels(), RegionExitKind::Break)
    }

    #[inline(always)]
    fn get_exit_kind(&self) -> RegionExitKind {
        RegionExitKind::Break
    }
}

impl RegionExitTerminatorOpInterface for Continue {
    #[inline]
    fn resolve_exit(&self) -> ExitResolution {
        resolve_levels(self.as_operation(), *self.get_levels(), RegionExitKind::Continue)
    }

    #[inline(always)]
    fn get_exit_kind(&self) -> RegionExitKind {
        RegionExitKind::Continue
    }
}

impl RegionBranchTerminatorOpInterface for Break {
    #[inline]
    fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        SuccessorOperandRange::forward(self.args())
    }

    #[inline]
    fn get_mutable_successor_operands(
        &mut self,
        _point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_> {
        SuccessorOperandRangeMut::forward(self.args_mut())
    }

    fn get_successor_regions(
        &self,
        _operands: &[Option<AttributeRef>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        // A break has exactly one successor and it is statically known: control leaves the target
        // loop, whose results receive this op's operands.
        let Some(target) =
            exit_target(self.as_operation(), *self.get_levels(), RegionExitKind::Break)
        else {
            // Malformed IR — the verifier rejects an exit that does not resolve, so this is
            // VERIFIER-GUARDED LATENCY rather than a conservative answer, and it is worth being
            // precise about which.
            //
            // An empty successor set is the BLIND state, not the unknown one: it is exactly the
            // mutation N-1 uses to prove the framework can tell correct from blind, and an
            // analysis reading it concludes that what follows the exit is unreachable — a *wrong*
            // answer, not a safe one. `RegionExitTerminatorOpInterface::get_exit_target`'s own
            // doc says the same thing from the other side: a `None` there means "unknown control
            // flow", never "no control flow". The interface has no representation for unknown
            // successors, so this returns nothing and relies on the verifier having already
            // rejected the program. A guessed target would be silently wrong in a worse way.
            return SmallVec::new();
        };
        let values = target
            .borrow()
            .results()
            .all()
            .iter()
            .map(|r| r.borrow().as_value_ref())
            .collect();
        smallvec![RegionSuccessorInfo::Exiting {
            target,
            to: RegionBranchPoint::Parent,
            values,
        }]
    }
}

impl RegionBranchTerminatorOpInterface for Continue {
    #[inline]
    fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        SuccessorOperandRange::forward(self.args())
    }

    #[inline]
    fn get_mutable_successor_operands(
        &mut self,
        _point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_> {
        SuccessorOperandRangeMut::forward(self.args_mut())
    }

    fn get_successor_regions(
        &self,
        _operands: &[Option<AttributeRef>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        let Some(target) =
            exit_target(self.as_operation(), *self.get_levels(), RegionExitKind::Continue)
        else {
            return SmallVec::new();
        };
        let target_op = target.borrow();
        let Some(header) = target_op
            .as_trait::<dyn HasBreakingControlFlowOpInterface>()
            .and_then(|receiver| receiver.get_continue_target_region())
        else {
            // Same verifier-guarded latency as above: a receiver with no continue target cannot
            // be addressed by a `scf.continue` in verified IR.
            return SmallVec::new();
        };
        let values = header
            .borrow()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect();
        smallvec![RegionSuccessorInfo::Exiting {
            target,
            to: RegionBranchPoint::Child(header),
            values,
        }]
    }
}

#[cfg(test)]
mod tests {
    use midenc_expect_test::expect;
    use midenc_hir::{
        diagnostics::Report,
        dialects::{
            builtin::{Function, attributes::Signature},
            test::TestDialect,
        },
        testing::parse_function_fixpoint,
    };

    use super::*;

    #[test]
    fn nested_break_can_return_a_loop_result() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = r#"
builtin.function public extern("C") @exit(%flag: i1, %value: u32) -> u32 {
    %result = scf.while %value before {
    ^head(%current: u32):
        scf.condition %flag, %current : (i1, u32);
    } after {
    ^body(%next: u32):
        scf.if %flag then {
            scf.break 1 %next : (u32);
        } else {
            scf.yield : ();
        } : (i1) -> ();
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %result : (u32);
};"#;
        parse_function_fixpoint(&context, "nested_break.hir", source)?;
        Ok(())
    }

    /// An op with a region that has **not** declared [PropagateControlFlowBreak].
    ///
    /// It exists only for V-3, and it has to exist because the tree has no such op today: the
    /// region-bearing ops that can nest inside a function body are exactly `scf.if`,
    /// `scf.index_switch` and `scf.while`, and all three opted in. Rule 3's whole job is to keep
    /// that true as the tree grows, so its witness has to bring its own non-member — otherwise
    /// the rule would be "tested" by a set that cannot violate it.
    #[derive(OpPrinter, OpParser, EffectOpInterface)]
    #[effects(MemoryEffect())]
    #[effects(AdviceEffect())]
    #[operation(
        dialect = TestDialect,
        traits(SingleBlock, HasRecursiveMemoryEffects, HasRecursiveAdviceEffects),
        implements(MemoryEffectOpInterface, AdviceEffectOpInterface, OpPrinter)
    )]
    pub struct OpaqueRegion {
        #[region]
        body: Region,
    }

    /// [OpaqueRegion]'s twin, identical in every respect except that it declares
    /// [PropagateControlFlowBreak].
    ///
    /// The pair is what makes V-3 a test of the trait rather than of anything else about the two
    /// programs: the accept and reject programs differ in exactly one token, and that token names
    /// an op whose only difference is the marker.
    #[derive(OpPrinter, OpParser, EffectOpInterface)]
    #[effects(MemoryEffect())]
    #[effects(AdviceEffect())]
    #[operation(
        dialect = TestDialect,
        traits(
            SingleBlock,
            HasRecursiveMemoryEffects,
            HasRecursiveAdviceEffects,
            PropagateControlFlowBreak
        ),
        implements(MemoryEffectOpInterface, AdviceEffectOpInterface, OpPrinter)
    )]
    pub struct TransparentRegion {
        #[region]
        body: Region,
    }

    /// An exit whose resolver **forgets rule 3**: it counts outward to the first receiver without
    /// ever asking whether the ops it passed through declared [PropagateControlFlowBreak].
    ///
    /// It exists to prove that the blanket verifier in `hir` CHECKS the resolver rather than
    /// restating it. `scf.break` in the same position reports the crossing itself, as an
    /// [ExitDefectKind::Opaque] defect, and is rejected through the defect arm (V-3). This op
    /// reports a confident `Resolved` and is rejected through the RE-DERIVATION arm, with the
    /// same rule and the same wording — which is the whole claim of variant (B).
    #[derive(OpPrinter, OpParser, EffectOpInterface)]
    #[effects(MemoryEffect())]
    #[effects(AdviceEffect())]
    #[operation(
        dialect = TestDialect,
        traits(Terminator, ReturnLike),
        implements(
            RegionBranchTerminatorOpInterface,
            RegionExitTerminatorOpInterface,
            MemoryEffectOpInterface,
        AdviceEffectOpInterface,
            OpPrinter
        )
    )]
    pub struct BlindExit {
        #[operands]
        args: AnyType,
    }

    /// An exit whose resolver **forgets rule 2's receiver test**: it names the nearest enclosing
    /// region-bearing op, whatever that op is.
    ///
    /// Placed directly in a loop body its answer is right, and the verifier accepts it; placed one
    /// region deeper it names an op that receives no breaking control flow, and the verifier
    /// rejects it. The pair is what makes this a test of the CHECK rather than of the op.
    #[derive(OpPrinter, OpParser, EffectOpInterface)]
    #[effects(MemoryEffect())]
    #[effects(AdviceEffect())]
    #[operation(
        dialect = TestDialect,
        traits(Terminator, ReturnLike),
        implements(
            RegionBranchTerminatorOpInterface,
            RegionExitTerminatorOpInterface,
            MemoryEffectOpInterface,
        AdviceEffectOpInterface,
            OpPrinter
        )
    )]
    pub struct StrayExit {
        #[operands]
        args: AnyType,
    }

    /// An exit whose target and originating region are **right**, and whose `callable` is wrong:
    /// it names the receiver it found instead of the callable it lives in.
    ///
    /// This is the witness for the HIR half of the Q9-6 carry. [BlindExit] in the identical
    /// program position reports the identical target and `from_region` with `callable: None` and
    /// is ACCEPTED — so the two resolutions differ in exactly one field, and that field is what
    /// the verifier is reacting to.
    #[derive(OpPrinter, OpParser, EffectOpInterface)]
    #[effects(MemoryEffect())]
    #[effects(AdviceEffect())]
    #[operation(
        dialect = TestDialect,
        traits(Terminator, ReturnLike),
        implements(
            RegionBranchTerminatorOpInterface,
            RegionExitTerminatorOpInterface,
            MemoryEffectOpInterface,
        AdviceEffectOpInterface,
            OpPrinter
        )
    )]
    pub struct DisownedExit {
        #[operands]
        args: AnyType,
    }

    /// The nearest enclosing region and its parent op, or `None` at the top of the IR.
    fn nearest_enclosing(op: &Operation) -> Option<(RegionRef, OperationRef)> {
        let region = op.parent_region()?;
        let parent = region.borrow().parent()?;
        Some((region, parent))
    }

    /// The honest walk [BlindExit] and [DisownedExit] share: outward to the first receiver, with
    /// no transparency test, which is enough for both of their programs.
    fn nearest_receiver(op: &Operation) -> Option<(RegionRef, OperationRef)> {
        let mut region = op.parent_region();
        while let Some(current) = region {
            let parent_ref = current.borrow().parent()?;
            if parent_ref.borrow().implements::<dyn HasBreakingControlFlowOpInterface>() {
                return Some((current, parent_ref));
            }
            region = parent_ref.borrow().parent_region();
        }
        None
    }

    #[derive(OpPrinter, OpParser)]
    #[operation(
        dialect = TestDialect,
        traits(Terminator, ReturnLike),
        implements(RegionExitTerminatorOpInterface, RegionBranchTerminatorOpInterface, OpPrinter)
    )]
    pub struct WrongOriginExit {
        #[operands]
        args: AnyType,
    }

    /// Implements the Rust trait but omits its dynamic registration, which analyses require.
    #[derive(OpPrinter, OpParser)]
    #[operation(
        dialect = TestDialect,
        traits(Terminator, ReturnLike),
        implements(RegionExitTerminatorOpInterface, OpPrinter)
    )]
    pub struct UnregisteredBranchExit {
        #[operands]
        args: AnyType,
    }

    impl RegionExitTerminatorOpInterface for UnregisteredBranchExit {
        fn resolve_exit(&self) -> ExitResolution {
            let (from_region, target) = nearest_receiver(self.as_operation()).unwrap();
            ExitResolution::Resolved(ResolvedExit {
                target,
                from_region,
                callable: None,
            })
        }

        fn get_exit_kind(&self) -> RegionExitKind {
            RegionExitKind::Break
        }
    }

    macro_rules! impl_test_break_successors {
        ($op:ty) => {
            impl RegionBranchTerminatorOpInterface for $op {
                fn get_successor_operands(
                    &self,
                    _point: RegionBranchPoint,
                ) -> SuccessorOperandRange<'_> {
                    SuccessorOperandRange::forward(self.args())
                }

                fn get_mutable_successor_operands(
                    &mut self,
                    _point: RegionBranchPoint,
                ) -> SuccessorOperandRangeMut<'_> {
                    SuccessorOperandRangeMut::forward(self.args_mut())
                }

                fn get_successor_regions(
                    &self,
                    _operands: &[Option<AttributeRef>],
                ) -> SmallVec<[RegionSuccessorInfo; 2]> {
                    let target = self.get_exit_target().unwrap();
                    let values = target
                        .borrow()
                        .results()
                        .iter()
                        .map(|result| result.borrow().as_value_ref())
                        .collect();
                    smallvec![RegionSuccessorInfo::Exiting {
                        target,
                        to: RegionBranchPoint::Parent,
                        values,
                    }]
                }
            }
        };
    }

    impl_test_break_successors!(WrongOriginExit);
    impl_test_break_successors!(UnregisteredBranchExit);

    /// A callable that also registers as an exit receiver, to test boundary-check precedence.
    #[derive(OpPrinter, OpParser)]
    #[operation(
        dialect = TestDialect,
        traits(SingleBlock),
        implements(CallableOpInterface, RegionBranchOpInterface, HasBreakingControlFlowOpInterface, OpPrinter)
    )]
    pub struct CallableReceiver {
        #[region]
        body: Region,
    }

    impl CallableOpInterface for CallableReceiver {
        fn get_callable_region(&self) -> Option<RegionRef> {
            Some(self.body().as_region_ref())
        }

        fn signature(&self) -> Signature {
            Signature::new(&self.as_operation().context_rc(), [], [])
        }
    }

    impl RegionBranchOpInterface for CallableReceiver {
        fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
            let successor = match point {
                RegionBranchPoint::Parent => {
                    RegionSuccessorInfo::Entering(self.body().as_region_ref())
                }
                RegionBranchPoint::Child(_) => RegionSuccessorInfo::Returning(SmallVec::new()),
            };
            RegionSuccessorIter::new(self.as_operation(), [successor])
        }
    }

    impl HasBreakingControlFlowOpInterface for CallableReceiver {
        fn has_nested_predecessors(&self) -> bool {
            true
        }

        fn get_continue_target_region(&self) -> Option<RegionRef> {
            None
        }
    }

    #[test]
    fn exit_verifier_requires_registered_branch_terminator_interface() {
        let error = rejection(
            "unregistered_branch_exit.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.unregistered_branch_exit %j : (u32);"),
        );
        assert!(error.contains("RegionBranchTerminatorOpInterface"), "{error}");
    }

    #[test]
    fn exit_verifier_rejects_a_callable_receiver_as_target() {
        let error = rejection(
            "callable_receiver_exit.hir",
            r#"
builtin.function public extern("C") @f() {
    test.callable_receiver {
        test.stray_exit : ();
    };
    builtin.ret;
};"#,
        );
        assert!(error.contains("callable boundary"), "{error}");
    }

    impl RegionExitTerminatorOpInterface for WrongOriginExit {
        fn resolve_exit(&self) -> ExitResolution {
            let (_, target) = nearest_receiver(self.as_operation()).unwrap();
            let from_region = target
                .borrow()
                .as_trait::<dyn HasBreakingControlFlowOpInterface>()
                .unwrap()
                .get_continue_target_region()
                .unwrap();
            ExitResolution::Resolved(ResolvedExit {
                target,
                from_region,
                callable: None,
            })
        }

        fn get_exit_kind(&self) -> RegionExitKind {
            RegionExitKind::Break
        }
    }

    #[test]
    fn exit_verifier_checks_the_resolvers_originating_region() {
        let error = rejection(
            "wrong_exit_origin.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.wrong_origin_exit %j : (u32);"),
        );
        assert!(error.contains("originating region"), "{error}");
    }

    impl RegionExitTerminatorOpInterface for BlindExit {
        fn resolve_exit(&self) -> ExitResolution {
            match nearest_receiver(self.as_operation()) {
                Some((from_region, target)) => ExitResolution::Resolved(ResolvedExit {
                    target,
                    from_region,
                    callable: None,
                }),
                None => ExitResolution::Unresolved(ExitDefect {
                    kind: ExitDefectKind::NotFound,
                    detail: alloc::string::ToString::to_string("no enclosing receiver"),
                }),
            }
        }

        fn get_exit_kind(&self) -> RegionExitKind {
            RegionExitKind::Break
        }
    }

    impl RegionExitTerminatorOpInterface for DisownedExit {
        fn resolve_exit(&self) -> ExitResolution {
            match nearest_receiver(self.as_operation()) {
                Some((from_region, target)) => ExitResolution::Resolved(ResolvedExit {
                    target,
                    from_region,
                    // THE LIE, and the only one: the receiver is not the callable.
                    callable: Some(target),
                }),
                None => ExitResolution::Unresolved(ExitDefect {
                    kind: ExitDefectKind::NotFound,
                    detail: alloc::string::ToString::to_string("no enclosing receiver"),
                }),
            }
        }

        fn get_exit_kind(&self) -> RegionExitKind {
            RegionExitKind::Break
        }
    }

    impl RegionExitTerminatorOpInterface for StrayExit {
        fn resolve_exit(&self) -> ExitResolution {
            match nearest_enclosing(self.as_operation()) {
                Some((from_region, target)) => ExitResolution::Resolved(ResolvedExit {
                    target,
                    from_region,
                    callable: None,
                }),
                None => ExitResolution::Unresolved(ExitDefect {
                    kind: ExitDefectKind::NotFound,
                    detail: alloc::string::ToString::to_string("no enclosing op"),
                }),
            }
        }

        fn get_exit_kind(&self) -> RegionExitKind {
            RegionExitKind::Break
        }
    }

    impl RegionBranchTerminatorOpInterface for BlindExit {
        fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
            SuccessorOperandRange::forward(self.args())
        }

        fn get_mutable_successor_operands(
            &mut self,
            _point: RegionBranchPoint,
        ) -> SuccessorOperandRangeMut<'_> {
            SuccessorOperandRangeMut::forward(self.args_mut())
        }

        fn get_successor_regions(
            &self,
            _operands: &[Option<AttributeRef>],
        ) -> SmallVec<[RegionSuccessorInfo; 2]> {
            SmallVec::new()
        }
    }

    impl RegionBranchTerminatorOpInterface for StrayExit {
        fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
            SuccessorOperandRange::forward(self.args())
        }

        fn get_mutable_successor_operands(
            &mut self,
            _point: RegionBranchPoint,
        ) -> SuccessorOperandRangeMut<'_> {
            SuccessorOperandRangeMut::forward(self.args_mut())
        }

        fn get_successor_regions(
            &self,
            _operands: &[Option<AttributeRef>],
        ) -> SmallVec<[RegionSuccessorInfo; 2]> {
            SmallVec::new()
        }
    }

    impl RegionBranchTerminatorOpInterface for DisownedExit {
        fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
            SuccessorOperandRange::forward(self.args())
        }

        fn get_mutable_successor_operands(
            &mut self,
            _point: RegionBranchPoint,
        ) -> SuccessorOperandRangeMut<'_> {
            SuccessorOperandRangeMut::forward(self.args_mut())
        }

        fn get_successor_regions(
            &self,
            _operands: &[Option<AttributeRef>],
        ) -> SmallVec<[RegionSuccessorInfo; 2]> {
            SmallVec::new()
        }
    }

    /// Parse `source` and return the error it must be rejected with.
    #[track_caller]
    fn rejection(name: &str, source: &str) -> alloc::string::String {
        let context = Rc::new(Context::default());
        match parse_function_fixpoint(&context, name, source) {
            Ok(_) => panic!("expected '{name}' to be rejected, but it was accepted"),
            Err(err) => alloc::format!("{err}"),
        }
    }

    /// Find the first operation of type `T` in the entry block of `function`.
    fn find_op<T: OpRegistration>(function: &Function) -> UnsafeIntrusiveEntityRef<T> {
        function
            .body()
            .entry()
            .body()
            .iter()
            .find_map(|op| op.as_operation_ref().try_downcast_op::<T>().ok())
            .unwrap_or_else(|| {
                panic!("expected a {} op in the function body", <T as OpRegistration>::full_name())
            })
    }

    #[test]
    fn loop_range_requirements_distinguish_conditions_from_forwarded_values() -> Result<(), Report>
    {
        let context = Rc::new(Context::default());
        let source = r#"
builtin.function public extern("C") @forward(%condition: i1, %value: u32) -> u32 {
    %result = scf.while %value before {
    ^head(%current: u32):
        scf.condition %condition, %current : (i1, u32);
    } after {
    ^body(%next: u32):
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %result : (u32);
};"#;
        let (function, _) =
            parse_function_fixpoint(&context, "loop_range_requirements.hir", source)?;
        let function = function.borrow();
        let while_op = find_op::<While>(&function);
        let while_op = while_op.borrow();
        assert_eq!(operation_operand_range_requirement(while_op.as_operation(), 0), None);
        let condition = while_op.condition_op().unwrap();
        let condition = condition.borrow();
        assert_eq!(
            operation_operand_range_requirement(condition.as_operation(), 0),
            Some(ValueRangeConstraint::Type(Type::I1))
        );
        assert_eq!(operation_operand_range_requirement(condition.as_operation(), 1), None);
        Ok(())
    }

    /// The regions of an operation with operands must parse with no pre-bound entry arguments:
    /// the `^block(...)` header the printer emits declares them. Also covers the variadic
    /// forwarded operands of `scf.condition`, which parse as the trailing comma list following
    /// the condition operand, and the `scf.while` result type signature.
    #[test]
    fn parse_scf_while_round_trips() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @count_to(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %count = scf.while %zero before {
    ^head(%i: u32):
        %continue = arith.lt %i, %n;
        scf.condition %continue, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %next = arith.incr %j;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %count : (u32);
};";
        let (function, printed) = parse_function_fixpoint(&context, "parse_scf_while.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @count_to(%0: u32) -> u32 {
                %1 = arith.constant 0 : u32;
                %6 = scf.while %1 before {
                ^block2(%2: u32):
                    %3 = arith.lt %2, %0;
                    scf.condition %3, %2 : (i1, u32);
                } after {
                ^block3(%4: u32):
                    %5 = arith.incr %4;
                    scf.yield %5 : (u32);
                } : (u32) -> (u32);
                builtin.ret %6 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let while_op = find_op::<While>(&function);
        let while_op = while_op.borrow();
        assert_eq!(while_op.inits().len(), 1);
        assert_eq!(while_op.num_results(), 1);
        assert_eq!(while_op.before().entry().num_arguments(), 1);
        let condition = while_op.condition_op().unwrap();
        let condition = condition.borrow();
        assert_eq!(condition.forwarded().len(), 1);

        Ok(())
    }

    /// The trailing forwarded-operand list of `scf.condition` may be empty, in which case the
    /// printer omits it entirely; the parser must accept the bare form.
    #[test]
    fn parse_scf_condition_without_forwarded_operands() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @spin(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    scf.while %zero before {
    ^head(%i: u32):
        %continue = arith.lt %i, %n;
        scf.condition %continue : (i1);
    } after {
    ^body:
        scf.yield %zero : (u32);
    } : (u32) -> ();
    builtin.ret %n : (u32);
};";
        let (function, printed) =
            parse_function_fixpoint(&context, "parse_scf_condition_bare.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @spin(%0: u32) -> u32 {
                %1 = arith.constant 0 : u32;
                scf.while %1 before {
                ^block2(%2: u32):
                    %3 = arith.lt %2, %0;
                    scf.condition %3 : (i1);
                } after {
                ^block3:
                    scf.yield %1 : (u32);
                } : (u32) -> ();
                builtin.ret %0 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let while_op = find_op::<While>(&function);
        let while_op = while_op.borrow();
        assert_eq!(while_op.num_results(), 0);
        let condition = while_op.condition_op().unwrap();
        let condition = condition.borrow();
        assert_eq!(condition.forwarded().len(), 0);

        Ok(())
    }

    /// Multi-name result bindings (`%x, %y = scf.if ...`) map the parsed names onto the
    /// flattened result list of a single variadic result group.
    #[test]
    fn parse_multi_name_result_bindings() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @pick(%c: i1, %a: u32, %b: u32) -> u32 {
    %x, %y = scf.if %c then {
        scf.yield %a, %b : (u32, u32);
    } else {
        scf.yield %b, %a : (u32, u32);
    } : (i1) -> (u32, u32);
    %sum = arith.add %x, %y <{ overflow = #builtin.overflow<unchecked> }>;
    builtin.ret %sum : (u32);
};";
        let (function, printed) =
            parse_function_fixpoint(&context, "parse_multi_result.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @pick(%0: i1, %1: u32, %2: u32) -> u32 {
                %3, %4 = scf.if %0 then {
                    scf.yield %1, %2 : (u32, u32);
                } else {
                    scf.yield %2, %1 : (u32, u32);
                } : (i1) -> (u32, u32);
                %5 = arith.add %3, %4 <{ overflow = #builtin.overflow<unchecked> }>;
                builtin.ret %5 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let if_op = find_op::<If>(&function);
        let if_op = if_op.borrow();
        assert_eq!(if_op.num_results(), 2);
        // Both bound names resolve to the if's results: the adder uses each exactly once.
        for result in if_op.results().all().iter() {
            assert_eq!(result.borrow().iter_uses().count(), 1);
        }

        Ok(())
    }

    /// An op with more than 255 results prints and re-parses, one SSA name per result.
    #[test]
    fn parse_wide_result_list() -> Result<(), Report> {
        const NUM_RESULTS: usize = 256;

        let context = Rc::new(Context::default());
        let names = (0..NUM_RESULTS).map(|i| format!("%r{i}")).collect::<Vec<_>>().join(", ");
        let values = ["%a"; NUM_RESULTS].join(", ");
        let types = ["u32"; NUM_RESULTS].join(", ");
        let source = format!(
            "\
builtin.function public extern(\"C\") @wide(%c: i1, %a: u32) -> u32 {{
    {names} = scf.if %c then {{
        scf.yield {values} : ({types});
    }} else {{
        scf.yield {values} : ({types});
    }} : (i1) -> ({types});
    builtin.ret %r{last} : (u32);
}};",
            last = NUM_RESULTS - 1
        );
        let (function, _printed) =
            parse_function_fixpoint(&context, "parse_wide_result_list.hir", &source)?;

        let function = function.borrow();
        let if_op = find_op::<If>(&function);
        assert_eq!(if_op.borrow().num_results(), NUM_RESULTS);

        Ok(())
    }

    /// `scf.index_switch` regions parse with the default region first, matching the accessors
    /// (`default_region` is region 0, case regions follow in case order).
    #[test]
    fn parse_index_switch_region_order() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @dispatch(%sel: u32, %a: u32, %b: u32) -> u32 {
    %r = scf.index_switch %sel
    case 1 {
        scf.yield %a : (u32);
    }
    default {
        scf.yield %b : (u32);
    } : u32;
    builtin.ret %r : (u32);
};";
        let (function, printed) =
            parse_function_fixpoint(&context, "parse_index_switch.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @dispatch(%0: u32, %1: u32, %2: u32) -> u32 {
                %3 = scf.index_switch %0 
                case 1 {
                    scf.yield %1 : (u32);
                }
                default {
                    scf.yield %2 : (u32);
                } : (u32);
                builtin.ret %3 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let body = function.body();
        let entry = body.entry();
        let arg_a = entry.arguments()[1] as ValueRef;
        let arg_b = entry.arguments()[2] as ValueRef;

        let yielded_value = |region: &Region| -> ValueRef {
            let terminator = region.entry().terminator().unwrap();
            let yield_op = terminator
                .try_downcast_op::<Yield>()
                .expect("expected region to terminate with scf.yield");
            let yield_op = yield_op.borrow();
            let yielded = yield_op.yielded();
            let operand = yielded.iter().next().unwrap();
            operand.borrow().as_value_ref()
        };

        let switch_op = find_op::<IndexSwitch>(&function);
        let switch_op = switch_op.borrow();
        // The default region yields %b and the `case 1` region yields %a; if the parser
        // appended the default region last, the two would come back swapped.
        assert_eq!(yielded_value(&switch_op.default_region()), arg_b);
        let case_region = switch_op.get_case_region(0);
        assert_eq!(yielded_value(&case_region.borrow()), arg_a);

        Ok(())
    }
    /// The round-trip fixture for the new ops: a `break` nested in an `scf.if` inside the loop it
    /// exits, printed and re-parsed to a fixpoint.
    #[test]
    fn parse_scf_break_round_trips() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @first_over(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %found = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %stop = arith.gt %j, %n;
        %k = scf.if %stop then {
            scf.break 1 %j : (u32);
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %k;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %found : (u32);
};";
        let (function, printed) = parse_function_fixpoint(&context, "scf_break.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @first_over(%0: u32) -> u32 {
                %1 = arith.constant 0 : u32;
                %8 = scf.while %1 before {
                ^block2(%2: u32):
                    %3 = arith.lt %2, %0;
                    scf.condition %3, %2 : (i1, u32);
                } after {
                ^block3(%4: u32):
                    %5 = arith.gt %4, %0;
                    %6 = scf.if %5 then {
                        scf.break 1 %4 : (u32);
                    } else {
                        scf.yield %4 : (u32);
                    } : (i1) -> (u32);
                    %7 = arith.incr %6;
                    scf.yield %7 : (u32);
                } : (u32) -> (u32);
                builtin.ret %8 : (u32);
            };"#]]
        .assert_eq(&printed);

        // The break resolves through the `scf.if` to the loop, not to the `scf.if`.
        let function = function.borrow();
        let while_op = find_op::<While>(&function);
        let break_op = {
            let while_op = while_op.borrow();
            let if_op = while_op
                .after()
                .entry()
                .body()
                .iter()
                .find_map(|op| op.as_operation_ref().try_downcast_op::<If>().ok())
                .expect("expected an scf.if in the loop body");
            let if_op = if_op.borrow();
            if_op
                .then_body()
                .entry()
                .terminator()
                .unwrap()
                .try_downcast_op::<Break>()
                .unwrap()
        };
        assert_eq!(
            break_op.borrow().get_exit_target(),
            Some(while_op.borrow().as_operation().as_operation_ref())
        );
        assert_eq!(break_op.borrow().get_exit_kind(), RegionExitKind::Break);

        Ok(())
    }

    /// `scf.continue` round-trips, and resolves through an intermediate `scf.if` to the loop's
    /// header rather than to the `scf.if`.
    #[test]
    fn parse_scf_continue_round_trips() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @skip_odds(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %total = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %odd = arith.gt %j, %n;
        %k = scf.if %odd then {
            %bump = arith.incr %j;
            scf.continue 1 %bump : (u32);
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %k;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %total : (u32);
};";
        let (function, printed) = parse_function_fixpoint(&context, "scf_continue.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @skip_odds(%0: u32) -> u32 {
                %1 = arith.constant 0 : u32;
                %9 = scf.while %1 before {
                ^block2(%2: u32):
                    %3 = arith.lt %2, %0;
                    scf.condition %3, %2 : (i1, u32);
                } after {
                ^block3(%4: u32):
                    %5 = arith.gt %4, %0;
                    %7 = scf.if %5 then {
                        %6 = arith.incr %4;
                        scf.continue 1 %6 : (u32);
                    } else {
                        scf.yield %4 : (u32);
                    } : (i1) -> (u32);
                    %8 = arith.incr %7;
                    scf.yield %8 : (u32);
                } : (u32) -> (u32);
                builtin.ret %9 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let while_op = find_op::<While>(&function);
        let while_ref = while_op.borrow().as_operation().as_operation_ref();
        let continue_op = {
            let while_op = while_op.borrow();
            let if_op = while_op
                .after()
                .entry()
                .body()
                .iter()
                .find_map(|op| op.as_operation_ref().try_downcast_op::<If>().ok())
                .expect("expected an scf.if in the loop body");
            let if_op = if_op.borrow();
            if_op
                .then_body()
                .entry()
                .terminator()
                .unwrap()
                .try_downcast_op::<Continue>()
                .unwrap()
        };
        assert_eq!(continue_op.borrow().get_exit_target(), Some(while_ref));
        assert_eq!(continue_op.borrow().get_exit_kind(), RegionExitKind::Continue);

        // The successor names the loop's header, and it is an Exiting edge — not a Returning one
        // to the `scf.if` that lexically contains it.
        let continue_op = continue_op.borrow();
        let successors = <Continue as RegionBranchTerminatorOpInterface>::get_successor_regions(
            &continue_op,
            &[None],
        );
        assert_eq!(successors.len(), 1);
        match &successors[0] {
            RegionSuccessorInfo::Exiting { target, to, .. } => {
                assert_eq!(*target, while_ref);
                assert_eq!(
                    *to,
                    RegionBranchPoint::Child(while_op.borrow().before().as_region_ref())
                );
            }
            other => panic!("expected an Exiting successor, got {other:?}"),
        }

        Ok(())
    }

    /// **V-1** — rule 1: `levels` is 1-based, so zero names no loop.
    #[test]
    fn v1_rejects_levels_zero() {
        let err = rejection(
            "v1.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        scf.break 0 %j : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        );
        assert!(err.contains("must be at least 1"), "unexpected diagnostic: {err}");
        assert!(err.contains("names level 0"), "unexpected diagnostic: {err}");
    }

    /// **V-2** — rule 2: `levels` must resolve. Two enclosing loops, and the exit names three.
    #[test]
    fn v2_rejects_more_levels_than_enclosing_loops() {
        let err = rejection(
            "v2.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^outer_head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^outer_body(%j: u32):
        %inner = scf.while %j before {
        ^inner_head(%k: u32):
            %go = arith.lt %k, %n;
            scf.condition %go, %k : (i1, u32);
        } after {
        ^inner_body(%m: u32):
            scf.break 3 %m : (u32);
        } : (u32) -> u32;
        scf.yield %inner : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        );
        assert!(err.contains("names enclosing loop 3"), "unexpected diagnostic: {err}");
        assert!(
            err.contains("only 2 enclosing loop(s) were found"),
            "unexpected diagnostic: {err}"
        );
    }

    /// **V-3** — rule 3, the opt-in property, and the witness that proves the set is closed.
    ///
    /// The program is the accepted one with a single op swapped: `test.opaque_region` in place of
    /// `scf.if`. Both hold a region, both sit between the break and its target loop, and the ONLY
    /// difference is that one declares [PropagateControlFlowBreak] and the other does not. So the
    /// accept-side control below is not decoration — it is what makes this a test of membership
    /// rather than of nesting depth.
    #[test]
    fn v3_rejects_an_exit_crossing_an_op_that_has_not_opted_in() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        INTERMEDIATE {
            scf.break 1 %j : (u32);
        };
        scf.yield %j : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};";

        let err = rejection("v3.hir", &PROGRAM.replace("INTERMEDIATE", "test.opaque_region"));
        assert!(
            err.contains("would have to cross 'test.opaque_region'"),
            "unexpected diagnostic: {err}"
        );
        assert!(
            err.contains("has not declared that it forwards breaking control flow"),
            "unexpected diagnostic: {err}"
        );

        // THE ACCEPT SIDE: the same program with one token changed — an op identical to the one
        // above except that it declares the marker.
        let context = Rc::new(Context::default());
        parse_function_fixpoint(
            &context,
            "v3_accept.hir",
            &PROGRAM.replace("INTERMEDIATE", "test.transparent_region"),
        )
        .expect("an exit crossing an op that declares PropagateControlFlowBreak must be accepted");
    }

    /// **V-4** — rule 4: a break's operands become the target loop's results.
    #[test]
    fn v4_rejects_break_operands_that_do_not_match_the_targets_results() {
        let err = rejection(
            "v4.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %flag = arith.gt %j, %n;
        scf.break 1 %flag : (i1);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        );
        assert!(
            err.contains("this break carries (i1), but the loop it exits produces (u32)"),
            "unexpected diagnostic: {err}"
        );
    }

    /// **V-5** — rule 5: a continue's operands become the target loop's next iter_args.
    #[test]
    fn v5_rejects_continue_operands_that_do_not_match_the_targets_iter_args() {
        let err = rejection(
            "v5.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %flag = arith.gt %j, %n;
        scf.continue 1 %flag : (i1);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        );
        assert!(
            err.contains("this continue carries (i1), but the loop it targets iterates on (u32)"),
            "unexpected diagnostic: {err}"
        );
    }

    /// **V-6** — rule 6: a continue in its own target's header re-enters the region it is in.
    ///
    /// The mirror case is legal and is asserted beside it: a `break` in the `before` region is
    /// equivalent to `scf.condition(false, args)` and costs nothing to allow.
    #[test]
    fn v6_rejects_continue_in_the_targets_header() {
        let err = rejection(
            "v6.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        scf.continue 1 %i : (u32);
    } after {
    ^body(%j: u32):
        scf.yield %j : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        );
        assert!(
            err.contains("originates in the header of the loop it targets"),
            "unexpected diagnostic: {err}"
        );

        // A break in the header IS legal.
        let context = Rc::new(Context::default());
        parse_function_fixpoint(
            &context,
            "v6_accept.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        scf.break 1 %i : (u32);
    } after {
    ^body(%j: u32):
        scf.yield %j : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};",
        )
        .expect("a break in a loop's header is equivalent to `scf.condition(false, args)`");
    }

    /// **V-7** — rule 2's outer bound: a break may not escape a function.
    #[test]
    fn v7_rejects_a_break_with_no_enclosing_loop() {
        let err = rejection(
            "v7.hir",
            "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    scf.break 1 %n : (u32);
};",
        );
        assert!(err.contains("names enclosing loop 1"), "unexpected diagnostic: {err}");
        assert!(
            err.contains("only 0 enclosing loop(s) were found before the enclosing function body"),
            "unexpected diagnostic: {err}"
        );
    }

    /// V-3's program with the exit left as a hole, so that the resolver-check witnesses run on the
    /// SAME shape the rule's own witness runs on, and the only variable is which op sits in the
    /// hole and how deep.
    const RESOLVER_CHECK_PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        BODY
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};";

    /// The reject-side body: the exit one region deeper, inside `wrapper`.
    fn nested_body(wrapper: &str, exit: &str) -> alloc::string::String {
        alloc::format!(
            "{wrapper} {{\n            {exit} %j : (u32);\n        }};\n        scf.yield %j : \
             (u32);"
        )
    }

    /// **V-8** — the blanket CHECKS the resolver on rule 3, and the check is not the resolver's.
    ///
    /// [BlindExit] never asks whether it crossed a transparent op, so on the reject side it
    /// returns a confident `Resolved` naming the loop. The rejection therefore cannot come from
    /// the implementor, and it does not: it comes from `hir`'s own re-derivation, in the same
    /// words rule 3 has always used.
    ///
    /// **The accept side is what makes this a test of the CHECK.** The same op, one region
    /// shallower, resolves correctly and is accepted — so the verifier is discriminating between
    /// two answers rather than refusing the op.
    #[test]
    fn v8_the_blanket_rejects_a_resolver_that_skipped_rule_3() {
        let err = rejection(
            "v8.hir",
            &RESOLVER_CHECK_PROGRAM
                .replace("BODY", &nested_body("test.opaque_region", "test.blind_exit")),
        );
        assert!(
            err.contains("would have to cross 'test.opaque_region'"),
            "unexpected diagnostic: {err}"
        );
        assert!(
            err.contains("has not declared that it forwards breaking control flow"),
            "unexpected diagnostic: {err}"
        );

        // THE ACCEPT SIDE: the same op where its answer happens to be right.
        let context = Rc::new(Context::default());
        parse_function_fixpoint(
            &context,
            "v8_accept.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.blind_exit %j : (u32);"),
        )
        .expect("an exit whose resolver returns the right answer must be accepted");
    }

    /// **V-9** — the blanket CHECKS the resolver on rule 2's receiver half.
    ///
    /// [StrayExit] names the nearest enclosing region-bearing op. Inside a `test.transparent_region`
    /// that op forwards breaking control flow but does not RECEIVE it, so rule 3 cannot be what
    /// fires and rule 2 is. The accept side is the same op one region shallower, where the nearest
    /// enclosing op really is the loop.
    #[test]
    fn v9_the_blanket_rejects_a_resolver_that_named_a_non_receiver() {
        let err = rejection(
            "v9.hir",
            &RESOLVER_CHECK_PROGRAM
                .replace("BODY", &nested_body("test.transparent_region", "test.stray_exit")),
        );
        assert!(
            err.contains("resolved to 'test.transparent_region'"),
            "unexpected diagnostic: {err}"
        );
        assert!(
            err.contains("does not receive breaking control flow"),
            "unexpected diagnostic: {err}"
        );

        let context = Rc::new(Context::default());
        parse_function_fixpoint(
            &context,
            "v9_accept.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.stray_exit %j : (u32);"),
        )
        .expect("an exit whose resolver returns the right answer must be accepted");
    }

    /// **V-10** — the Q9-6 carry, on the HIR side: `ResolvedExit::callable` is CHECKED.
    ///
    /// [DisownedExit] and [BlindExit] resolve to the same target from the same region in the same
    /// program; the only difference between their two resolutions is that one names a `callable`
    /// and it is the wrong one. **That is what makes this a test of the field rather than of the
    /// op** — and it is why the field is not decoration for the counting encoding that reports
    /// `None`: an encoding that NAMES its target gets this check for free.
    #[test]
    fn v10_the_blanket_rejects_a_resolver_that_claimed_the_wrong_callable() {
        let err = rejection(
            "v10.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.disowned_exit %j : (u32);"),
        );
        assert!(
            err.contains("resolved to a target in 'scf.while'"),
            "unexpected diagnostic: {err}"
        );
        assert!(
            err.contains("which is not the callable it lives in"),
            "unexpected diagnostic: {err}"
        );

        // THE CONTROL, in the same position: same target, same `from_region`, `callable: None`.
        let context = Rc::new(Context::default());
        parse_function_fixpoint(
            &context,
            "v10_control.hir",
            &RESOLVER_CHECK_PROGRAM.replace("BODY", "test.blind_exit %j : (u32);"),
        )
        .expect("an exit that leaves `callable` unreported must be accepted");
    }
}
