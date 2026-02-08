use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::operation,
    dialects::builtin::attributes::{BoolAttr, U32Attr},
    effects::*,
    matchers::Matcher,
    patterns::RewritePatternSet,
    print::AsmPrinter,
    smallvec,
    traits::*,
    *,
};

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
        rewrites.push(crate::canonicalization::SimplifyPassthroughBr::new(context.clone()));
        rewrites.push(crate::canonicalization::SimplifyBrToReturn::new(context));
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
        _operands: &[Option<AttributeRef>],
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
        let name = context
            .get_or_register_dialect::<ControlFlowDialect>()
            .expect_registered_name::<Self>();
        rewrites.push(crate::canonicalization::SimplifyPassthroughCondBr::new(context.clone()));
        rewrites.push(crate::canonicalization::SplitCriticalEdges::for_op(context.clone(), name));
        rewrites.push(crate::canonicalization::RemoveUnusedSinglePredBlockArgs::new(context));
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
        operands: &[Option<AttributeRef>],
    ) -> Option<SuccessorInfo> {
        let value = operands[0].as_ref()?.borrow();
        let cond = value.value().downcast_ref::<bool>().copied().unwrap_or_else(|| {
            panic!("expected boolean for '{}' condition, got: {:?}", self.name(), value)
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
    implements(BranchOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Switch {
    #[operand]
    selector: UInt32,
    #[successor]
    fallback: Successor,
    #[successors(keyed)]
    cases: SwitchCase,
}

impl OpPrinter for Switch {
    fn print(&self, _printer: &mut AsmPrinter<'_>) {
        /*
        use formatter::*;
        let header =
            display(self.op.name()) + const_text(" ") + display(self.selector().as_value_ref());
        let cases = self.cases().iter().fold(Document::Empty, |acc, case| {
            let key = case.key().unwrap();
            let dest = case.block();
            let operands = case.arguments();
            let args = if operands.is_empty() {
                Document::Empty
            } else {
                operands.iter().enumerate().fold(const_text("("), |acc, (i, o)| {
                    if i > 0 {
                        acc + const_text(", ") + display(o.borrow().as_value_ref())
                    } else {
                        acc + display(o.borrow().as_value_ref())
                    }
                }) + const_text(")")
            };
            acc + nl()
                + const_text("case ")
                + display(*key)
                + const_text(" => ")
                + display(dest)
                + args
        });
        let fallback = self.fallback();
        let fallback_dest = fallback.successor();
        let fallback_args = if fallback.arguments.is_empty() {
            Document::Empty
        } else {
            fallback.arguments.iter().enumerate().fold(const_text("("), |acc, (i, o)| {
                if i > 0 {
                    acc + const_text(", ") + display(o.borrow().as_value_ref())
                } else {
                    acc + display(o.borrow().as_value_ref())
                }
            }) + const_text(")")
        };
        let fallback = nl() + const_text("default => ") + display(fallback_dest) + fallback_args;
        header
            + const_text(" {")
            + indent(4, cases + fallback)
            + nl()
            + const_text("}")
            + const_text(";")
             */
        todo!()
    }
}

impl Canonicalizable for Switch {
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>) {
        let name = context
            .get_or_register_dialect::<ControlFlowDialect>()
            .expect_registered_name::<Self>();
        rewrites.push(crate::canonicalization::SimplifyCondBrLikeSwitch::new(context.clone()));
        rewrites.push(crate::canonicalization::SimplifySwitchFallbackOverlap::new(context.clone()));
        rewrites.push(crate::canonicalization::SplitCriticalEdges::for_op(context.clone(), name));
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
        operands: &[Option<AttributeRef>],
    ) -> Option<SuccessorInfo> {
        let attr = operands[0].as_ref()?.borrow();
        let selector = if let Some(selector) = attr.downcast_ref::<U32Attr>() {
            *selector.as_value()
        } else if let Some(selector) = attr.as_attr().as_trait::<dyn IntegerLikeAttr>() {
            selector
                .as_immediate()
                .as_u32()
                .expect("invalid selector value for 'cf.switch'")
        } else {
            panic!("unsupported selector value type for '{}', got: {:?}", self.name(), attr)
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
    pub value: UnsafeIntrusiveEntityRef<U32Attr>,
    pub successor: BlockRef,
    pub arguments: Vec<ValueRef>,
}

impl SwitchCase {
    pub fn create(value: u32, successor: BlockRef, arguments: Vec<ValueRef>) -> Self {
        let value = successor.borrow().context_rc().create_attribute::<U32Attr, _>(value);
        Self {
            value,
            successor,
            arguments,
        }
    }
}

#[doc(hidden)]
pub struct SwitchCaseRef<'a> {
    pub value: UnsafeIntrusiveEntityRef<U32Attr>,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRange<'a>,
}

#[doc(hidden)]
pub struct SwitchCaseMut<'a> {
    pub value: UnsafeIntrusiveEntityRef<U32Attr>,
    pub successor: BlockOperandRef,
    pub arguments: OpOperandRangeMut<'a>,
}

impl KeyedSuccessor for SwitchCase {
    type Key = u32;
    type KeyStorage = U32Attr;
    type Repr<'a> = SwitchCaseRef<'a>;
    type ReprMut<'a> = SwitchCaseMut<'a>;

    fn key(&self) -> u32 {
        *self.value.borrow().as_value()
    }

    fn into_parts(self) -> (UnsafeIntrusiveEntityRef<Self::KeyStorage>, BlockRef, Vec<ValueRef>) {
        (self.value, self.successor, self.arguments)
    }

    fn into_repr(
        key: UnsafeIntrusiveEntityRef<Self::KeyStorage>,
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
        key: UnsafeIntrusiveEntityRef<Self::KeyStorage>,
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
        EffectIterator::from_smallvec(smallvec![])
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
            matchers::foldable_operand_of::<BoolAttr>().matches(&self.cond().as_operand_ref())
        {
            let cond = *value.borrow().as_value();
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

        FoldResult::Failed
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(cond) = operands[0]
            .as_ref()
            .and_then(|o| o.borrow().value().downcast_ref::<bool>().copied())
        {
            let maybe_folded = if cond {
                operands[1]
                    .map(OpFoldResult::Attribute)
                    .or_else(|| Some(OpFoldResult::Value(self.first().as_value_ref())))
            } else {
                operands[2]
                    .map(OpFoldResult::Attribute)
                    .or_else(|| Some(OpFoldResult::Value(self.second().as_value_ref())))
            };

            if let Some(folded) = maybe_folded {
                results.push(folded);
                return FoldResult::Ok(());
            }
        }
        FoldResult::Failed
    }
}
