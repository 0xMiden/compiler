use alloc::{collections::BTreeMap, rc::Rc};

use smallvec::SmallVec;

use super::*;
use crate::{Context, OperationName};

pub struct RewritePatternSet {
    context: Rc<Context>,
    patterns: Vec<Box<dyn RewritePattern>>,
}
impl RewritePatternSet {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            patterns: vec![],
        }
    }

    pub fn from_iter<P>(context: Rc<Context>, patterns: P) -> Self
    where
        P: IntoIterator<Item = Box<dyn RewritePattern>>,
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
    pub fn patterns(&self) -> &[Box<dyn RewritePattern>] {
        &self.patterns
    }

    pub fn push(&mut self, pattern: impl RewritePattern + 'static) {
        self.patterns.push(Box::new(pattern));
    }
}

pub struct FrozenRewritePatternSet {
    context: Rc<Context>,
    patterns: Vec<Rc<dyn RewritePattern>>,
    op_specific_patterns: BTreeMap<OperationName, SmallVec<[Rc<dyn RewritePattern>; 2]>>,
    any_op_patterns: SmallVec<[Rc<dyn RewritePattern>; 1]>,
}
impl FrozenRewritePatternSet {
    pub fn new(patterns: RewritePatternSet) -> Self {
        let RewritePatternSet { context, patterns } = patterns;
        let mut this = Self {
            context,
            patterns: Default::default(),
            op_specific_patterns: Default::default(),
            any_op_patterns: Default::default(),
        };

        for pattern in patterns {
            let pattern = Rc::<dyn RewritePattern>::from(pattern);
            match pattern.kind() {
                PatternKind::Operation(name) => {
                    this.op_specific_patterns
                        .entry(name.clone())
                        .or_default()
                        .push(Rc::clone(&pattern));
                    this.patterns.push(pattern);
                }
                PatternKind::Trait(ref trait_id) => {
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
    pub fn patterns(&self) -> &[Rc<dyn RewritePattern>] {
        &self.patterns
    }

    #[inline]
    pub fn op_specific_patterns(
        &self,
    ) -> &BTreeMap<OperationName, SmallVec<[Rc<dyn RewritePattern>; 2]>> {
        &self.op_specific_patterns
    }

    #[inline]
    pub fn any_op_patterns(&self) -> &[Rc<dyn RewritePattern>] {
        &self.any_op_patterns
    }
}
