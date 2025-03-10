use alloc::{boxed::Box, rc::Rc, vec::Vec};

use midenc_hir2::{derive::operation, effects::*, matchers::Matcher, traits::*, *};

use crate::ControlFlowDialect;

/// An unstructured control flow primitive representing an unconditional branch to `target`
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface)
)]
pub struct Br {
    #[successor]
    target: Successor,
}

impl Canonicalizable for Br {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites
            .push(crate::canonicalization::SimplifyBrToBlockWithSinglePred::new(context.clone()));
        rewrites.push(crate::canonicalization::SimplifyPassthroughBr::new(context));
    }
}

impl EffectOpInterface<MemoryEffect> for Br {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![])
    }

    fn has_no_effect(&self) -> bool {
        true
    }
}

impl BranchOpInterface for Br {
    #[inline]
    fn get_successor_for_operands(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        Some(self.successors()[0])
    }
}

/// An unstructured control flow primitive representing a conditional branch to either `then_dest`
/// or `else_dest` depending on the value of `condition`, a boolean value.
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface)
)]
pub struct CondBr {
    #[operand]
    condition: Bool,
    #[successor]
    then_dest: Successor,
    #[successor]
    else_dest: Successor,
}

impl Canonicalizable for CondBr {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::SimplifyPassthroughCondBr::new(context.clone()));
        rewrites.push(crate::canonicalization::SplitCriticalEdges::new(context));
    }
}

impl EffectOpInterface<MemoryEffect> for CondBr {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![])
    }

    fn has_no_effect(&self) -> bool {
        true
    }
}

impl BranchOpInterface for CondBr {
    fn get_successor_for_operands(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        let value = operands[0].as_deref()?;
        let cond = value.as_bool().unwrap_or_else(|| {
            panic!("expected boolean immediate for '{}' condition, got: {:?}", self.name(), value)
        });

        Some(if cond {
            self.successors()[0]
        } else {
            self.successors()[1]
        })
    }
}

/// An unstructured control flow primitive that represents a multi-way branch to one of multiple
/// branch targets, depending on the value of `selector`.
///
/// If a specific selector value is matched by `cases`, the branch target corresponding to that
/// case is the one to which control is transferred. If no matching case is found for the selector,
/// then the `fallback` target is used instead.
///
/// A `fallback` successor must always be provided.
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface)
)]
pub struct Switch {
    #[operand]
    selector: UInt32,
    #[successor]
    fallback: Successor,
    #[successors(keyed)]
    cases: SwitchCase,
}

impl Canonicalizable for Switch {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        rewrites.push(crate::canonicalization::SplitCriticalEdges::new(context));
    }
}

impl EffectOpInterface<MemoryEffect> for Switch {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![])
    }

    fn has_no_effect(&self) -> bool {
        true
    }
}

impl BranchOpInterface for Switch {
    #[inline]
    fn get_successor_for_operands(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
    ) -> Option<SuccessorInfo> {
        let value = operands[0].as_deref()?;
        let selector = if let Some(selector) = value.downcast_ref::<Immediate>() {
            selector.as_u32().expect("invalid selector value for 'cf.switch'")
        } else if let Some(selector) = value.downcast_ref::<u32>() {
            *selector
        } else if let Some(selector) = value.downcast_ref::<i32>() {
            u32::try_from(*selector).expect("invalid selector value for 'cf.switch'")
        } else if let Some(selector) = value.downcast_ref::<usize>() {
            u32::try_from(*selector).expect("invalid selector value for 'cf.switch': out of range")
        } else {
            panic!("unsupported selector value type for '{}', got: {:?}", self.name(), value)
        };

        for switch_case in self.cases().iter() {
            let key = *switch_case.key().unwrap();
            if selector == key {
                return Some(*switch_case.info());
            }
        }

        // If we reach here, no selector match was found, so use the fallback successor
        Some(self.successors().all().as_slice().last().copied().unwrap())
    }
}

/// Represents a single branch target by matching a specific selector value in a [Switch]
/// operation.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: BlockRef,
    pub arguments: Vec<ValueRef>,
}

#[doc(hidden)]
pub struct SwitchCaseRef<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRange<'a>,
}

#[doc(hidden)]
pub struct SwitchCaseMut<'a> {
    pub value: u32,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRangeMut<'a>,
}

impl KeyedSuccessor for SwitchCase {
    type Key = u32;
    type Repr<'a> = SwitchCaseRef<'a>;
    type ReprMut<'a> = SwitchCaseMut<'a>;

    fn key(&self) -> &Self::Key {
        &self.value
    }

    fn into_parts(self) -> (Self::Key, BlockRef, Vec<ValueRef>) {
        (self.value, self.successor, self.arguments)
    }

    fn into_repr(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRange<'_>,
    ) -> Self::Repr<'_> {
        SwitchCaseRef {
            value: key,
            successor: block,
            arguments: operands,
        }
    }

    fn into_repr_mut(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRangeMut<'_>,
    ) -> Self::ReprMut<'_> {
        SwitchCaseMut {
            value: key,
            successor: block,
            arguments: operands,
        }
    }
}

/// Choose a value based on a boolean condition
#[operation(
    dialect = ControlFlowDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Select {
    #[operand]
    cond: Bool,
    #[operand]
    first: AnyInteger,
    #[operand]
    second: AnyInteger,
    #[result]
    result: AnyInteger,
}

impl EffectOpInterface<MemoryEffect> for Select {
    fn has_no_effect(&self) -> bool {
        true
    }

    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec::smallvec![])
    }
}

impl InferTypeOpInterface for Select {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.first().ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Select {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.cond().as_operand_ref())
        {
            if let Some(cond) = value.as_bool() {
                let maybe_folded = if cond {
                    matchers::foldable_operand()
                        .matches(&self.first().as_operand_ref())
                        .map(OpFoldResult::Attribute)
                        .or_else(|| Some(OpFoldResult::Value(self.first().as_value_ref())))
                } else {
                    matchers::foldable_operand()
                        .matches(&self.second().as_operand_ref())
                        .map(OpFoldResult::Attribute)
                        .or_else(|| Some(OpFoldResult::Value(self.second().as_value_ref())))
                };

                if let Some(folded) = maybe_folded {
                    results.push(folded);
                    return FoldResult::Ok(());
                }
            }
        }

        FoldResult::Failed
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            if let Some(cond) = value.as_bool() {
                let maybe_folded = if cond {
                    operands[1]
                        .as_deref()
                        .map(|o| OpFoldResult::Attribute(o.clone_value()))
                        .or_else(|| Some(OpFoldResult::Value(self.first().as_value_ref())))
                } else {
                    operands[2]
                        .as_deref()
                        .map(|o| OpFoldResult::Attribute(o.clone_value()))
                        .or_else(|| Some(OpFoldResult::Value(self.second().as_value_ref())))
                };

                if let Some(folded) = maybe_folded {
                    results.push(folded);
                    return FoldResult::Ok(());
                }
            }
        }
        FoldResult::Failed
    }
}
