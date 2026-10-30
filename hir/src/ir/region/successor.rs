use core::fmt;

use self::value::BlockArgumentRange;
use super::*;
use crate::ValueRange;

/// This struct represents the metadata about a [RegionBranchOpInterface] successor, without
/// borrowing the op or the successor.
#[derive(Debug, Clone)]
pub enum RegionSuccessorInfo {
    /// We're entering the given region, and its successor operands are the block arguments of its
    /// entry block.
    Entering(RegionRef),
    /// We're exiting/returning to the parent op from one of its child regions.
    ///
    /// The given result group index is used to obtain the successor operands from the results of
    /// the terminator operation which transfers control to the parent.
    Returning(SmallVec<[ValueRef; 2]>),
    /// Control leaves this region AND every intermediate ancestor op, up to `target`, which
    /// receives it.
    ///
    /// `to` says whether the target is re-entered at `Child(header)` (a `continue`) or left at
    /// `Parent` (a `break`). `values` are the successor inputs *at the target*: the target's
    /// results for a break, the header region's block arguments for a continue.
    ///
    /// NOTE: Unlike [Self::Entering] and [Self::Returning], which are both relative to the
    /// *immediate* parent op, this variant names the ancestor it addresses. A consumer that
    /// treats `to` as relative to the immediate parent will attribute the edge to the wrong
    /// operation; use [RegionSuccessor::owner] to get the op this edge actually describes.
    Exiting {
        /// The ancestor operation which receives this exit
        target: OperationRef,
        /// Where control resumes at `target`
        to: RegionBranchPoint,
        /// The successor inputs at `target`
        values: SmallVec<[ValueRef; 2]>,
    },
}
impl RegionSuccessorInfo {
    /// The point control transfers to.
    ///
    /// NOTE: For [Self::Exiting] this point is relative to the addressed ancestor, *not* to the
    /// immediate parent op. See [Self::target].
    pub fn successor(&self) -> RegionBranchPoint {
        match self {
            Self::Entering(region) => RegionBranchPoint::Child(*region),
            Self::Returning(_) => RegionBranchPoint::Parent,
            Self::Exiting { to, .. } => *to,
        }
    }

    /// The operation whose control flow this edge describes.
    ///
    /// Returns `None` for [Self::Entering] and [Self::Returning], whose owner is the immediate
    /// parent op and is therefore implied by context; returns the addressed ancestor for
    /// [Self::Exiting].
    pub fn target(&self) -> Option<OperationRef> {
        match self {
            Self::Entering(_) | Self::Returning(_) => None,
            Self::Exiting { target, .. } => Some(*target),
        }
    }

    /// Returns true if this edge leaves more than the immediate parent op.
    #[inline]
    pub fn is_exiting(&self) -> bool {
        matches!(self, Self::Exiting { .. })
    }
}

/// A [RegionSuccessor] represents the successor of a region.
///
///
/// A region successor can either be another region, or the parent operation. If the successor is a
/// region, this class represents the destination region, as well as a set of arguments from that
/// region that will be populated when control flows into the region. If the successor is the parent
/// operation, this class represents an optional set of results that will be populated when control
/// returns to the parent operation.
///
/// This interface assumes that the values from the current region that are used to populate the
/// successor inputs are the operands of the return-like terminator operations in the blocks within
/// this region.
pub struct RegionSuccessor<'a> {
    dest: RegionBranchPoint,
    owner: Option<OperationRef>,
    is_exiting: bool,
    arguments: ValueRange<'a>,
}

/// Where a [RegionSuccessor] transfers control to.
///
/// This is the three-valued question `RegionSuccessor` used to answer with an `Option<RegionRef>`,
/// whose `None` meant "control returns to the immediate parent". That encoding has no word for an
/// early exit — control leaving the parent AND every intermediate op — which is exactly why the
/// third case is a variant here rather than a flag a consumer may forget to consult.
#[derive(Debug, Copy, Clone)]
pub enum RegionSuccessorKind {
    /// Control enters `region`, a region of this successor's owner.
    Entering(RegionRef),
    /// Control returns to this successor's owner — the immediate parent op.
    Returning,
    /// Control leaves the immediate parent AND every intermediate op up to `target`, resuming at
    /// `to`: [RegionBranchPoint::Parent] for a break (leaving `target`, its results receiving the
    /// successor inputs) or [RegionBranchPoint::Child] for a continue (re-entering `target`'s
    /// header, whose block arguments receive them).
    ///
    /// **`target` is not the immediate parent.** A consumer that anchors this edge at the parent
    /// records it against the wrong operation, and nothing crashes when it does.
    Exiting {
        target: OperationRef,
        to: RegionBranchPoint,
    },
}

