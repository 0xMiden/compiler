use alloc::{collections::BTreeMap, rc::Rc, vec::Vec};

use smallvec::SmallVec;

use super::{ConversionPattern, ConversionTarget, FrozenConversionPatternSet, StaticLegality};
use crate::OperationName;

/// Pattern graph used to decide which conversions may reach a target.
///
/// The graph is built from a conversion target and a frozen pattern set. It keeps only patterns
/// whose declared generated operations can themselves reach terminal legal operations, which lets
/// the driver orchestrate transitive conversion such as A -> B -> C when only C is legal.
///
/// This is primarily driver infrastructure, but it is public so tests, diagnostics, and future
/// tooling can inspect legalization reachability.
pub struct LegalizationGraph<'a> {
    target: &'a ConversionTarget,
    legalizer_patterns: BTreeMap<OperationName, SmallVec<[Rc<dyn ConversionPattern>; 2]>>,
    any_op_patterns: SmallVec<[Rc<dyn ConversionPattern>; 1]>,
}

impl<'a> LegalizationGraph<'a> {
    /// Build a legalization graph for `target` using the already-frozen `patterns`.
    ///
    /// Any-op patterns are conservatively retained for all roots because their generated
    /// operation set is not root-specific. Operation-specific patterns are retained only when all
    /// declared generated operations are legalizable.
    pub fn new(target: &'a ConversionTarget, patterns: &FrozenConversionPatternSet) -> Self {
        let any_op_patterns = patterns.any_op_patterns().iter().cloned().collect();
        let mut this = Self {
            target,
            legalizer_patterns: Default::default(),
            any_op_patterns,
        };

        if !this.any_op_patterns.is_empty() {
            for (root, patterns) in patterns.op_specific_patterns().iter() {
                this.legalizer_patterns.insert(root.clone(), patterns.iter().cloned().collect());
            }
            this.sort_legalizer_patterns();
            return this;
        }

        let mut changed = true;
        while changed {
            changed = false;
            for (root, patterns) in patterns.op_specific_patterns().iter() {
                if matches!(target.static_legality(root), StaticLegality::Legal) {
                    continue;
                }

                for pattern in patterns {
                    if this.has_pattern(root, pattern) {
                        continue;
                    }
                    if this.pattern_may_legalize(pattern.as_ref()) {
                        this.legalizer_patterns
                            .entry(root.clone())
                            .or_default()
                            .push(Rc::clone(pattern));
                        changed = true;
                    }
                }
            }
        }

        this.sort_legalizer_patterns();
        this
    }

    /// Return true when `name` is legal, dynamically legal, or has at least one possible
    /// legalization pattern.
    #[inline]
    pub fn is_legalizable(&self, name: &OperationName) -> bool {
        target_is_terminal(self.target, name)
            || self.legalizer_patterns.contains_key(name)
            || !self.any_op_patterns.is_empty()
    }

    /// Return operation-specific legalizer patterns for `name`.
    ///
    /// The returned patterns are sorted by estimated legalization depth and then pattern benefit.
    #[inline]
    pub fn legalizer_patterns(&self, name: &OperationName) -> &[Rc<dyn ConversionPattern>] {
        self.legalizer_patterns
            .get(name)
            .map(|patterns| patterns.as_slice())
            .unwrap_or(&[])
    }

    /// Return any-op conversion patterns retained by the graph.
    #[inline]
    pub fn any_op_patterns(&self) -> &[Rc<dyn ConversionPattern>] {
        &self.any_op_patterns
    }

    fn has_pattern(&self, root: &OperationName, pattern: &Rc<dyn ConversionPattern>) -> bool {
        self.legalizer_patterns
            .get(root)
            .is_some_and(|patterns| patterns.iter().any(|existing| Rc::ptr_eq(existing, pattern)))
    }

    fn pattern_may_legalize(&self, pattern: &dyn ConversionPattern) -> bool {
        pattern.generated_ops().iter().all(|generated| self.is_legalizable(generated))
    }

    fn sort_legalizer_patterns(&mut self) {
        let roots = self.legalizer_patterns.keys().cloned().collect::<Vec<_>>();
        for root in roots {
            let mut patterns = self.legalizer_patterns.remove(&root).unwrap();
            patterns.sort_by(|lhs, rhs| {
                let lhs_depth = self.pattern_depth(lhs.as_ref());
                let rhs_depth = self.pattern_depth(rhs.as_ref());
                lhs_depth.cmp(&rhs_depth).then_with(|| lhs.benefit().cmp(rhs.benefit()))
            });
            self.legalizer_patterns.insert(root, patterns);
        }
    }

    fn pattern_depth(&self, pattern: &dyn ConversionPattern) -> u32 {
        pattern
            .generated_ops()
            .iter()
            .map(|generated| self.legalization_depth(generated, &mut Vec::new()))
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    fn legalization_depth(&self, name: &OperationName, stack: &mut Vec<OperationName>) -> u32 {
        if target_is_terminal(self.target, name) {
            return 0;
        }
        if stack.iter().any(|active| active == name) {
            return u32::MAX / 2;
        }

        let Some(patterns) = self.legalizer_patterns.get(name) else {
            return u32::MAX / 2;
        };

        stack.push(name.clone());
        let depth = patterns
            .iter()
            .map(|pattern| {
                pattern
                    .generated_ops()
                    .iter()
                    .map(|generated| self.legalization_depth(generated, stack))
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1)
            })
            .min()
            .unwrap_or(u32::MAX / 2);
        stack.pop();
        depth
    }
}

