use alloc::rc::Rc;
use core::{any::TypeId, fmt};

use smallvec::SmallVec;

use super::Rewriter;
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

pub trait Pattern {
    fn info(&self) -> &PatternInfo;
    /// A name used when printing diagnostics related to this pattern
    #[inline(always)]
    fn name(&self) -> &'static str {
        self.info().name
    }
    /// The kind of value used to select candidate root operations for this pattern.
    #[inline(always)]
    fn kind(&self) -> &PatternKind {
        &self.info().kind
    }
    /// Returns the benefit - the inverse of "cost" - of matching this pattern.
    ///
    /// The benefit of a [Pattern] is always static - rewrites that may have dynamic benefit can be
    /// instantiated multiple times (different instances), for each benefit that they may return,
    /// and be guarded by different match condition predicates.
    #[inline(always)]
    fn benefit(&self) -> &PatternBenefit {
        &self.info().benefit
    }
    /// Returns true if this pattern is known to result in recursive application, i.e. this pattern
    /// may generate IR that also matches this pattern, but is known to bound the recursion. This
    /// signals to the rewrite driver that it is safe to apply this pattern recursively to the
    /// generated IR.
    #[inline(always)]
    fn has_bounded_rewrite_recursion(&self) -> bool {
        self.info().has_bounded_recursion
    }
    /// Return a list of operations that may be generated when rewriting an operation instance
    /// with this pattern.
    #[inline(always)]
    fn generated_ops(&self) -> &[OperationName] {
        &self.info().generated_ops
    }
    /// Return the root operation that this pattern matches.
    ///
    /// Patterns that can match multiple root types return `None`
    #[inline(always)]
    fn get_root_operation(&self) -> Option<OperationName> {
        self.info().root_operation()
    }
    /// Return the trait id used to match the root operation of this pattern.
    ///
    /// If the pattern does not use a trait id for deciding the root match, this returns `None`
    #[inline(always)]
    fn get_root_trait(&self) -> Option<TypeId> {
        self.info().get_root_trait()
    }
}

/// [PatternBase] describes all of the data related to a pattern, but does not express any actual
/// pattern logic, i.e. it is solely used for metadata about a pattern.
pub struct PatternInfo {
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

impl PatternInfo {
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

    /// Set whether or not this pattern has bounded rewrite recursion
    #[inline(always)]
    pub fn with_bounded_rewrite_recursion(&mut self, yes: bool) -> &mut Self {
        self.has_bounded_recursion = yes;
        self
    }

    /// Return the root operation that this pattern matches.
    ///
    /// Patterns that can match multiple root types return `None`
    pub fn root_operation(&self) -> Option<OperationName> {
        match self.kind {
            PatternKind::Operation(ref name) => Some(name.clone()),
            _ => None,
        }
    }

