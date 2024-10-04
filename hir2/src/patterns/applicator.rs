use alloc::{collections::BTreeMap, rc::Rc};

use smallvec::SmallVec;

use super::{FrozenRewritePatternSet, PatternBenefit, PatternRewriter, RewritePattern};
use crate::{Builder, OperationName, OperationRef, Report};

/// This type manages the application of a group of rewrite patterns, with a user-provided cost model
pub struct PatternApplicator {
    /// The list that owns the patterns used within this applicator
    rewrite_patterns_set: Rc<FrozenRewritePatternSet>,
    /// The set of patterns to match for each operation, stable sorted by benefit.
    patterns: BTreeMap<OperationName, SmallVec<[Rc<dyn RewritePattern>; 2]>>,
    /// The set of patterns that may match against any operation type, stable sorted by benefit.
    match_any_patterns: SmallVec<[Rc<dyn RewritePattern>; 1]>,
}
impl PatternApplicator {
    pub fn new(rewrite_patterns_set: Rc<FrozenRewritePatternSet>) -> Self {
        Self {
            rewrite_patterns_set,
            patterns: Default::default(),
            match_any_patterns: Default::default(),
        }
    }

    /// Apply a cost model to the patterns within this applicator.
    pub fn apply_cost_model<CostModel>(&mut self, model: CostModel)
    where
        CostModel: Fn(&dyn RewritePattern) -> PatternBenefit,
    {
        // Clear the results computed by the previous cost model
        self.match_any_patterns.clear();
        self.patterns.clear();

        // Filter out op-specific patterns with no benefit, and order by highest benefit first
        let mut benefits = Vec::default();
        for (op, op_patterns) in self.rewrite_patterns_set.op_specific_patterns().iter() {
            benefits
                .extend(op_patterns.iter().filter_map(|p| filter_map_pattern_benefit(p, &model)));
            benefits.sort_by_key(|(_, benefit)| *benefit);
            self.patterns
                .insert(op.clone(), benefits.drain(..).map(|(pat, _)| pat).collect());
        }

        // Filter out "match any" patterns with no benefit, and order by highest benefit first
        benefits.extend(
            self.rewrite_patterns_set
                .any_op_patterns()
                .iter()
                .filter_map(|p| filter_map_pattern_benefit(p, &model)),
        );
        benefits.sort_by_key(|(_, benefit)| *benefit);
        self.match_any_patterns.extend(benefits.into_iter().map(|(pat, _)| pat));
    }

    /// Apply the default cost model that solely uses the pattern's static benefit
    #[inline]
    pub fn apply_default_cost_model(&mut self) {
        self.apply_cost_model(|pattern| pattern.benefit());
    }

    /// Walk all of the patterns within the applicator.
    pub fn walk_all_patterns<F>(&self, mut callback: F)
    where
        F: FnMut(Rc<dyn RewritePattern>),
    {
        for patterns in self.rewrite_patterns_set.op_specific_patterns().values() {
            for pattern in patterns {
                callback(Rc::clone(pattern));
            }
        }
        for pattern in self.rewrite_patterns_set.any_op_patterns() {
            callback(Rc::clone(pattern));
        }
    }

    pub fn match_and_rewrite<A, F, S>(
        &mut self,
        op: OperationRef,
        rewriter: &mut PatternRewriter,
        can_apply: Option<A>,
        mut on_failure: Option<F>,
        mut on_success: Option<S>,
    ) -> Result<(), Report>
    where
        A: Fn(&dyn RewritePattern) -> bool,
        F: FnMut(&dyn RewritePattern),
        S: FnMut(&dyn RewritePattern) -> Result<(), Report>,
    {
        // Check to see if there are patterns matching this specific operation type.
        let op_name = {
            let op = op.borrow();
            op.name()
        };
        let op_specific_patterns = self.patterns.get(&op_name).map(|p| p.as_slice()).unwrap_or(&[]);

        // Process the op-specific patterns and op-agnostic patterns in an interleaved fashion
        let mut op_patterns = op_specific_patterns.iter().peekable();
        let mut any_op_patterns = self.match_any_patterns.iter().peekable();
        loop {
            // Find the next pattern with the highest benefit
            //
            // 1. Start with the assumption that we'll use the next op-specific pattern
            let mut best_pattern = op_patterns.peek().copied();
            // 2. But take the next op-agnostic pattern instead, IF:
            //   a. There are no more op-specific patterns
            //   b. The benefit of the op-agnostic pattern is higher than the op-specific pattern
            if let Some(next_any_pattern) = any_op_patterns
                .next_if(|p| best_pattern.is_none_or(|bp| bp.benefit() < p.benefit()))
            {
                best_pattern.replace(next_any_pattern);
            } else {
                // The op-specific pattern is best, so actually consume it from the iterator
                best_pattern = op_patterns.next();
            }

            // Break if we have exhausted all patterns
            let Some(best_pattern) = best_pattern else {
                break;
            };

            // Can we apply this pattern?
            let applicable = can_apply.as_ref().is_none_or(|can_apply| can_apply(&**best_pattern));
            if !applicable {
                continue;
            }

            // Try to match and rewrite this pattern.
            //
            // The patterns are sorted by benefit, so if we match we can immediately rewrite.
            rewriter.set_insertion_point_before(crate::ProgramPoint::Op(op.clone()));

            // TODO: Save the nearest parent IsolatedFromAbove op of this op for use in debug
            // messages/rendering, as the rewrite may invalidate `op`
            log::debug!("trying to match '{}'", best_pattern.name());

            if best_pattern.match_and_rewrite(op.clone(), rewriter)? {
                log::debug!("successfully matched pattern '{}'", best_pattern.name());
                if let Some(on_success) = on_success.as_mut() {
                    on_success(&**best_pattern)?;
                }
                break;
            } else {
                // Perform any necessary cleanup
                log::debug!("failed to match pattern '{}'", best_pattern.name());
                if let Some(on_failure) = on_failure.as_mut() {
                    on_failure(&**best_pattern);
                }
            }
        }

        Ok(())
    }
}

fn filter_map_pattern_benefit<CostModel>(
    pattern: &Rc<dyn RewritePattern>,
    cost_model: &CostModel,
) -> Option<(Rc<dyn RewritePattern>, PatternBenefit)>
where
    CostModel: Fn(&dyn RewritePattern) -> PatternBenefit,
{
    let benefit = if pattern.benefit().is_impossible_to_match() {
        PatternBenefit::NONE
    } else {
        cost_model(&**pattern)
    };
    if benefit.is_impossible_to_match() {
        log::debug!(
            "ignoring pattern '{}' ({}) because it is impossible to match or cannot lead to legal \
             IR (by cost model)",
            pattern.name(),
            pattern.kind(),
        );
        None
    } else {
        Some((Rc::clone(pattern), benefit))
    }
}
