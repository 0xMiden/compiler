use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::{BoolAttr, U32Attr},
    effects::*,
    matchers::Matcher,
    patterns::RewritePatternSet,
    traits::*,
    *,
};

use crate::ControlFlowDialect;

/// An unstructured control flow primitive representing an unconditional branch to `target`
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface, OpPrinter)
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ControlFlowDialect,
    traits(Terminator),
    implements(BranchOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Switch {
    #[operand]
    selector: UInt32,
    #[successors(keyed)]
    cases: SwitchCase,
    #[successor]
    fallback: Successor,
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
            let key = *switch_case.key();
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
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ControlFlowDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
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

#[cfg(test)]
mod tests {
    use alloc::vec;

    use midenc_expect_test::expect;
    use midenc_hir::{
        Type,
        diagnostics::Report,
        dialects::builtin::Ret,
        testing::{Test, parse_function_fixpoint},
    };

    use super::*;
    use crate::ControlFlowOpBuilder;

    #[test]
    fn switch_building() {
        let mut test = Test::new("foo", &[Type::U32], &[]);
        let context = test.context_rc();
        let selector = test.function().borrow().entry_block().borrow().arguments()[0] as ValueRef;
        let mut builder = test.function_builder();
        let block2 = builder.create_block();
        let block3 = builder.create_block();
        builder.append_block_param(block3, Type::U32, SourceSpan::UNKNOWN);
        let fallback = builder.create_block();
        let cases = vec![
            SwitchCase {
                value: context.create_attribute::<U32Attr, _>(1u32),
                successor: block2,
                arguments: vec![],
            },
            SwitchCase {
                value: context.create_attribute::<U32Attr, _>(2u32),
                successor: block3,
                arguments: vec![selector],
            },
        ];
        let op = builder.switch(selector, cases, fallback, [], SourceSpan::UNKNOWN).unwrap();
        let switch_op = op.borrow();

        assert_eq!(switch_op.fallback().successor(), fallback);
        let cases = switch_op.cases();
        let block2_case = cases.get(0).unwrap();
        assert_eq!(block2_case.block(), block2);
        assert_eq!(*block2_case.key(), 1u32);
        let block3_case = cases.get(1).unwrap();
        assert_eq!(block3_case.block(), block3);
        assert_eq!(*block3_case.key(), 2u32);
        assert_eq!(block3_case.arguments().len(), 1);
        assert_eq!(block3_case.arguments()[0].borrow().as_value_ref(), selector);
    }

    /// Regression test for https://github.com/0xMiden/compiler/issues/1084.
    ///
    /// A `cf.switch` built with an empty `cases` list must still place the fallback successor
    /// in its own group. Previously, the builder routed the fallback into group 0 when the
    /// prior keyed-successor group was empty, causing `Switch::fallback()` to panic with
    /// `index out of bounds` in the accessor for group 1.
    #[test]
    fn switch_building_with_empty_cases() {
        let mut test = Test::new("foo", &[Type::U32], &[]);
        let selector = test.function().borrow().entry_block().borrow().arguments()[0] as ValueRef;
        let mut builder = test.function_builder();
        let fallback = builder.create_block();

        let op = builder.switch(selector, vec![], fallback, [], SourceSpan::UNKNOWN).unwrap();
        let switch_op = op.borrow();

        assert_eq!(switch_op.fallback().successor(), fallback);
        assert_eq!(switch_op.cases().len(), 0);
    }

    /// The value returned by the `builtin.ret` terminating `block`.
    fn returned_value(block: BlockRef) -> ValueRef {
        let block = block.borrow();
        let terminator = block.terminator().expect("expected block to have a terminator");
        let ret = terminator
            .try_downcast_op::<Ret>()
            .expect("expected block to terminate with builtin.ret");
        let ret = ret.borrow();
        let values = ret.values();
        let operand = values.iter().next().expect("expected builtin.ret to have an operand");
        operand.borrow().as_value_ref()
    }

    /// Statically-known successor lists (`cf.cond_br %c ^a, ^b`) parse with the `, ` separator
    /// the printer emits between successors, and the condition operand stays attached to the
    /// operation rather than leaking into the first successor's operand group.
    #[test]
    fn parse_cond_br_with_bare_successors() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @branch(%c: i1, %a: u32, %b: u32) -> u32 {
    cf.cond_br %c ^yes, ^no : (i1);
^yes:
    builtin.ret %a : (u32);
^no:
    builtin.ret %b : (u32);
};";
        let (function, printed) = parse_function_fixpoint(&context, "parse_cond_br.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @branch(%0: i1, %1: u32, %2: u32) -> u32 {
                cf.cond_br %0 ^block2, ^block3 : (i1);
            ^block2:
                builtin.ret %1 : (u32);
            ^block3:
                builtin.ret %2 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let body = function.body();
        let entry = body.entry();
        let arg_c = entry.arguments()[0] as ValueRef;
        let arg_a = entry.arguments()[1] as ValueRef;
        let arg_b = entry.arguments()[2] as ValueRef;

        let cond_br = entry
            .terminator()
            .unwrap()
            .try_downcast_op::<CondBr>()
            .expect("expected entry block to terminate with cf.cond_br");
        let cond_br = cond_br.borrow();
        assert_eq!(cond_br.condition().as_value_ref(), arg_c);
        let then_dest = cond_br.then_dest();
        let else_dest = cond_br.else_dest();
        assert!(then_dest.arguments.is_empty());
        assert!(else_dest.arguments.is_empty());
        assert_eq!(returned_value(then_dest.successor()), arg_a);
        assert_eq!(returned_value(else_dest.successor()), arg_b);

        Ok(())
    }

    /// Successor argument lists (`^block(%v, ...)`) parse and attach to the operand group of
    /// the successor they follow, and the target block's own arguments bind as usual.
    #[test]
    fn parse_cond_br_with_successor_arguments() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @branch_args(%c: i1, %a: u32, %b: u32) -> u32 {
    cf.cond_br %c ^one(%a, u32), ^two(%a, %b, u32, u32) : (i1);
^one(%x: u32):
    builtin.ret %x : (u32);
^two(%y: u32, %z: u32):
    builtin.ret %z : (u32);
};";
        let (function, printed) =
            parse_function_fixpoint(&context, "parse_cond_br_args.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @branch_args(%0: i1, %1: u32, %2: u32) -> u32 {
                cf.cond_br %0 ^block2(%1, u32), ^block3(%1, %2, u32, u32) : (i1);
            ^block2(%3: u32):
                builtin.ret %3 : (u32);
            ^block3(%4: u32, %5: u32):
                builtin.ret %5 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let body = function.body();
        let entry = body.entry();
        let arg_a = entry.arguments()[1] as ValueRef;
        let arg_b = entry.arguments()[2] as ValueRef;

        let cond_br = entry
            .terminator()
            .unwrap()
            .try_downcast_op::<CondBr>()
            .expect("expected entry block to terminate with cf.cond_br");
        let cond_br = cond_br.borrow();

        let then_dest = cond_br.then_dest();
        let then_args = then_dest
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<Vec<_>>();
        assert_eq!(then_args, [arg_a]);

        let else_dest = cond_br.else_dest();
        let else_args = else_dest
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<Vec<_>>();
        assert_eq!(else_args, [arg_a, arg_b]);

        // The successor blocks' own arguments are bound and usable: `^two` returns its second
        // block argument.
        let else_block = else_dest.successor();
        let expected = else_block.borrow().arguments()[1] as ValueRef;
        assert_eq!(returned_value(else_block), expected);

        Ok(())
    }

    /// Block labels register in the parser's block name map when defined: a branch may
    /// reference a block defined earlier in the region (backward reference), and a labeled
    /// definition must unify with targets forward-declared by earlier branches without
    /// dropping the parsed block body.
    #[test]
    fn parse_backward_block_references() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @looped(%start: i1) -> i1 {
    cf.br ^header(%start, i1);
^header(%v: i1):
    cf.cond_br %v ^header(%v, i1), ^exit : (i1);
^exit:
    builtin.ret %v : (i1);
};";
        let (function, printed) =
            parse_function_fixpoint(&context, "parse_backward_refs.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @looped(%0: i1) -> i1 {
                cf.br ^block2(%0, i1);
            ^block2(%1: i1):
                cf.cond_br %1 ^block2(%1, i1), ^block3 : (i1);
            ^block3:
                builtin.ret %1 : (i1);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let body = function.body();
        assert_eq!(body.body().iter().count(), 3, "expected all three blocks to be parsed");

        let entry = body.entry();
        let br = entry
            .terminator()
            .unwrap()
            .try_downcast_op::<Br>()
            .expect("expected entry block to terminate with cf.br");
        let header = br.borrow().target().successor();

        // The labeled definition must populate the forward-declared block, not a placeholder.
        let header_block = header.borrow();
        assert_eq!(header_block.num_arguments(), 1);
        assert_eq!(header_block.body().iter().count(), 1, "header block body must not be empty");

        // The backward reference resolves to that same block, forwarding its argument.
        let cond_br = header_block
            .terminator()
            .unwrap()
            .try_downcast_op::<CondBr>()
            .expect("expected header block to terminate with cf.cond_br");
        let cond_br = cond_br.borrow();
        let then_dest = cond_br.then_dest();
        assert_eq!(then_dest.successor(), header);
        let loop_args = then_dest
            .arguments
            .iter()
            .map(|o| o.borrow().as_value_ref())
            .collect::<Vec<_>>();
        assert_eq!(loop_args, [header_block.arguments()[0] as ValueRef]);

        Ok(())
    }

    /// Keyed successor groups (`cf.switch`) parse: each case key maps to its own successor and
    /// operand group, and the trailing successor is the fallback.
    #[test]
    fn parse_switch_with_keyed_successors() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let source = "\
builtin.function public extern(\"C\") @select(%sel: u32, %a: u32, %b: u32) -> u32 {
    cf.switch %sel #builtin.u32<1> -> ^one(%a, u32), ^fallback : (u32);
^one(%x: u32):
    builtin.ret %x : (u32);
^fallback:
    builtin.ret %b : (u32);
};";
        let (function, printed) = parse_function_fixpoint(&context, "parse_switch.hir", source)?;
        expect![[r#"
            builtin.function public extern("C") @select(%0: u32, %1: u32, %2: u32) -> u32 {
                cf.switch %0 #builtin.u32<1> -> ^block2(%1, u32), ^block3 : (u32);
            ^block2(%3: u32):
                builtin.ret %3 : (u32);
            ^block3:
                builtin.ret %2 : (u32);
            };"#]]
        .assert_eq(&printed);

        let function = function.borrow();
        let body = function.body();
        let entry = body.entry();
        let arg_a = entry.arguments()[1] as ValueRef;
        let arg_b = entry.arguments()[2] as ValueRef;

        let switch_op = entry
            .terminator()
            .unwrap()
            .try_downcast_op::<Switch>()
            .expect("expected entry block to terminate with cf.switch");
        let switch_op = switch_op.borrow();

        let cases = switch_op.cases();
        assert_eq!(cases.len(), 1);
        let case = cases.iter().next().unwrap();
        assert_eq!(*case.key(), 1);
        let case_args =
            case.arguments().iter().map(|o| o.borrow().as_value_ref()).collect::<Vec<_>>();
        assert_eq!(case_args, [arg_a]);
        // The case target returns its own block argument, fed by the case's operand.
        let case_block = case.block();
        let expected = case_block.borrow().arguments()[0] as ValueRef;
        assert_eq!(returned_value(case_block), expected);

        let fallback = switch_op.fallback();
        assert!(fallback.arguments.is_empty());
        assert_eq!(returned_value(fallback.successor()), arg_b);

        Ok(())
    }
}
