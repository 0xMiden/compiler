use core::fmt;

use super::*;
use crate::{
    AttributeRef, Op, SuccessorOperandRange, SuccessorOperandRangeMut, Type,
    derive::operation_trait, traits::Terminator,
};

/// An op interface that indicates what types of regions it holds
pub trait RegionKindInterface {
    /// Get the [RegionKind] for this operation
    fn kind(&self) -> RegionKind;
    /// Returns true if the kind of this operation's regions requires SSA dominance
    #[inline]
    fn has_ssa_dominance(&self) -> bool {
        matches!(self.kind(), RegionKind::SSA)
    }
    #[inline]
    fn has_graph_regions(&self) -> bool {
        matches!(self.kind(), RegionKind::Graph)
    }
}

// TODO(pauls): Verify that ordinary region terminators have no results or block successors.
/// This interface provides information for region operations that exhibit branching behavior
/// between held regions. I.e., this interface allows for expressing control flow information for
/// region holding operations.
///
/// This interface is meant to model well-defined cases of control-flow and value propagation,
/// where what occurs along control-flow edges is assumed to be side-effect free.
///
/// A "region branch point" indicates a point from which a branch originates. It can indicate either
/// a region of this op or [RegionBranchPoint::Parent]. In the latter case, the branch originates
/// from outside of the op, i.e., when first executing this op.
///
/// A "region successor" indicates the target of a branch. It can indicate either a region of this
/// op or this op. In the former case, the region successor is a region pointer and a range of block
/// arguments to which the "successor operands" are forwarded to. In the latter case, the control
/// flow leaves this op and the region successor is a range of results of this op to which the
/// successor operands are forwarded to.
///
/// By default, successor operands and successor block arguments/successor results must have the
/// same type. `areTypesCompatible` can be implemented to allow non-equal types.
///
/// ## Example
///
/// ```hir,ignore
/// %r = scf.for %iv = %lb to %ub step %step iter_args(%a = %b)
///     -> tensor<5xf32> {
///     ...
///     scf.yield %c : tensor<5xf32>
/// }
/// ```
///
/// `scf.for` has one region. The region has two region successors: the region itself and the
/// `scf.for` op. `%b` is an entry successor operand. `%c` is a successor operand. `%a` is a
/// successor block argument. `%r` is a successor result.
pub trait RegionBranchOpInterface: Op {
    /// Returns the operands of this operation that are forwarded to the region successor's block
    /// arguments or this operation's results when branching to `point`. `point` is guaranteed to
    /// be among the successors that are returned by `get_entry_succcessor_regions` or
    /// `get_successor_regions(parent_op())`.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function returns the operand `%b`
    /// of the `scf.for` op, regardless of the value of `point`, i.e. this op always forwards the
    /// same operands, regardless of whether the loop has 0 or more iterations.
    #[inline]
    #[allow(unused_variables)]
    fn get_entry_successor_operands(&self, point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
        crate::SuccessorOperandRange::empty()
    }
    /// Returns the potential region successors when first executing the op.
    ///
    /// Unlike [Self::get_successor_regions], this method also passes along the constant operands of
    /// this op. Based on these, the implementation may filter out certain successors. By default, it
    /// simply dispatches to `get_successor_regions`. `operands` contains an entry for every operand
    /// of this op, with `None` representing if the operand is non-constant.
    ///
    /// NOTE: The control flow does not necessarily have to enter any region of this op.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function may return two region
    /// successors: the single region of the `scf.for` op and the `scf.for` operation (that
    /// implements this interface). If `%lb`, `%ub`, `%step` are constants and it can be determined
    /// the loop does not have any iterations, this function may choose to return only this
    /// operation. Similarly, if it can be determined that the loop has at least one iteration, this
    /// function may choose to return only the region of the loop.
    #[inline]
    #[allow(unused_variables)]
    fn get_entry_successor_regions(
        &self,
        operands: &[Option<AttributeRef>],
    ) -> RegionSuccessorIter<'_> {
        self.get_successor_regions(RegionBranchPoint::Parent)
    }
    /// Returns the potential region successors when branching from `point`.
    ///
    /// These are the regions that may be selected during the flow of control.
    ///
    /// When `point` is [RegionBranchPoint::Parent], this function returns the region successors
    /// when entering the operation. Otherwise, this method returns the successor regions when
    /// branching from the region indicated by `point`.
    ///
    /// ## Example
    ///
    /// In the example in the top-level docs of this trait, this function returns the region of the
    /// `scf.for` and this operation for either region branch point (`parent` and the region of the
    /// `scf.for`). An implementation may choose to filter out region successors when it is
    /// statically known (e.g., by examining the operands of this op) that those successors are not
    /// branched to.
    fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_>;
    /// Returns a set of invocation bounds, representing the minimum and maximum number of times
    /// this operation will invoke each attached region (assuming the regions yield normally, i.e.
    /// do not abort or invoke an infinite loop). The minimum number of invocations is at least 0.
    /// If the maximum number of invocations cannot be statically determined, then it will be set to
    /// [InvocationBounds::Unknown].
    ///
    /// This function also passes along the constant operands of this op. `operands` contains an
    /// entry for every operand of this op, with `None` representing if the operand is non-constant.
    ///
    /// This function may be called speculatively on operations where the provided operands are not
    /// necessarily the same as the operation's current operands. This may occur in analyses that
    /// wish to determine "what would be the region invocations if these were the operands?"
    #[inline]
    #[allow(unused_variables)]
    fn get_region_invocation_bounds(
        &self,
        operands: &[Option<AttributeRef>],
    ) -> SmallVec<[InvocationBounds; 1]> {
        use smallvec::smallvec;

        smallvec![InvocationBounds::Unknown; self.num_regions()]
    }
    /// This function is called to compare types along control-flow edges.
    ///
    /// By default, the types are check for exact equality.
    #[inline]
    fn are_types_compatible(&self, lhs: &Type, rhs: &Type) -> bool {
        lhs == rhs
    }
    /// Returns `true` if control flow originating from the region at `index` may eventually branch
    /// back to the same region, either from itself, or after passing through other regions first.
    fn is_repetitive_region(&self, index: usize) -> bool {
        self.region(index).is_repetitive_region()
    }
    /// Returns `true` if there is a loop in the region branching graph.
    ///
    /// Only reachable regions (starting from the entry region) are considered.
    fn has_loop(&self) -> bool {
        self.get_successor_regions(RegionBranchPoint::Parent)
            .filter_map(|entry| entry.region_graph_successor())
            .any(|region| {
                Region::traverse_region_graph(&region.borrow(), |r, visited| {
                    // Interrupted traversal if the region was already visited
                    visited.contains(&r.as_region_ref())
                })
            })
    }
}

