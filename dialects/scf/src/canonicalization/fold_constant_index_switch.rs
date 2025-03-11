use alloc::rc::Rc;

use midenc_hir2::*;

use crate::*;

/// A canonicalization pattern for [IndexSwitch] that folds away the operation if it has a constant
/// selector value.
pub struct FoldConstantIndexSwitch {
    info: PatternInfo,
}

impl FoldConstantIndexSwitch {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let switch_op = scf_dialect
            .registered_name::<IndexSwitch>()
            .expect("scf.index_switch is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "fold-constant-index-switch",
                PatternKind::Operation(switch_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl FoldConstantIndexSwitch {
    fn match_constant_selector(&self, op: OperationRef) -> Option<u32> {
        use midenc_hir2::matchers::{self, Matcher};

        let op = op.borrow();
        if let Some(op) = op.downcast_ref::<IndexSwitch>() {
            let selector = op.selector().as_value_ref();
            selector
                .borrow()
                .get_defining_op()
                .and_then(|defined_by| {
                    let matcher = matchers::constant_of::<Immediate>();
                    matcher.matches(&*defined_by.borrow())
                })
                .and_then(|imm| imm.as_u32())
        } else {
            None
        }
    }
}

impl Pattern for FoldConstantIndexSwitch {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for FoldConstantIndexSwitch {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let Some(selector) = self.match_constant_selector(op) else {
            return Ok(false);
        };
        let case_region = {
            let switch_operation = op.borrow();
            let switch_op = switch_operation.downcast_ref::<IndexSwitch>().unwrap();
            let case_index = switch_op.get_case_index_for_selector(selector);
            case_index
                .map(|idx| switch_op.get_case_region(idx))
                .unwrap_or_else(|| switch_op.default_region().as_region_ref())
        };

        let source = case_region.borrow().entry_block_ref().expect("expected non-empty region");
        let terminator =
            source.borrow().terminator().expect("expected region to have a terminator");
        let results = terminator
            .borrow()
            .operands()
            .iter()
            .copied()
            .map(|o| Some(o.borrow().as_value_ref()))
            .collect::<SmallVec<[Option<ValueRef>; 2]>>();

        let dest = op.parent().unwrap();
        rewriter.inline_block_before(source, dest, Some(op), &[]);
        rewriter.erase_op(terminator);
        // Replace the operation with a potentially empty list of results.
        //
        // The fold mechanism doesn't support the case where the result list is empty
        rewriter.replace_op_with_values(op, &results);

        Ok(true)
    }
}