impl<'a> RegionSuccessor<'a> {
    /// Creates a [RegionSuccessor] representing a branch to `dest` with `arguments`
    pub fn new(dest: RegionBranchPoint, arguments: impl Into<ValueRange<'a>>) -> Self {
        Self {
            dest,
            owner: None,
            is_exiting: false,
            arguments: arguments.into(),
        }
    }

    /// Creates a [RegionSuccessor] representing a branch to another region of the parent operation.
    pub fn child(region: RegionRef, inputs: BlockArgumentRange<'a>) -> Self {
        Self {
            dest: RegionBranchPoint::Child(region),
            owner: None,
            is_exiting: false,
            arguments: inputs.into(),
        }
    }

    /// Creates a [RegionSuccessor] representing a branch to/out of the parent operation.
    pub fn parent(inputs: OpResultRange<'a>) -> Self {
        Self {
            dest: RegionBranchPoint::Parent,
            owner: None,
            is_exiting: false,
            arguments: inputs.into(),
        }
    }

    /// Creates a [RegionSuccessor] representing an early exit out of `target`, an ancestor of the
    /// region this successor belongs to, past every intermediate op.
    pub fn exiting(
        target: OperationRef,
        to: RegionBranchPoint,
        inputs: impl Into<ValueRange<'a>>,
    ) -> Self {
        Self {
            dest: to,
            owner: Some(target),
            is_exiting: true,
            arguments: inputs.into(),
        }
    }

    /// Records the operation whose control flow this successor describes.
    pub fn owned_by(mut self, owner: OperationRef) -> Self {
        self.owner = Some(owner);
        self
    }

    /// The operation that OWNS this successor — the op whose control flow this edge describes.
    ///
    /// For `Entering`/`Returning` this is the immediate parent (today's silent assumption, made
    /// explicit); for `Exiting` it is the addressed ancestor, which is *not* the immediate parent.
    ///
    /// Returns `None` only for a successor built by hand without an owner, which today means the
    /// synthetic placeholders a sparse analysis constructs when it cannot reason about a region's
    /// control flow at all; those pass the owning op separately.
    ///
    /// **A consumer that resolves an edge against the immediate parent rather than against this
    /// op will attribute an early exit to the wrong operation.**
    #[inline]
    pub fn owner(&self) -> Option<OperationRef> {
        self.owner
    }

    /// Get the underlying [RegionBranchPoint]
    ///
    /// NOTE: This point is relative to [Self::owner], which for an early exit is an ancestor
    /// rather than the immediate parent.
    pub fn branch_point(&self) -> &RegionBranchPoint {
        &self.dest
    }

    /// Where this successor transfers control to, and whose control flow it describes.
    ///
    /// **This accessor is three-valued on purpose.** It replaced an `Option<RegionRef>` pair
    /// (`successor`/`into_successor`) whose `None` meant "returns to the immediate parent" — a
    /// two-valued encoding of the same question [RegionSuccessorInfo] used to ask, and therefore
    /// the place an early exit would have passed through every consumer silently. Matching on
    /// this type is what makes a consumer that has not considered an early exit a compile error
    /// rather than a wrong answer.
    pub fn kind(&self) -> RegionSuccessorKind {
        match (self.owner, self.dest) {
            // An owner is only recorded for an exit when the owner is NOT the immediate parent;
            // `RegionSuccessorIter::get` records the parent for the other two variants, and a
            // hand-built successor records nothing.
            (Some(target), to) if self.is_exiting => RegionSuccessorKind::Exiting { target, to },
            (_, RegionBranchPoint::Child(region)) => RegionSuccessorKind::Entering(region),
            (_, RegionBranchPoint::Parent) => RegionSuccessorKind::Returning,
        }
    }

    /// Returns true if this edge leaves more than the immediate parent op.
    #[inline]
    pub fn is_exiting(&self) -> bool {
        self.is_exiting
    }