// TODO(pauls): Verify ordinary region terminators have no results or block successors.
/// This interface provides information for branching terminator operations in the presence of a
/// parent [RegionBranchOpInterface] implementation. It specifies which operands are passed to which
/// successor region.
pub trait RegionBranchTerminatorOpInterface: Op + Terminator {
    /// Get a range of operands corresponding to values that are semantically "returned" by passing
    /// them to the region successor indicated by `point`.
    fn get_successor_operands(&self, point: RegionBranchPoint) -> SuccessorOperandRange<'_>;
    /// Get a mutable range of operands corresponding to values that are semantically "returned" by
    /// passing them to the region successor indicated by `point`.
    fn get_mutable_successor_operands(
        &mut self,
        point: RegionBranchPoint,
    ) -> SuccessorOperandRangeMut<'_>;
    /// Returns the potential region successors that are branched to after this terminator based on
    /// the given constant operands.
    ///
    /// This method also passes along the constant operands of this op. `operands` contains an entry
    /// for every operand of this op, with `None` representing non-constant values.
    ///
    /// The default implementation simply dispatches to the parent `RegionBranchOpInterface`'s
    /// `get_successor_regions` implementation.
    #[allow(unused_variables)]
    fn get_successor_regions(
        &self,
        operands: &[Option<AttributeRef>],
    ) -> SmallVec<[RegionSuccessorInfo; 2]> {
        let parent_region =
            self.parent_region().expect("expected operation to have a parent region");
        let parent_op = parent_region.parent().expect("expected operation to have a parent op");
        parent_op
            .borrow()
            .as_trait::<dyn RegionBranchOpInterface>()
            .expect("invalid region terminator parent: must implement RegionBranchOpInterface")
            .get_successor_regions(RegionBranchPoint::Child(parent_region))
            .into_successor_infos()
    }
}

