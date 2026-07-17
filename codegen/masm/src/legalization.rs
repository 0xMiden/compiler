use alloc::rc::Rc;

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    Context, EntityMut, Operation, OperationName, Report,
    conversion::{
        ConversionConfig, ConversionPatternSet, ConversionTarget, DynamicLegalityResult,
        apply_full_conversion,
    },
    dialects::{builtin, debuginfo},
    pass::{Pass, PassExecutionState, PostPassStatus},
};

use crate::HirLowering;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<LegalizeForMasm>(
    LegalizeForMasm::ARGUMENT,
    "legalize HIR for MASM codegen"
));

/// A dialect conversion pass that validates IR against the set of operations MASM codegen can
/// lower.
///
/// This pass is intentionally owned by `midenc-codegen-masm`: it builds the MASM-specific
/// legalization target, runs full dialect conversion, and fails before `ToMasmComponent` can
/// encounter unsupported operations.
#[derive(Default)]
pub struct LegalizeForMasm;

impl LegalizeForMasm {
    /// Command-line/pass-pipeline argument for this pass.
    pub const ARGUMENT: &'static str = "legalize-for-masm";
}

impl Pass for LegalizeForMasm {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "legalize-for-masm"
    }

    fn argument(&self) -> &'static str {
        Self::ARGUMENT
    }

    fn description(&self) -> &'static str {
        "Legalizes HIR to the set of operations supported by MASM codegen"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        register_masm_legalization_dialects(&context);
        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = op.as_operation_ref();
        let context = op.context_rc();
        drop(op);

        let target = masm_legalization_target(context.clone());
        let patterns = ConversionPatternSet::new(context);
        let result = apply_full_conversion(root, target, patterns, ConversionConfig::default())?;

        let changed = PostPassStatus::from(result.changed());
        state.set_post_pass_status(changed);
        if !changed.ir_changed() {
            state.preserved_analyses_mut().preserve_all();
        }

        Ok(())
    }
}

/// Build a conversion target that represents the final IR accepted by MASM codegen.
///
/// Structural builtin operations such as modules and functions are legal containers, but their
/// nested operations are still checked. Leaf operations in explicitly supported dialects are legal
/// only when they implement `HirLowering`. `builtin.unrealized_conversion_cast` is always illegal
/// as a final operation.
pub fn masm_legalization_target(context: Rc<Context>) -> ConversionTarget {
    register_masm_legalization_dialects(&context);
    let mut target = ConversionTarget::new(context);
    populate_masm_legalization_target(&mut target);
    target
}

/// Populate `target` with MASM codegen legality rules.
///
/// This helper is exposed so tests and future codegen passes can extend the MASM target while
/// keeping the base policy centralized in this crate.
pub fn populate_masm_legalization_target(target: &mut ConversionTarget) {
    target
        .add_legal_op::<builtin::World>()
        .add_legal_op::<builtin::Component>()
        .add_legal_op::<builtin::Module>()
        .add_legal_op::<builtin::Interface>()
        .add_legal_op::<builtin::Function>()
        .add_legal_op::<builtin::GlobalVariable>()
        .add_legal_op::<builtin::Segment>()
        .add_legal_op::<builtin::FunctionTable>()
        .add_dynamically_legal_op::<builtin::FunctionTableEntry, _>(|op| {
            let inside_table = op
                .parent_op()
                .is_some_and(|parent| parent.borrow().is::<builtin::FunctionTable>());
            if inside_table {
                DynamicLegalityResult::legal()
            } else {
                DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' is only permitted in the entries region of a \
                     'builtin.function_table'",
                    op.name()
                )))
            }
        })
        .add_dynamically_legal_op::<builtin::UnrealizedConversionCast, _>(|op| {
            DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                "operation '{}' is temporary dialect-conversion scaffolding and must be \
                 reconciled or lowered to a real cast before MASM codegen",
                op.name()
            )))
        })
        .add_dynamically_legal_dialect::<builtin::BuiltinDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<arith::ArithDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<cf::ControlFlowDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<scf::ScfDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<ub::UndefinedBehaviorDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<hir::HirDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<wasm::WasmDialect, _>(masm_lowerable_op)
        .add_dynamically_legal_dialect::<debuginfo::DebugInfoDialect, _>(masm_lowerable_op);
}

fn register_masm_legalization_dialects(context: &Rc<Context>) {
    context.get_or_register_dialect::<builtin::BuiltinDialect>();
    context.get_or_register_dialect::<arith::ArithDialect>();
    context.get_or_register_dialect::<cf::ControlFlowDialect>();
    context.get_or_register_dialect::<scf::ScfDialect>();
    context.get_or_register_dialect::<ub::UndefinedBehaviorDialect>();
    context.get_or_register_dialect::<hir::HirDialect>();
    context.get_or_register_dialect::<wasm::WasmDialect>();
    context.get_or_register_dialect::<debuginfo::DebugInfoDialect>();
}

fn masm_lowerable_op(op: &Operation) -> DynamicLegalityResult {
    if op.implements::<dyn HirLowering>() {
        DynamicLegalityResult::legal()
    } else {
        DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
            "operation '{}' is in a MASM-supported dialect but does not implement HirLowering",
            op.name()
        )))
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, testing::Test};

    use super::*;

    #[test]
    fn masm_supported_ops_pass_legalization() {
        let mut test = Test::new("masm_supported_ops_pass_legalization", &[], &[Type::U32]);
        {
            let mut builder = test.function_builder();
            let value = builder.u32(7, SourceSpan::UNKNOWN);
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }

        test.apply_pass::<LegalizeForMasm>(true).unwrap();
    }

    #[test]
    fn unsupported_hir_ops_fail_legalization() {
        let mut test = Test::new("unsupported_hir_ops_fail_legalization", &[], &[]);
        {
            let mut builder = test.function_builder();
            let _bytes = builder.bytes(&[1, 2, 3, 4], SourceSpan::UNKNOWN).unwrap();
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        }

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.bytes"));
        assert!(message.contains("does not implement HirLowering"));
    }

    #[test]
    fn unreconciled_unrealized_conversion_casts_fail_legalization() {
        let mut test = Test::new(
            "unreconciled_unrealized_conversion_casts_fail_legalization",
            &[Type::U32],
            &[Type::I32],
        );
        {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let arg = entry.borrow().arguments()[0].borrow().as_value_ref();
            let cast =
                builder.unrealized_conversion_cast(arg, Type::I32, SourceSpan::UNKNOWN).unwrap();
            builder.ret([cast], SourceSpan::UNKNOWN).unwrap();
        }

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("builtin.unrealized_conversion_cast"));
        assert!(message.contains("temporary dialect-conversion scaffolding"));
    }
}
