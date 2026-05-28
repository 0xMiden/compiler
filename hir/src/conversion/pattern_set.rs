use alloc::{boxed::Box, collections::BTreeMap, rc::Rc, vec, vec::Vec};

use smallvec::SmallVec;

use super::ConversionPattern;
use crate::{Context, OperationName, patterns::PatternKind};

/// Mutable collection of conversion patterns.
pub struct ConversionPatternSet {
    context: Rc<Context>,
    patterns: Vec<Box<dyn ConversionPattern>>,
}

impl ConversionPatternSet {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            patterns: vec![],
        }
    }

    pub fn from_iter<P>(context: Rc<Context>, patterns: P) -> Self
    where
        P: IntoIterator<Item = Box<dyn ConversionPattern>>,
    {
        Self {
            context,
            patterns: patterns.into_iter().collect(),
        }
    }

    #[inline]
    pub fn context(&self) -> Rc<Context> {
        Rc::clone(&self.context)
    }

    #[inline]
    pub fn patterns(&self) -> &[Box<dyn ConversionPattern>] {
        &self.patterns
    }

    pub fn push(&mut self, pattern: impl ConversionPattern + 'static) {
        self.patterns.push(Box::new(pattern));
    }

    pub fn extend<P>(&mut self, patterns: P)
    where
        P: IntoIterator<Item = Box<dyn ConversionPattern>>,
    {
        self.patterns.extend(patterns);
    }
}

/// Immutable conversion pattern set indexed by root kind.
pub struct FrozenConversionPatternSet {
    context: Rc<Context>,
    patterns: Vec<Rc<dyn ConversionPattern>>,
    op_specific_patterns: BTreeMap<OperationName, SmallVec<[Rc<dyn ConversionPattern>; 2]>>,
    any_op_patterns: SmallVec<[Rc<dyn ConversionPattern>; 1]>,
}

impl FrozenConversionPatternSet {
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

    #[inline]
    pub fn context(&self) -> Rc<Context> {
        Rc::clone(&self.context)
    }

    #[inline]
    pub fn patterns(&self) -> &[Rc<dyn ConversionPattern>] {
        &self.patterns
    }

    #[inline]
    pub fn op_specific_patterns(
        &self,
    ) -> &BTreeMap<OperationName, SmallVec<[Rc<dyn ConversionPattern>; 2]>> {
        &self.op_specific_patterns
    }

    #[inline]
    pub fn any_op_patterns(&self) -> &[Rc<dyn ConversionPattern>] {
        &self.any_op_patterns
    }
}

pub type PopulateConversionPatternsFn = fn(Rc<Context>, &mut ConversionPatternSet);

/// Inventory-friendly description of a conversion pattern provider.
pub struct ConversionPatternProviderInfo {
    name: &'static str,
    source_dialect: Option<&'static str>,
    target_dialects: &'static [&'static str],
    populate: PopulateConversionPatternsFn,
}

impl ConversionPatternProviderInfo {
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

    #[inline]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    pub const fn source_dialect(&self) -> Option<&'static str> {
        self.source_dialect
    }

    #[inline]
    pub const fn target_dialects(&self) -> &'static [&'static str] {
        self.target_dialects
    }

    #[inline]
    pub fn populate(&self, context: Rc<Context>, patterns: &mut ConversionPatternSet) {
        (self.populate)(context, patterns);
    }

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