/// Whether an early exit re-enters its target's header, or leaves the target entirely.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RegionExitKind {
    /// Control leaves `target` — the exit's operands become `target`'s results.
    Break,
    /// Control re-enters `target`'s header — the exit's operands become the header region's
    /// entry-block arguments.
    Continue,
}

impl fmt::Display for RegionExitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Break => f.write_str("break"),
            Self::Continue => f.write_str("continue"),
        }
    }
}

/// The resolved ancestor target, or a diagnostic describing why resolution failed.
#[derive(Debug, Clone)]
pub enum ExitResolution {
    /// The exit addresses an ancestor, described by [ResolvedExit].
    Resolved(ResolvedExit),
    /// The exit addresses nothing, and `ExitDefect` says why in the implementor's own words.
    Unresolved(ExitDefect),
}

/// The target and originating region reported by an exit's resolver.
///
/// The verifier checks the reported ancestry, region, and optional enclosing callable.
#[derive(Debug, Clone)]
pub struct ResolvedExit {
    /// The ancestor operation that receives this exit.
    pub target: OperationRef,
    /// The region of `target` that transitively contains this exit.
    pub from_region: RegionRef,
    /// The enclosing callable, when supplied by the resolver.
    ///
    /// Resolvers may omit this when their search is bounded by the enclosing callable.
    /// The verifier independently checks that the target is an ancestor within that boundary.
    pub callable: Option<OperationRef>,
}

/// A resolution failure. `kind` classifies the defect, while `detail` describes it using
/// the exit operation's target encoding.
#[derive(Debug, Clone)]
pub struct ExitDefect {
    pub kind: ExitDefectKind,
    pub detail: alloc::string::String,
}

/// The kinds of failure an exit resolver can report.
#[derive(Debug, Clone)]
pub enum ExitDefectKind {
    /// The target encoding is malformed. The resolver is responsible for detecting this.
    Malformed,
    /// The name is well-formed and denotes nothing reachable before the callable body.
    NotFound,
    /// Resolution reached an op that has not declared [PropagateControlFlowBreak].
    Opaque { crossing: OperationRef },
}

/// A terminator that may terminate more than its own region, addressing a specific ancestor.
///
/// This is the interface that distinguishes an *early exit* from an ordinary region terminator:
/// an ordinary [RegionBranchTerminatorOpInterface] transfers control to a sibling region of its
/// immediate parent, or back to that parent, and both of those are expressible as a
/// [RegionBranchPoint]. An early exit leaves the immediate parent *and every intermediate op* up
/// to the ancestor it addresses, which is why it is described by
/// [RegionSuccessorInfo::Exiting] rather than by [RegionSuccessorInfo::Returning].
///
/// Every op implementing this interface must also implement
/// [RegionBranchTerminatorOpInterface] — that is the interface every consumer in the tree already
/// dispatches on, so implementing it is what makes an early exit *visible* to them at all. What
/// this interface adds is the ability to ask which ancestor is addressed without knowing the
/// concrete op.
///
/// # Verification
///
/// The resolver must reject malformed target encodings. The default verifier checks that:
///
/// * The operation registers [RegionBranchTerminatorOpInterface] for successor discovery.
/// * The target is an enclosing [HasBreakingControlFlowOpInterface] operation within the
///   enclosing callable, and `from_region` is the target region containing the exit.
/// * Every intermediate operation implements [PropagateControlFlowBreak].
/// * Break operands match the target's results.
/// * Continue operands match the target header's entry-block arguments, and the exit does not
///   originate in that header.
/// * The exit has no results or block successors.
///
/// The verifier checks the reported target's structural validity, but cannot determine whether
/// it is the intended target of an operation-specific encoding. Implementors may specialize the
/// default verifier when they require a different contract.
pub trait RegionExitTerminatorOpInterface: Op + Terminator {
    /// Resolve the target, reporting malformed encodings and unresolved targets explicitly.
    fn resolve_exit(&self) -> ExitResolution;
    /// Whether the exit re-enters the target's header ([RegionExitKind::Continue]) or leaves it
    /// ([RegionExitKind::Break]).
    fn get_exit_kind(&self) -> RegionExitKind;
    /// The ancestor operation that receives this exit, or `None` if it does not resolve.
    ///
    /// A `None` here means the IR is invalid — the verifier rejects an exit that does not resolve
    /// — so consumers should treat it as "unknown control flow", never as "no control flow".
    #[inline]
    fn get_exit_target(&self) -> Option<OperationRef> {
        match self.resolve_exit() {
            ExitResolution::Resolved(resolved) => Some(resolved.target),
            ExitResolution::Unresolved(_) => None,
        }
    }
}

