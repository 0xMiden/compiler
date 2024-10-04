use alloc::rc::Rc;
use core::{any::TypeId, fmt};

use smallvec::SmallVec;

use super::PatternRewriter;
use crate::{interner, Context, OperationName, OperationRef, Report};

#[derive(Debug)]
pub enum PatternKind {
    /// The pattern root matches any operation
    Any,
    /// The pattern root matches a specific named operation
    Operation(OperationName),
    /// The pattern root matches a specific trait
    Trait(TypeId),
}
impl fmt::Display for PatternKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("for any"),
            Self::Operation(name) => write!(f, "for operation '{name}'"),
            Self::Trait(_) => write!(f, "for trait"),
        }
    }
}

/// Represents the benefit a pattern has.
///
/// More beneficial patterns are preferred over those with lesser benefit, while patterns with no
/// benefit whatsoever can be discarded.
///
/// This is used to evaluate which patterns to apply, and in what order.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct PatternBenefit(Option<core::num::NonZeroU16>);
impl PatternBenefit {
    /// Represents a pattern which is the most beneficial
    pub const MAX: Self = Self(Some(unsafe { core::num::NonZeroU16::new_unchecked(u16::MAX) }));
    /// Represents a pattern which is the least beneficial
    pub const MIN: Self = Self(Some(unsafe { core::num::NonZeroU16::new_unchecked(1) }));
    /// Represents a pattern which can never match, and thus should be discarded
    pub const NONE: Self = Self(None);

    /// Create a new [PatternBenefit] from a raw [u16] value.
    ///
    /// A value of `u16::MAX` is treated as impossible to match, while values from `0..=65534` range
    /// from the least beneficial to the most beneficial.
    pub fn new(benefit: u16) -> Self {
        if benefit == u16::MAX {
            Self(None)
        } else {
            Self(Some(unsafe { core::num::NonZeroU16::new_unchecked(benefit + 1) }))
        }
    }

    /// Returns true if the pattern benefit indicates it can never match
    #[inline]
    pub fn is_impossible_to_match(&self) -> bool {
        self.0.is_none()
    }
}

impl PartialOrd for PatternBenefit {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PatternBenefit {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        match (self.0, other.0) {
            (None, None) => Ordering::Equal,
            // Impossible to match is always last
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            // Benefits are ordered in reverse of integer order (higher benefit appears earlier)
            (Some(a), Some(b)) => a.get().cmp(&b.get()).reverse(),
        }
    }
}

/// A [Pattern] describes all of the data related to a pattern, but does not express any actual
/// pattern logic, i.e. it is solely used for metadata about a pattern.
pub struct Pattern {
    #[allow(unused)]
    context: Rc<Context>,
    name: &'static str,
    kind: PatternKind,
    #[allow(unused)]
    labels: SmallVec<[interner::Symbol; 1]>,
    benefit: PatternBenefit,
    has_bounded_recursion: bool,
    generated_ops: SmallVec<[OperationName; 0]>,
}
impl Pattern {
    /// Create a new [Pattern] from its component parts.
    pub fn new(
        context: Rc<Context>,
        name: &'static str,
        kind: PatternKind,
        benefit: PatternBenefit,
    ) -> Self {
        Self {
            context,
            name,
            kind,
            labels: SmallVec::default(),
            benefit,
            has_bounded_recursion: false,
            generated_ops: SmallVec::default(),
        }
    }

    /// A name used when printing diagnostics related to this pattern
    #[inline(always)]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The kind of value used to select candidate root operations for this pattern.
    #[inline(always)]
    pub const fn kind(&self) -> &PatternKind {
        &self.kind
    }

    /// Returns the benefit - the inverse of "cost" - of matching this pattern.
    ///
    /// The benefit of a [Pattern] is always static - rewrites that may have dynamic benefit can be
    /// instantiated multiple times (different instances), for each benefit that they may return,
    /// and be guarded by different match condition predicates.
    #[inline(always)]
    pub const fn benefit(&self) -> PatternBenefit {
        self.benefit
    }

    /// Return a list of operations that may be generated when rewriting an operation instance
    /// with this pattern.
    #[inline]
    pub fn generated_ops(&self) -> &[OperationName] {
        &self.generated_ops
    }

    /// Return the root operation that this pattern matches.
    ///
    /// Patterns that can match multiple root types return `None`
    pub fn get_root_operation(&self) -> Option<OperationName> {
        match self.kind {
            PatternKind::Operation(ref name) => Some(name.clone()),
            _ => None,
        }
    }

    /// Return the trait id used to match the root operation of this pattern.
    ///
    /// If the pattern does not use a trait id for deciding the root match, this returns `None`
    pub fn get_root_trait(&self) -> Option<TypeId> {
        match self.kind {
            PatternKind::Trait(type_id) => Some(type_id),
            _ => None,
        }
    }

    /// Returns true if this pattern is known to result in recursive application, i.e. this pattern
    /// may generate IR that also matches this pattern, but is known to bound the recursion. This
    /// signals to the rewrite driver that it is safe to apply this pattern recursively to the
    /// generated IR.
    #[inline(always)]
    pub const fn has_bounded_rewrite_recursion(&self) -> bool {
        self.has_bounded_recursion
    }

    /// Set whether or not this pattern has bounded rewrite recursion
    #[inline(always)]
    pub fn with_bounded_rewrite_recursion(&mut self, yes: bool) -> &mut Self {
        self.has_bounded_recursion = yes;
        self
    }
}

/// A [RewritePattern] represents two things:
///
/// * A pattern which matches some IR that we're interested in, typically to replace with something
///   else.
/// * A rewrite which replaces IR that maches the pattern, with new IR, i.e. a DAG-to-DAG
///   replacement
///
/// Implementations must provide `matches` and `rewrite` implementations, from which the
/// `match_and_rewrite` implementation is derived.
pub trait RewritePattern {
    /// A name to use for this pattern in diagnostics
    fn name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }
    /// The pattern used to match candidate root operations for this rewrite.
    fn kind(&self) -> &PatternKind;
    /// The estimated benefit of this pattern
    fn benefit(&self) -> PatternBenefit;
    /// Whether or not this rewrite pattern has bounded recursion
    fn has_bounded_rewrite_recursion(&self) -> bool;
    /// Rewrite the IR rooted at the specified operation with the result of this pattern, generating
    /// any new operations with the specified builder. If an unexpected error is encountered, i.e.
    /// an internal compiler error, it is emitted through the normal diagnostic system, and the IR
    /// is left in a valid state.
    fn rewrite(&self, op: OperationRef, rewriter: &mut PatternRewriter);

    /// Attempt to match this pattern against the IR rooted at the specified operation,
    /// which is the same operation as [Pattern::kind].
    fn matches(&self, op: OperationRef) -> Result<bool, Report>;

    /// Attempt to match this pattern against the IR rooted at the specified operation. If
    /// matching is successful, the rewrite is automatically applied.
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut PatternRewriter,
    ) -> Result<bool, Report> {
        if self.matches(op.clone())? {
            self.rewrite(op, rewriter);

            Ok(true)
        } else {
            Ok(false)
        }
    }
}
