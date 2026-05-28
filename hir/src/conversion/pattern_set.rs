use alloc::{boxed::Box, collections::BTreeMap, rc::Rc, vec, vec::Vec};

use smallvec::SmallVec;

use super::ConversionPattern;
use crate::{Context, OperationName, patterns::PatternKind};

/// Mutable collection of conversion patterns.
///
/// Build one of these in a concrete legalization pass, populate it with source-to-target
/// conversion patterns, then pass it to the conversion driver. The driver freezes the set before
/// use so it can index patterns by root operation name.
pub struct ConversionPatternSet {
    context: Rc<Context>,
    patterns: Vec<Box<dyn ConversionPattern>>,
}

impl ConversionPatternSet {
    /// Create an empty pattern set for `context`.
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            patterns: vec![],
        }
    }

    /// Create a pattern set from boxed conversion patterns.
    pub fn from_iter<P>(context: Rc<Context>, patterns: P) -> Self
    where
        P: IntoIterator<Item = Box<dyn ConversionPattern>>,
    {
        Self {
            context,
            patterns: patterns.into_iter().collect(),
        }
    }

    /// Return the context associated with this pattern set.
    #[inline]
    pub fn context(&self) -> Rc<Context> {
        Rc::clone(&self.context)
    }

    /// Return the mutable set's patterns in insertion order.
    #[inline]
    pub fn patterns(&self) -> &[Box<dyn ConversionPattern>] {
        &self.patterns
    }

    /// Add one conversion pattern to this set.
    pub fn push(&mut self, pattern: impl ConversionPattern + 'static) {
        self.patterns.push(Box::new(pattern));
    }

    /// Extend this set with boxed conversion patterns.
    pub fn extend<P>(&mut self, patterns: P)
    where
        P: IntoIterator<Item = Box<dyn ConversionPattern>>,
    {
        self.patterns.extend(patterns);
    }
}

/// Immutable conversion pattern set indexed by root kind.
///
/// Freezing expands trait-rooted patterns to the operations that are registered in the context at
/// freeze time. Callers that rely on trait-rooted conversion patterns must register the relevant
/// dialects before freezing the set.
pub struct FrozenConversionPatternSet {
    context: Rc<Context>,
    patterns: Vec<Rc<dyn ConversionPattern>>,
    op_specific_patterns: BTreeMap<OperationName, SmallVec<[Rc<dyn ConversionPattern>; 2]>>,
    any_op_patterns: SmallVec<[Rc<dyn ConversionPattern>; 1]>,
}

impl FrozenConversionPatternSet {
    /// Freeze and index a mutable conversion pattern set.
    pub fn new(patterns: ConversionPatternSet) -> Self {
        let ConversionPatternSet { context, patterns } = patterns;
        let mut this = Self {
            context,
            patterns: Default::default(),
            op_specific_patterns: Default::default(),
            any_op_patterns: Default::default(),
        };

        for pattern in patterns {
            let pattern = Rc::<dyn ConversionPattern>::from(pattern);
            match pattern.kind() {
                PatternKind::Operation(name) => {
                    this.op_specific_patterns
                        .entry(name.clone())
                        .or_default()
                        .push(Rc::clone(&pattern));
                    this.patterns.push(pattern);
                }
                PatternKind::Trait(trait_id) => {
                    for dialect in this.context.registered_dialects().values() {
                        for op in dialect.registered_ops().iter() {
                            if op.implements_trait_id(trait_id) {
                                this.op_specific_patterns
                                    .entry(op.clone())
                                    .or_default()
                                    .push(Rc::clone(&pattern));
                            }
                        }
                    }
                    this.patterns.push(pattern);
                }
                PatternKind::Any => {
                    this.any_op_patterns.push(Rc::clone(&pattern));
                    this.patterns.push(pattern);
                }
            }
        }

        this
    }

    /// Return the context used to freeze this set.
    #[inline]
    pub fn context(&self) -> Rc<Context> {
        Rc::clone(&self.context)
    }