impl<T> crate::Verify<dyn RegionExitTerminatorOpInterface> for T
where
    T: Op + RegionExitTerminatorOpInterface,
{
    default fn verify(&self, context: &Context) -> Result<(), Report> {
        verify_exit(self, context)
    }
}

impl crate::Verify<dyn RegionExitTerminatorOpInterface> for Operation {
    fn should_verify(&self, _context: &Context) -> bool {
        self.implements::<dyn RegionExitTerminatorOpInterface>()
    }

    fn verify(&self, context: &Context) -> Result<(), Report> {
        verify_exit(
            self.as_trait::<dyn RegionExitTerminatorOpInterface>()
                .expect("this operation does not implement 'RegionExitTerminatorOpInterface'"),
            context,
        )
    }
}

/// Check the exit structure independently of the operation-specific target resolver.
fn verify_exit(
    exit: &dyn RegionExitTerminatorOpInterface,
    context: &Context,
) -> Result<(), Report> {
    use midenc_session::diagnostics::{Severity, Spanned};

    let op = exit.as_operation();
    let kind = exit.get_exit_kind();

    // Include the rule in the message so it remains visible when a Report is displayed alone.
    let reject = |message: alloc::string::String, label: &'static str| {
        Err(context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(alloc::format!("invalid {}: {message}", op.name()))
            .with_primary_label(op.span(), label)
            .into_report())
    };

    // A Rust supertrait does not register the interface in the operation's dynamic trait table.
    // Analyses discover successor edges through this registration.
    if !op.implements::<dyn RegionBranchTerminatorOpInterface>() {
        return reject(
            alloc::string::String::from(
                "a structured exit must register RegionBranchTerminatorOpInterface",
            ),
            "this exit does not expose its successor edges",
        );
    }

    // Exits forward operands to their targets and do not branch within their own region.
    if op.num_results() != 0 {
        return reject(
            alloc::format!(
                "a structured exit produces no results, but this one declares {}",
                op.num_results()
            ),
            "this exit declares results",
        );
    }
    if op.num_successors() != 0 {
        return reject(
            alloc::format!(
                "a structured exit has no block successors, but this one declares {}",
                op.num_successors()
            ),
            "this exit declares block successors",
        );
    }

    let resolved = match exit.resolve_exit() {
        ExitResolution::Unresolved(defect) => {
            let label = match defect.kind {
                ExitDefectKind::Malformed => "this exit does not name a receiver",
                ExitDefectKind::NotFound => "this exit does not resolve to a loop",
                ExitDefectKind::Opaque { .. } => {
                    "this exit crosses an operation that does not forward it"
                }
            };
            return reject(defect.detail, label);
        }
        ExitResolution::Resolved(resolved) => resolved,
    };

    // Check any enclosing callable supplied by the resolver.
    if let Some(callable) = resolved.callable {
        let mut enclosing = op.parent_op();
        while let Some(candidate) = enclosing {
            if candidate.borrow().implements::<dyn crate::CallableOpInterface>() {
                break;
            }
            enclosing = candidate.borrow().parent_op();
        }
        if enclosing != Some(callable) {
            return reject(
                alloc::format!(
                    "this {kind} resolved to a target in '{}', which is not the callable it lives \
                     in — an exit may not escape a function",
                    callable.borrow().name()
                ),
                "this exit does not resolve to a loop",
            );
        }
    }

    // Independently walk to the reported target, checking ancestry and propagation through
    // intermediate operations. The target itself need not forward exits.
    let mut region = op.parent_region();
    loop {
        let Some(parent_ref) = region.and_then(|current| current.borrow().parent()) else {
            return reject(
                alloc::format!(
                    "this {kind} resolved to '{}', which does not enclose it",
                    resolved.target.borrow().name()
                ),
                "this exit's target is not one of its ancestors",
            );
        };
        let parent = parent_ref.borrow();
        if parent.implements::<dyn crate::CallableOpInterface>() {
            return reject(
                alloc::format!(
                    "this {kind} resolved to '{}', which crosses a callable boundary — an exit \
                     may not escape a function",
                    resolved.target.borrow().name()
                ),
                "this exit does not resolve to a loop",
            );
        }
        if parent_ref == resolved.target {
            if region != Some(resolved.from_region) {
                return reject(
                    alloc::format!(
                        "this {kind} resolved to an originating region that does not contain it"
                    ),
                    "this exit's originating region does not match its ancestry",
                );
            }
            break;
        }
        if !parent.implements::<dyn PropagateControlFlowBreak>() {
            return reject(
                alloc::format!(
                    "this {kind} would have to cross '{}', which has not declared that it \
                     forwards breaking control flow",
                    parent.name()
                ),
                "this exit crosses an operation that does not forward it",
            );
        }
        region = parent.parent_region();
    }

    let target_op = resolved.target.borrow();
    if !target_op.implements::<dyn HasBreakingControlFlowOpInterface>() {
        return reject(
            alloc::format!(
                "this {kind} resolved to '{}', which does not receive breaking control flow",
                target_op.name()
            ),
            "this exit does not resolve to a loop",
        );
    }

    let operand_types = op
        .operands()
        .all()
        .iter()
        .map(|operand| operand.borrow().as_value_ref().borrow().ty().clone())
        .collect::<SmallVec<[Type; 2]>>();

    match kind {
        RegionExitKind::Break => {
            let expected = target_op
                .results()
                .all()
                .iter()
                .map(|result| result.borrow().ty().clone())
                .collect::<SmallVec<[Type; 2]>>();
            if operand_types != expected {
                return reject(
                    alloc::format!(
                        "this break carries ({}), but the loop it exits produces ({})",
                        DisplayTypes(&operand_types),
                        DisplayTypes(&expected)
                    ),
                    "these operands do not match the target loop's results",
                );
            }
        }
        RegionExitKind::Continue => {
            let Some(header) = target_op
                .as_trait::<dyn HasBreakingControlFlowOpInterface>()
                .and_then(|receiver| receiver.get_continue_target_region())
            else {
                return reject(
                    alloc::format!(
                        "'{}' receives breaking control flow but names no region for a continue \
                         to re-enter",
                        target_op.name()
                    ),
                    "this continue has no header to re-enter",
                );
            };

            // A continue must originate outside the target header.
            if resolved.from_region == header {
                return reject(
                    alloc::string::ToString::to_string(
                        "this continue originates in the header of the loop it targets, where it \
                         would re-enter the region it is already in",
                    ),
                    "this continue is already in the region it names",
                );
            }

            let expected = header
                .borrow()
                .entry()
                .arguments()
                .iter()
                .map(|arg| arg.borrow().ty().clone())
                .collect::<SmallVec<[Type; 2]>>();
            if operand_types != expected {
                return reject(
                    alloc::format!(
                        "this continue carries ({}), but the loop it targets iterates on ({})",
                        DisplayTypes(&operand_types),
                        DisplayTypes(&expected)
                    ),
                    "these operands do not match the target loop's iter_args",
                );
            }
        }
    }

    Ok(())
}