    /// Return the trait id used to match the root operation of this pattern.
    ///
    /// If the pattern does not use a trait id for deciding the root match, this returns `None`
    pub fn root_trait(&self) -> Option<TypeId> {
        match self.kind {
            PatternKind::Trait(type_id) => Some(type_id),
            _ => None,
        }
    }
}

impl Pattern for PatternInfo {
    #[inline(always)]
    fn info(&self) -> &PatternInfo {
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
pub trait RewritePattern: Pattern {
    /// Rewrite the IR rooted at the specified operation with the result of this pattern, generating
    /// any new operations with the specified builder. If an unexpected error is encountered, i.e.
    /// an internal compiler error, it is emitted through the normal diagnostic system, and the IR
    /// is left in a valid state.
    fn rewrite(&self, op: OperationRef, rewriter: &mut dyn Rewriter);

    /// Attempt to match this pattern against the IR rooted at the specified operation,
    /// which is the same operation as [Pattern::kind].
    fn matches(&self, op: OperationRef) -> Result<bool, Report>;

    /// Attempt to match this pattern against the IR rooted at the specified operation. If
    /// matching is successful, the rewrite is automatically applied.
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        if self.matches(op.clone())? {
            self.rewrite(op, rewriter);

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use pretty_assertions::{assert_eq, assert_str_eq};

    use super::*;
    use crate::{dialects::hir::*, *};

    /// In Miden, `n << 1` is vastly inferior to `n * 2` in cost, so reverse it
    ///
    /// NOTE: These two ops have slightly different semantics, a real implementation would have
    /// to handle the edge cases.
    struct ConvertShiftLeftBy1ToMultiply {
        info: PatternInfo,
    }
    impl ConvertShiftLeftBy1ToMultiply {
        pub fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<HirDialect>();
            let op_name = <Shl as crate::OpRegistration>::register_with(&*dialect);
            let mut info = PatternInfo::new(
                context,
                "convert-shl1-to-mul2",
                PatternKind::Operation(op_name),
                PatternBenefit::new(1),
            );
            info.with_bounded_rewrite_recursion(true);
            Self { info }
        }
    }
    impl Pattern for ConvertShiftLeftBy1ToMultiply {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }
    impl RewritePattern for ConvertShiftLeftBy1ToMultiply {
        fn matches(&self, op: OperationRef) -> Result<bool, Report> {
            use crate::matchers::{self, match_chain, match_op, MatchWith, Matcher};

            let binder = MatchWith(|op: &UnsafeIntrusiveEntityRef<Shl>| {
                log::trace!(
                    "found matching 'hir.shl' operation, checking if `shift` operand is foldable"
                );
                let op = op.borrow();
                let shift = op.shift().as_operand_ref();
                let matched = matchers::foldable_operand_of::<Immediate>().matches(&shift);
                matched.and_then(|imm| {
                    log::trace!("`shift` operand is an immediate: {imm}");
                    let imm = imm.as_u64();
                    if imm.is_none() {
                        log::trace!("`shift` operand is not a valid u64 value");
                    }
                    if imm.is_some_and(|imm| imm == 1) {
                        Some(())
                    } else {
                        None
                    }
                })
            });
            log::trace!("attempting to match '{}'", self.name());
            let matched = match_chain(match_op::<Shl>(), binder).matches(&op.borrow()).is_some();
            log::trace!("'{}' matched: {matched}", self.name());
            Ok(matched)
        }

        fn rewrite(&self, op: OperationRef, rewriter: &mut dyn Rewriter) {
            log::trace!("found match, rewriting '{}'", op.borrow().name());
            let (span, lhs) = {
                let shl = op.borrow();
                let shl = shl.downcast_ref::<Shl>().unwrap();
                let span = shl.span();
                let lhs = shl.lhs().as_value_ref();
                (span, lhs)
            };
            let constant_builder = rewriter.create::<Constant, _>(span);
            let constant: UnsafeIntrusiveEntityRef<Constant> =
                constant_builder(Immediate::U32(2)).unwrap();
            let shift = constant.borrow().result().as_value_ref();
            let mul_builder = rewriter.create::<Mul, _>(span);
            let mul = mul_builder(lhs, shift, Overflow::Wrapping).unwrap();
            let mul = mul.borrow().as_operation().as_operation_ref();
            log::trace!("replacing shl with mul");
            rewriter.replace_op(op, mul);
        }
    }

    #[test]
    fn rewrite_pattern_api_test() {
        let mut builder = env_logger::Builder::from_env("MIDENC_TRACE");
        builder.init();

        let context = Rc::new(Context::default());
        let pattern = ConvertShiftLeftBy1ToMultiply::new(Rc::clone(&context));

        let mut builder = OpBuilder::new(Rc::clone(&context));
        let mut function = {
            let builder = builder.create::<Function, (_, _)>(SourceSpan::default());
            let id = Ident::new("test".into(), SourceSpan::default());
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(id, signature).unwrap()
        };

        // Define function body
        {
            let mut func = function.borrow_mut();
            let mut builder = FunctionBuilder::new(&mut func);
            let shift = builder.ins().u32(1, SourceSpan::default()).unwrap();
            let block = builder.current_block();
            let lhs = block.borrow().arguments()[0].clone().upcast();
            let result = builder.ins().shl(lhs, shift, SourceSpan::default()).unwrap();
            builder.ins().ret(Some(result), SourceSpan::default()).unwrap();
        }

        // Construct pattern set
        let mut rewrites = RewritePatternSet::new(builder.context_rc());
        rewrites.push(pattern);
        let rewrites = Rc::new(FrozenRewritePatternSet::new(rewrites));

        // Execute pattern driver
        let mut config = GreedyRewriteConfig::default();
        config.with_region_simplification_level(RegionSimplificationLevel::None);
        let result = crate::apply_patterns_and_fold_greedily(
            function.borrow().as_operation().as_operation_ref(),
            rewrites,
            config,
        );

        // The rewrite should converge and modify the IR
        assert_eq!(result, Ok(true));

        // Confirm that the expected rewrite occurred
        let func = function.borrow();
        let output = func.as_operation().to_string();
        let expected = "\
hir.function public @test(v0: u32) -> u32 {
^block0(v0: u32):
    v1 = hir.constant 1 : u32;
    v3 = hir.constant 2 : u32;
    v4 = hir.mul v0, v3 : u32 #[overflow = wrapping];
    hir.ret v4;
};";
        assert_str_eq!(output.as_str(), expected);
    }
}