fn target_is_terminal(target: &ConversionTarget, name: &OperationName) -> bool {
    matches!(target.static_legality(name), StaticLegality::Legal | StaticLegality::Dynamic)
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use super::*;
    use crate::{
        Context, OperationRef, Report,
        conversion::{
            ConversionPatternRewriter, ConversionPatternSet, ConvertedOperands,
            DynamicLegalityResult,
        },
        dialects::test::{Add, Constant, Mul, TestDialect},
        patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind},
    };

    struct TestConversionPattern {
        info: PatternInfo,
    }

    impl TestConversionPattern {
        fn new(
            context: Rc<Context>,
            name: &'static str,
            root: OperationName,
            generated_ops: impl IntoIterator<Item = OperationName>,
            benefit: u16,
        ) -> Self {
            let mut info = PatternInfo::new(
                context,
                name,
                PatternKind::Operation(root),
                PatternBenefit::new(benefit),
            );
            info.with_generated_ops(generated_ops);
            Self { info }
        }

        fn any(context: Rc<Context>) -> Self {
            Self {
                info: PatternInfo::new(
                    context,
                    "any-conversion",
                    PatternKind::Any,
                    PatternBenefit::new(1),
                ),
            }
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

    fn names(context: &Rc<Context>) -> (OperationName, OperationName, OperationName) {
        let dialect = context.get_or_register_dialect::<TestDialect>();
        (
            dialect.expect_registered_name::<Constant>(),
            dialect.expect_registered_name::<Add>(),
            dialect.expect_registered_name::<Mul>(),
        )
    }

    #[test]
    fn resolves_transitive_legalization_paths() {
        let context = Rc::new(Context::default());
        let (constant, add, mul) = names(&context);
        let mut target = ConversionTarget::new(context.clone());
        target.add_legal_op::<Mul>();

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add",
            constant.clone(),
            [add.clone()],
            1,
        ));
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "add-to-mul",
            add.clone(),
            [mul],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(graph.is_legalizable(&constant));
        assert!(graph.is_legalizable(&add));
        assert_eq!(graph.legalizer_patterns(&constant).len(), 1);
    }

    #[test]
    fn rejects_paths_with_no_terminal_target() {
        let context = Rc::new(Context::default());
        let (constant, add, _) = names(&context);
        let target = ConversionTarget::new(context.clone());

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add",
            constant.clone(),
            [add],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(!graph.is_legalizable(&constant));
        assert!(graph.legalizer_patterns(&constant).is_empty());
    }

    #[test]
    fn rejects_cycles_without_a_terminal_target() {
        let context = Rc::new(Context::default());
        let (constant, add, _) = names(&context);
        let target = ConversionTarget::new(context.clone());

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add",
            constant.clone(),
            [add.clone()],
            1,
        ));
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "add-to-constant",
            add,
            [constant.clone()],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(!graph.is_legalizable(&constant));
    }

    #[test]
    fn treats_dynamic_legality_as_conditional_terminal() {
        let context = Rc::new(Context::default());
        let (constant, add, _) = names(&context);
        let mut target = ConversionTarget::new(context.clone());
        target.add_dynamically_legal_op::<Add, _>(|_| DynamicLegalityResult::legal());

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add",
            constant.clone(),
            [add],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(graph.is_legalizable(&constant));
    }

    #[test]
    fn retains_patterns_rooted_on_dynamic_legality() {
        let context = Rc::new(Context::default());
        let (_, add, mul) = names(&context);
        let mut target = ConversionTarget::new(context.clone());
        target
            .add_dynamically_legal_op::<Add, _>(|_| DynamicLegalityResult::illegal())
            .add_legal_op::<Mul>();

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "add-to-mul",
            add.clone(),
            [mul],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(graph.is_legalizable(&add));
        assert_eq!(graph.legalizer_patterns(&add).len(), 1);
    }

    #[test]
    fn any_op_patterns_keep_rooted_patterns_conservatively() {
        let context = Rc::new(Context::default());
        let (constant, add, _) = names(&context);
        let target = ConversionTarget::new(context.clone());

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::any(context.clone()));
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add",
            constant.clone(),
            [add],
            1,
        ));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);

        assert!(graph.is_legalizable(&constant));
        assert_eq!(graph.any_op_patterns().len(), 1);
        assert_eq!(graph.legalizer_patterns(&constant).len(), 1);
    }

    #[test]
    fn orders_by_depth_before_benefit() {
        let context = Rc::new(Context::default());
        let (constant, add, mul) = names(&context);
        let mut target = ConversionTarget::new(context.clone());
        target.add_legal_op::<Mul>();

        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-add-high-benefit",
            constant.clone(),
            [add.clone()],
            10,
        ));
        patterns.push(TestConversionPattern::new(
            context.clone(),
            "constant-to-mul-low-benefit",
            constant.clone(),
            [mul.clone()],
            1,
        ));
        patterns.push(TestConversionPattern::new(context.clone(), "add-to-mul", add, [mul], 1));
        let frozen = FrozenConversionPatternSet::new(patterns);

        let graph = LegalizationGraph::new(&target, &frozen);
        let candidates = graph.legalizer_patterns(&constant);

        assert_eq!(candidates[0].name(), "constant-to-mul-low-benefit");
        assert_eq!(candidates[1].name(), "constant-to-add-high-benefit");
    }
}