struct DisplayTypes<'a>(&'a [Type]);
impl fmt::Display for DisplayTypes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, ty) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{ty}")?;
        }
        Ok(())
    }
}

/// Marks an operation that forwards exits to enclosing receivers.
///
/// Every operation between an exit and its target must carry this marker; otherwise the exit
/// verifier rejects the operation.
#[operation_trait]
pub trait PropagateControlFlowBreak {}

/// The receiver of a breaking exit.
///
/// A receiver is necessarily a [RegionBranchOpInterface] op, because the exit's successor inputs
/// are its results (for a break) or its header region's block arguments (for a continue), and
/// both of those are the region-branch vocabulary.
pub trait HasBreakingControlFlowOpInterface: RegionBranchOpInterface {
    /// Whether a nested operation breaks or continues to this receiver.
    fn has_nested_predecessors(&self) -> bool;
    /// The region a `continue` re-enters (for a loop: the header).
    fn get_continue_target_region(&self) -> Option<RegionRef>;
}

/// This trait is implemented by operations which have loop-like semantics.
///
/// It provides useful helpers and access to properties of the loop represented, and is used in
/// order to perform transformations on the loop. Implementors will be considered by loop-invariant
/// code motion.
///
/// Loop-carried variables can be exposed through this interface. There are 3 components to a
/// loop-carried variable:
///
/// - The "region iter_arg" is the block argument of the entry block that represents the loop-
///   carried variable in each iteration.
/// - The "init value" is an operand of the loop op that serves as the initial region iter_arg value
///   for the first iteration (if any).
/// - The "yielded" value is the value that is forwarded from one iteration to serve as the region
///   iter_arg of the next iteration.
///
/// If one of the respective interface methods is implemented, so must the other two. The interface
/// verifier ensures that the number of types of the region iter_args, init values and yielded
/// values match.
///
/// Optionally, "loop results" can be exposed through this interface. These are the values that are
/// returned from the loop op when there are no more iterations. The number and types of the loop
/// results must match with the region iter_args. Note: Loop results are optional because some loops
/// (e.g., `scf.while`) may produce results that do match 1-to-1 with the region iter_args.
#[allow(unused_variables)]
#[allow(clippy::result_unit_err)]
pub trait LoopLikeOpInterface: Op {
    /// Returns true if the given value is defined outside of the loop.
    ///
    /// A sensible implementation could be to check whether the value's defining operation lies
    /// outside of the loops body region. If the loop uses explicit capture of dependencies, an
    /// implementation could check whether the value corresponds to a captured dependency.
    fn is_defined_outside_of_loop(&self, value: ValueRef) -> bool {
        let value = value.borrow();
        if let Some(defining_op) = value.get_defining_op() {
            self.as_operation().is_ancestor_of(&defining_op.borrow())
        } else {
            let block_arg = value
                .downcast_ref::<BlockArgument>()
                .expect("invalid value reference: defining op is orphaned");
            let defining_region = block_arg.parent_region().unwrap();
            let defining_op = defining_region.parent().unwrap();
            self.as_operation().is_ancestor_of(&defining_op.borrow())
        }
    }