    /// Return all frozen patterns in insertion order.
    #[inline]
    pub fn patterns(&self) -> &[Rc<dyn ConversionPattern>] {
        &self.patterns
    }

    /// Return patterns indexed by concrete root operation name.
    ///
    /// This includes operation-rooted patterns and trait-rooted patterns expanded against
    /// registered operation metadata.
    #[inline]
    pub fn op_specific_patterns(
        &self,
    ) -> &BTreeMap<OperationName, SmallVec<[Rc<dyn ConversionPattern>; 2]>> {
        &self.op_specific_patterns
    }

    /// Return patterns that may match any operation.
    #[inline]
    pub fn any_op_patterns(&self) -> &[Rc<dyn ConversionPattern>] {
        &self.any_op_patterns
    }
}

/// Callback type used by conversion providers to populate a pattern set.
pub type PopulateConversionPatternsFn = fn(Rc<Context>, &mut ConversionPatternSet);

/// Inventory-friendly description of a conversion pattern provider.
///
/// Providers are lightweight metadata records. Concrete passes may discover registered providers,
/// filter them by source/target dialect metadata, and invoke [`Self::populate`] to add their
/// patterns to a pass-owned [`ConversionPatternSet`].
pub struct ConversionPatternProviderInfo {
    name: &'static str,
    source_dialect: Option<&'static str>,
    target_dialects: &'static [&'static str],
    populate: PopulateConversionPatternsFn,
}

impl ConversionPatternProviderInfo {
    /// Create provider metadata for inventory registration.
    ///
    /// `source_dialect` may be `None` for generic providers. `target_dialects` is descriptive
    /// metadata for pass-level filtering and diagnostics; it does not by itself make any dialect
    /// legal.
    pub const fn new(
        name: &'static str,
        source_dialect: Option<&'static str>,
        target_dialects: &'static [&'static str],
        populate: PopulateConversionPatternsFn,
    ) -> Self {
        Self {
            name,
            source_dialect,
            target_dialects,
            populate,
        }
    }

    /// Return the provider's human-readable name.
    #[inline]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Return the provider's source dialect, if it is specific to one dialect.
    #[inline]
    pub const fn source_dialect(&self) -> Option<&'static str> {
        self.source_dialect
    }

    /// Return the provider's declared target dialect namespaces.
    #[inline]
    pub const fn target_dialects(&self) -> &'static [&'static str] {
        self.target_dialects
    }

    /// Populate `patterns` with this provider's conversion patterns.
    #[inline]
    pub fn populate(&self, context: Rc<Context>, patterns: &mut ConversionPatternSet) {
        (self.populate)(context, patterns);
    }

    /// Iterate over provider metadata registered with `inventory`.
    #[inline]
    pub fn registered() -> impl Iterator<Item = &'static ConversionPatternProviderInfo> {
        inventory::iter::<ConversionPatternProviderInfo>()
    }
}