    /// The region this successor enters *within the owner's own region graph*, if any.
    ///
    /// This is the accessor for walkers of a single op's region graph — `traverse_region_graph`,
    /// `postorder_region_graph`, `has_loop`, entry-label elision. It answers `None` for
    /// [RegionSuccessorKind::Returning] (control leaves the op) and for **every**
    /// [RegionSuccessorKind::Exiting], including a `continue`, because an early exit is a
    /// cross-op edge: its target region belongs to an ancestor, not to the op whose graph is
    /// being walked.
    ///
    /// **This is the design's record-and-stop rule**: an op's region graph stays that op's graph,
    /// and the cross-op edge is visible only through [Self::kind]. A walker that followed an
    /// `Exiting` edge would leave the graph it was asked about, and its `visited` set — keyed by
    /// region — would be comparing regions from two different ops.
    ///
    /// NOTE: No [RegionBranchOpInterface] implementation produces an `Exiting` successor today;
    /// early exits are described by the *terminator* interface on the exit op itself. The arm
    /// exists so that a receiver which later chose to describe its own back edge that way stops
    /// here instead of walking into an ancestor.
    #[inline]
    pub fn region_graph_successor(&self) -> Option<RegionRef> {
        match self.kind() {
            RegionSuccessorKind::Entering(region) => Some(region),
            RegionSuccessorKind::Returning | RegionSuccessorKind::Exiting { .. } => None,
        }
    }

    /// Return the inputs to the successor that are remapped by the exit values of the current
    /// region.
    pub fn successor_inputs(&self) -> &ValueRange<'a> {
        &self.arguments
    }
}

impl fmt::Debug for RegionSuccessor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegionSuccessor")
            .field("dest", &self.dest)
            .field_with("owner", |f| match self.owner {
                None => f.write_str("<implied: immediate parent>"),
                Some(owner) => write!(f, "{}", owner.borrow().name()),
            })
            .field_with("arguments", |f| {
                let mut list = f.debug_list();
                for operand in self.arguments.iter() {
                    list.entry(&operand.borrow());
                }
                list.finish()
            })
            .finish()
    }
}

pub struct RegionSuccessorIter<'a> {
    // TODO(pauls): See if we can get rid of [RegionSuccessorInfo] entirely and just use
    // [RegionSuccessor]
    op: &'a Operation,
    successors: SmallVec<[RegionSuccessorInfo; 2]>,
    index: usize,
}
impl<'a> RegionSuccessorIter<'a> {
    pub fn new(
        op: &'a Operation,
        successors: impl IntoIterator<Item = RegionSuccessorInfo>,
    ) -> Self {
        Self {
            op,
            successors: SmallVec::from_iter(successors),
            index: 0,
        }
    }

    pub fn empty(op: &'a Operation) -> Self {
        Self {
            op,
            successors: Default::default(),
            index: 0,
        }
    }

    pub fn get(&self, index: usize) -> Option<RegionSuccessor<'a>> {
        self.successors.get(index).map(|info| match info {
            RegionSuccessorInfo::Entering(region) => {
                let operands = ValueRange::Owned(
                    region
                        .borrow()
                        .entry()
                        .arguments()
                        .iter()
                        .map(|arg| arg.borrow().as_value_ref())
                        .collect(),
                );
                RegionSuccessor::new(RegionBranchPoint::Child(*region), operands)
                    .owned_by(self.op.as_operation_ref())
            }
            RegionSuccessorInfo::Returning(results) => {
                RegionSuccessor::new(RegionBranchPoint::Parent, results.clone())
                    .owned_by(self.op.as_operation_ref())
            }
            RegionSuccessorInfo::Exiting { target, to, values } => {
                RegionSuccessor::exiting(*target, *to, values.clone())
            }
        })
    }

    pub fn into_successor_infos(self) -> SmallVec<[RegionSuccessorInfo; 2]> {
        self.successors
    }
}
impl core::iter::FusedIterator for RegionSuccessorIter<'_> {}
impl ExactSizeIterator for RegionSuccessorIter<'_> {
    fn len(&self) -> usize {
        self.successors.len()
    }
}
impl<'a> Iterator for RegionSuccessorIter<'a> {
    type Item = RegionSuccessor<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.successors.len() {
            return None;
        }

        let next = self.get(self.index)?;

        self.index += 1;

        Some(next)
    }
}