    /// Returns the entry region for this loop, which is expected to also play the role of loop
    /// header.
    ///
    /// NOTE: It is expected that if the loop has iteration arguments, that the values returned
    /// from `Self::get_region_iter_args` correspond to block arguments of the header region.
    /// Additionally, it is presumed that initialization variables expected by the op are provided
    /// to the loop body via block arguments of this region.
    fn get_loop_header_region(&self) -> RegionRef;

    /// Returns the regions that make up the body of the loop, and should be inspected for loop-
    /// invariant operations.
    fn get_loop_regions(&self) -> SmallVec<[RegionRef; 2]>;

    /// Moves the given loop-invariant operation out of the loop.
    fn move_out_of_loop(&mut self, mut op: OperationRef) {
        op.borrow_mut().move_to(crate::ProgramPoint::before(self.as_operation()));
    }

    /// Promotes the loop body to its containing block if the loop is known to have a single
    /// iteration.
    ///
    /// Returns `Ok` if the promotion was successful
    fn promote_if_single_iteration(
        &mut self,
        rewriter: &mut dyn crate::Rewriter,
    ) -> Result<(), ()> {
        Err(())
    }

    /// Return all induction variables, if they exist.
    ///
    /// If the op has no notion of induction variable, then return `None`. If it does have a notion
    /// but an instance doesn't have induction variables, then return an empty vector.
    fn get_loop_induction_vars(&self) -> Option<SmallVec<[ValueRef; 2]>> {
        None
    }