inventory::collect!(ConversionPatternProviderInfo);

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;
    use core::any::TypeId;

    use crate::{
        Context, DialectRegistration, OperationName, OperationRef, Report, SmallVec, ValueRef,
        conversion::{
            ConversionPattern, ConversionPatternProviderInfo, ConversionPatternRewriter,
            ConversionPatternSet, ConvertedOperands, FrozenConversionPatternSet,
        },
        dialects::test::{Add, Constant, TestDialect},
        patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind},
        traits::ConstantLike,
    };

    struct TestConversionPattern {
        info: PatternInfo,
    }

    impl TestConversionPattern {
        fn new(context: Rc<Context>, name: &'static str, kind: PatternKind) -> Self {
            Self {
                info: PatternInfo::new(context, name, kind, PatternBenefit::new(1)),
            }
        }

        fn with_generated_ops(
            mut self,
            generated_ops: impl IntoIterator<Item = OperationName>,
        ) -> Self {
            self.info.with_generated_ops(generated_ops);
            self
        }
    }

    impl Pattern for TestConversionPattern {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for TestConversionPattern {
        fn match_and_rewrite(
            &self,
            _op: OperationRef,
            _operands: ConvertedOperands<'_>,
            _rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            Ok(false)
        }
    }

    #[test]
    fn freezes_operation_rooted_patterns() {
        let context = Rc::new(Context::default());
        let root = context
            .get_or_register_dialect::<TestDialect>()
            .expect_registered_name::<Constant>();

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-conversion",
            PatternKind::Operation(root.clone()),
        ));

        let frozen = FrozenConversionPatternSet::new(patterns);
        assert_eq!(frozen.patterns().len(), 1);
        assert_eq!(frozen.op_specific_patterns()[&root].len(), 1);
        assert!(frozen.any_op_patterns().is_empty());
    }

    #[test]
    fn expands_trait_rooted_patterns_to_registered_ops() {
        let context = Rc::new(Context::default());
        let dialect = context.get_or_register_dialect::<TestDialect>();
        let constant = dialect.expect_registered_name::<Constant>();
        let add = dialect.expect_registered_name::<Add>();

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-like-conversion",
            PatternKind::Trait(TypeId::of::<dyn ConstantLike>()),
        ));

        let frozen = FrozenConversionPatternSet::new(patterns);
        assert_eq!(frozen.op_specific_patterns()[&constant].len(), 1);
        assert!(!frozen.op_specific_patterns().contains_key(&add));
    }

    #[test]
    fn keeps_any_op_patterns_separate() {
        let context = Rc::new(Context::default());
        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(context, "any-conversion", PatternKind::Any));

        let frozen = FrozenConversionPatternSet::new(patterns);
        assert_eq!(frozen.patterns().len(), 1);
        assert_eq!(frozen.any_op_patterns().len(), 1);
        assert!(frozen.op_specific_patterns().is_empty());
    }

    #[test]
    fn preserves_generated_op_metadata() {
        let context = Rc::new(Context::default());
        let dialect = context.get_or_register_dialect::<TestDialect>();
        let root = dialect.expect_registered_name::<Constant>();
        let generated = dialect.expect_registered_name::<Add>();
        let pattern =
            TestConversionPattern::new(context, "constant-to-add", PatternKind::Operation(root))
                .with_generated_ops([generated.clone()]);

        assert_eq!(pattern.generated_ops(), &[generated]);
    }

    fn populate_test_patterns(context: Rc<Context>, patterns: &mut ConversionPatternSet) {
        let root = context
            .get_or_register_dialect::<TestDialect>()
            .expect_registered_name::<Constant>();
        patterns.push(TestConversionPattern::new(
            context,
            "provider-pattern",
            PatternKind::Operation(root),
        ));
    }

    #[test]
    fn provider_info_populates_pattern_set() {
        let context = Rc::new(Context::default());
        let info = ConversionPatternProviderInfo::new(
            "test-provider",
            Some(TestDialect::NAMESPACE),
            &["test-target"],
            populate_test_patterns,
        );
        let mut patterns = ConversionPatternSet::new(context.clone());

        info.populate(context, &mut patterns);

        assert_eq!(info.name(), "test-provider");
        assert_eq!(info.source_dialect(), Some(TestDialect::NAMESPACE));
        assert_eq!(info.target_dialects(), &["test-target"]);
        assert_eq!(patterns.patterns().len(), 1);
    }

    #[test]
    fn converted_operands_exposes_groups() {
        let groups: [SmallVec<[ValueRef; 2]>; 1] = [SmallVec::new()];
        let operands = ConvertedOperands::new(&groups);

        assert_eq!(operands.len(), 1);
        assert!(operands.get(0).is_some_and(|group| group.is_empty()));
    }

    #[test]
    fn pattern_info_generated_ops_helper_appends() {
        let context = Rc::new(Context::default());
        let dialect = context.get_or_register_dialect::<TestDialect>();
        let root = dialect.expect_registered_name::<Constant>();
        let generated = dialect.expect_registered_name::<Add>();
        let mut info = PatternInfo::new(
            context,
            "generated-op-helper",
            PatternKind::Operation(root),
            PatternBenefit::new(1),
        );

        info.with_generated_ops([generated.clone()]);

        assert_eq!(info.generated_ops(), &[generated]);
    }
}