    /// Return all lower bounds, if they exist.
    ///
    /// If the op has no notion of lower bounds, then return `None`. If it does have a notion but an
    /// instance doesn't have lower bounds, then return an empty vector.
    fn get_loop_lower_bounds(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return all upper bounds, if they exist.
    ///
    /// If the op has no notion of upper bounds, then return `None`. If it does have a notion but an
    /// instance doesn't have upper bounds, then return an empty vector.
    fn get_loop_upper_bounds(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return all steps, if they exist.
    ///
    /// If the op has no notion of steps, then return `None`. If it does have a notion but an
    /// instance doesn't have steps, then return an empty vector.
    fn get_loop_steps(&self) -> Option<SmallVec<[OpFoldResult; 2]>> {
        None
    }

    /// Return the mutable "init" operands that are used as initialization values for the region
    /// "iter_args" of this loop.
    fn get_inits_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().empty_mut()
    }

    /// Return the region "iter_args" (block arguments) that correspond to the "init" operands.
    ///
    /// If the op has multiple regions, return the corresponding block arguments of the entry region.
    fn get_region_iter_args(&self) -> Option<EntityRef<'_, [BlockArgumentRef]>> {
        None
    }

    /// Return the mutable operand range of values that are yielded to the next iteration by the
    /// loop terminator.
    ///
    /// For loop operations that dont yield a value, this should return `None`.
    fn get_yielded_values_mut(&mut self) -> Option<EntityProjectionMut<'_, OpOperandRangeMut<'_>>> {
        None
    }

    /// Return the range of results that are return from this loop and correspond to the "init"
    /// operands.
    ///
    /// Note: This interface method is optional. If loop results are not exposed via this interface,
    /// `None` should be returned.
    ///
    /// Otherwise, the number and types of results must match with the region iter_args, inits and
    /// yielded values that are exposed via this interface. If loop results are exposed but this
    /// loop op has no loop-carried variables, an empty result range (and not `None`) should be
    /// returned.
    fn get_loop_results(&self) -> Option<OpResultRange<'_>> {
        None
    }
}

impl dyn LoopLikeOpInterface {
    /// If there is a single induction variable return it, otherwise return `None`
    pub fn get_single_induction_var(&self) -> Option<ValueRef> {
        let vars = self.get_loop_induction_vars();
        if let Some([var]) = vars.as_deref() {
            return Some(*var);
        }
        None
    }

    /// Return the single lower bound value or attribute if it exists, otherwise return `None`
    pub fn get_single_lower_bound(&self) -> Option<OpFoldResult> {
        let mut lower_bounds = self.get_loop_lower_bounds()?;
        if lower_bounds.len() == 1 {
            lower_bounds.pop()
        } else {
            None
        }
    }

    /// Return the single upper bound value or attribute if it exists, otherwise return `None`
    pub fn get_single_upper_bound(&self) -> Option<OpFoldResult> {
        let mut upper_bounds = self.get_loop_upper_bounds()?;
        if upper_bounds.len() == 1 {
            upper_bounds.pop()
        } else {
            None
        }
    }

    /// Return the single step value or attribute if it exists, otherwise return `None`
    pub fn get_single_step(&self) -> Option<OpFoldResult> {
        let mut steps = self.get_loop_steps()?;
        if steps.len() == 1 { steps.pop() } else { None }
    }
}
