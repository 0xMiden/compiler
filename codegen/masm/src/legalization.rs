use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    Context, EntityMut, Op, Operation, OperationName, OperationRef, Report, Symbol, SymbolRef,
    Visibility, WalkResult,
    conversion::{
        ConversionConfig, ConversionPatternSet, ConversionTarget, DynamicLegalityResult,
        apply_full_conversion,
    },
    dialects::{builtin, debuginfo},
    pass::{Pass, PassExecutionState, PostPassStatus},
};
use midenc_session::diagnostics::{Severity, Spanned};

use crate::HirLowering;

/// The number of operand stack elements addressable by Miden Assembly instructions.
///
/// An indirect call schedules its arguments plus the table index inside this window, which
/// bounds the argument size its lowering can support.
const OPERAND_STACK_WINDOW_FELTS: usize = miden_core::program::MIN_STACK_DEPTH;

/// Validate every `hir.procedure_root` below `root` before MASM procedures begin snapshotting HIR
/// visibility.
///
/// MASM dialect legalization establishes that this operation has a lowering, while this preflight
/// checks linkability only for the operations the component builder selected for emission. Running
/// it at that boundary keeps invalid input from reaching instruction emission without inspecting
/// intentionally omitted world siblings.
pub(crate) fn validate_procedure_roots(root: &Operation) -> Result<(), Report> {
    root.prewalk(|op| {
        let Some(procedure_root) = op.downcast_ref::<hir::ProcedureRoot>() else {
            return WalkResult::Continue(());
        };
        match validate_procedure_root(procedure_root) {
            Ok(_) => WalkResult::Continue(()),
            Err(err) => WalkResult::Break(err),
        }
    })
    .into_result()
}

/// Resolve and validate one `hir.procedure_root` for MASM lowering.
///
/// A private procedure is linkable only from within the MASM module that defines it. HIR symbol
/// tables are the ownership boundaries lowered to MASM modules for components, interfaces, and
/// modules. The one exception is a component-less world with exactly one module: its world-level
/// functions and that module intentionally coalesce into the same MASM root. Comparing the
/// effective owners determines whether a private reference crosses a boundary without making
/// visibility depend on lowering order.
pub(crate) fn validate_procedure_root(
    procedure_root: &hir::ProcedureRoot,
) -> Result<SymbolRef, Report> {
    let op = procedure_root.as_operation();
    let context = op.context();
    let caller_symbol_table = op.nearest_symbol_table().ok_or_else(|| {
        context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message("invalid procedure_root operation: no containing symbol table")
            .with_primary_label(
                procedure_root.span(),
                "this operation must be nested in a symbol table",
            )
            .into_report()
    })?;
    let callee = {
        let symbol_table = caller_symbol_table.borrow();
        symbol_table
            .as_symbol_table()
            .expect("nearest_symbol_table returned a non-symbol-table operation")
            .resolve(procedure_root.callee().path())
    }
    .ok_or_else(|| {
        context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message("invalid procedure_root operation: unable to resolve callee")
            .with_primary_label(
                procedure_root.span(),
                "this symbol path is not resolvable from this operation",
            )
            .into_report()
    })?;

    let callee_op = callee.borrow();

    // An op marked as the note script root must have been repointed at the lifted note-script
    // export by component export lifting. Check this before ordinary visibility so a missed
    // retarget keeps its more specific diagnostic.
    if op.get_attribute(hir::ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR).is_some()
        && callee_op
            .as_symbol_operation()
            .get_attribute(hir::NOTE_SCRIPT_EXPORT_ATTR)
            .is_none()
    {
        return Err(context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(
                "invalid procedure_root operation: expected the note script root, but the callee \
                 is not the `note_script`-attributed export",
            )
            .with_primary_label(
                procedure_root.span(),
                "this operation must reference the lifted note-script export",
            )
            .with_help(
                "the containing component must define a note-script export, and operations marked \
                 as the note script root must be retargeted at it during component export lifting",
            )
            .into_report());
    }

    let callee_symbol_table = callee_op.as_symbol_operation().nearest_symbol_table();
    if callee_op.visibility() == Visibility::Private
        && callee_symbol_table
            .is_none_or(|callee_owner| !share_masm_module(caller_symbol_table, callee_owner))
    {
        return Err(context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(format!(
                "invalid hir.procedure_root: private callee '{}' is not linkable from another \
                 Miden Assembly module",
                callee_op.path()
            ))
            .with_primary_label(
                procedure_root.span(),
                "this reference crosses a Miden Assembly module boundary",
            )
            .with_secondary_label(
                callee_op.as_symbol_operation().span(),
                "this callee is private to its defining module",
            )
            .with_help(
                "declare the callee internal or public and ensure any intervening module is \
                 public, or materialize the root within its defining module",
            )
            .into_report());
    }

    if let Some(callee_symbol_table) = callee_symbol_table
        && let Some(inaccessible_module) =
            first_inaccessible_callee_module(caller_symbol_table, callee_symbol_table)
    {
        let inaccessible_module = inaccessible_module.borrow();
        let module = inaccessible_module
            .downcast_ref::<builtin::Module>()
            .expect("only a module can make a MASM module path inaccessible");
        return Err(context
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(format!(
                "invalid hir.procedure_root: callee '{}' is nested beneath private module '{}'",
                callee_op.path(),
                module.path()
            ))
            .with_primary_label(
                procedure_root.span(),
                "this reference cannot reach the callee's Miden Assembly module",
            )
            .with_secondary_label(
                module.as_operation().span(),
                "this module is private outside its parent and sibling modules",
            )
            .with_help(
                "declare the intervening module public, or materialize the root within its parent \
                 or a sibling module",
            )
            .into_report());
    }

    drop(callee_op);
    Ok(callee)
}

/// Whether two HIR symbol-table owners emit procedures into the same MASM module.
fn share_masm_module(lhs: OperationRef, rhs: OperationRef) -> bool {
    if lhs == rhs {
        return true;
    }

    fn is_the_only_module_of_world(module: OperationRef, world: OperationRef) -> bool {
        if module.borrow().parent_op() != Some(world) {
            return false;
        }
        let world = world.borrow();
        let Some(world) = world.downcast_ref::<builtin::World>() else {
            return false;
        };
        let body = world.body();
        let entry = body.entry();
        let ops = entry.body();
        let mut modules = ops.iter().filter(|op| op.is::<builtin::Module>());
        modules.next().is_some_and(|only| only.as_operation_ref() == module)
            && modules.next().is_none()
            && !ops.iter().any(|op| op.is::<builtin::Component>())
    }

    (lhs.borrow().is::<builtin::World>() && is_the_only_module_of_world(rhs, lhs))
        || (rhs.borrow().is::<builtin::World>() && is_the_only_module_of_world(lhs, rhs))
}

/// Return the first module on the callee side which is not visible from the caller's MASM module.
///
/// A private MASM submodule is visible to its parent and every descendant of that parent.
/// Consequently, the first callee branch below the owners' common ancestor may remain private;
/// every deeper callee-only module must be public.
fn first_inaccessible_callee_module(
    caller_owner: OperationRef,
    callee_owner: OperationRef,
) -> Option<OperationRef> {
    fn owner_ancestry(mut owner: OperationRef) -> Vec<OperationRef> {
        let mut ancestry = Vec::new();
        loop {
            ancestry.push(owner);
            let parent = owner.borrow().nearest_symbol_table();
            let Some(parent) = parent else {
                break;
            };
            owner = parent;
        }
        ancestry.reverse();
        ancestry
    }

    let caller_ancestry = owner_ancestry(caller_owner);
    let callee_ancestry = owner_ancestry(callee_owner);
    let common_len = caller_ancestry
        .iter()
        .zip(callee_ancestry.iter())
        .take_while(|(caller, callee)| caller == callee)
        .count();
    callee_ancestry[common_len..].iter().enumerate().find_map(|(index, owner)| {
        let owner_op = owner.borrow();
        let module = owner_op.downcast_ref::<builtin::Module>()?;
        let private_in_masm = !modules_form_the_artifact_interface(*owner)
            && *module.get_visibility() != Visibility::Public;
        (private_in_masm && index != 0).then_some(*owner)
    })
}

/// Whether lowering forces modules in this artifact to be public regardless of HIR visibility.
fn modules_form_the_artifact_interface(mut owner: OperationRef) -> bool {
    loop {
        let op = owner.borrow();
        if let Some(component) = op.downcast_ref::<builtin::Component>() {
            return component.is_synthetic_wrapper();
        }
        if let Some(world) = op.downcast_ref::<builtin::World>() {
            let body = world.body();
            return !body.entry().body().iter().any(|op| op.is::<builtin::Component>());
        }
        let Some(parent) = op.parent_op() else {
            return false;
        };
        drop(op);
        owner = parent;
    }
}

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
        .add_dynamically_legal_op::<builtin::FunctionTable, _>(|op| {
            let inside_module =
                op.parent_op().is_some_and(|parent| parent.borrow().is::<builtin::Module>());
            if inside_module {
                DynamicLegalityResult::legal()
            } else {
                DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' is only permitted in the body of a 'builtin.module', the one \
                     place the linker's memory layout visits",
                    op.name()
                )))
            }
        })
        .add_dynamically_legal_op::<builtin::FunctionTableEntry, _>(|op| {
            let entry = op
                .downcast_ref::<builtin::FunctionTableEntry>()
                .expect("this legality rule is registered for builtin.function_table_entry");
            let Some(parent) = op.parent_op() else {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' is only permitted in the entries region of a \
                     'builtin.function_table'",
                    op.name()
                )));
            };
            let parent = parent.borrow();
            let Some(table) = parent.downcast_ref::<builtin::FunctionTable>() else {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' is only permitted in the entries region of a \
                     'builtin.function_table'",
                    op.name()
                )));
            };
            let slot = *entry.get_index();
            let num_slots = *table.get_num_slots();
            if slot >= num_slots {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' initializes slot {slot}, which is out of bounds for table \
                     '{}' with {num_slots} slots",
                    op.name(),
                    table.get_name().as_str()
                )));
            }
            if *entry.get_type_tag() == 0 {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' uses signature tag 0, which is reserved for null slots",
                    op.name()
                )));
            }
            if entry.resolve_callee().is_none() {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' names callee '{}', which does not resolve",
                    op.name(),
                    entry.callee().path()
                )));
            }
            DynamicLegalityResult::legal()
        })
        .add_dynamically_legal_op::<hir::ExecIndirect, _>(|op| {
            let exec = op
                .downcast_ref::<hir::ExecIndirect>()
                .expect("this legality rule is registered for hir.exec_indirect");
            let signature = exec.get_signature();
            // The lowering consumes the arguments as-is: an extension requirement would need
            // instructions operating on the stack top, which the transient slot address holds
            if let Some(index) = signature.params.iter().position(|param| {
                !matches!(
                    param.extension(),
                    midenc_hir::dialects::builtin::attributes::ArgumentExtension::None
                )
            }) {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' does not support argument extension, which parameter {index} \
                     requires",
                    op.name()
                )));
            }
            let arg_felts: usize =
                signature.params.iter().map(|param| param.ty.size_in_felts()).sum();
            if arg_felts + 1 > OPERAND_STACK_WINDOW_FELTS {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' schedules {arg_felts} argument field elements plus the table \
                     index, which exceeds the {OPERAND_STACK_WINDOW_FELTS}-element operand stack \
                     window",
                    op.name()
                )));
            }
            DynamicLegalityResult::legal()
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
    use alloc::{boxed::Box, format};

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        Ident, SourceSpan, Type, ValueRef, Visibility,
        dialects::builtin::{
            BuiltinOpBuilder, ModuleBuilder,
            attributes::{AbiParam, Signature},
        },
        testing::Test,
    };

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

    /// A function table anywhere but a module body is invisible to the linker's layout scan, so
    /// it must be rejected here instead of panicking when a dispatch cannot find its address.
    #[test]
    fn function_tables_outside_a_module_fail_legalization() {
        let mut test = Test::new("function_tables_outside_a_module_fail_legalization", &[], &[]);
        {
            let mut builder = test.function_builder();
            builder
                .create_function_table(Ident::from("tbl"), Visibility::Private, 2)
                .unwrap();
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        }

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("builtin.function_table"), "{message}");
        assert!(message.contains("body of a 'builtin.module'"), "{message}");
    }

    /// Run `LegalizeForMasm` over `test`'s module.
    ///
    /// `Test::apply_pass` anchors the pass on the test's primary function, which never reaches a
    /// function table: tables live in the module body, as they do under the
    /// `PassManager::on::<builtin::World>` the backend pipeline uses.
    fn legalize_module(test: &Test) -> Result<(), Report> {
        use midenc_hir::pass::{Nesting, PassManager};

        let mut pm = PassManager::on::<builtin::Module>(test.context_rc(), Nesting::Implicit);
        pm.add_pass(Box::new(LegalizeForMasm));
        pm.enable_verifier(false);
        pm.run(test.module().as_operation_ref())
    }

    /// A slot past the end of its table has no address in the linker's layout, so codegen
    /// cannot emit an initializer for it; legalization is where that is decided.
    #[test]
    fn out_of_bounds_function_table_entries_fail_legalization() {
        let mut test = Test::named("out_of_bounds_entry").in_module("m");
        test.with_function("dispatch", &[], &[]);
        let table = ModuleBuilder::new(test.module())
            .define_function_table(Ident::from("tbl"), Visibility::Private, 1)
            .unwrap();
        ModuleBuilder::new(test.module())
            .append_function_table_entry(table, 0, 1, test.function(), SourceSpan::UNKNOWN)
            .unwrap();
        // The builder rejects an out-of-bounds slot up front, so rewrite the index afterwards to
        // build the IR a producer that did not go through the builder could hand codegen
        {
            let mut entry_op = {
                let table = table.borrow();
                let entries = table.entries();
                entries.entry().body().into_iter().next().unwrap().as_operation_ref()
            };
            let mut entry_op = entry_op.borrow_mut();
            entry_op
                .downcast_mut::<builtin::FunctionTableEntry>()
                .expect("a function table's entries region holds only entries")
                .set_index(9u32);
        }

        let err = legalize_module(&test).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("builtin.function_table_entry"), "{message}");
        assert!(message.contains("out of bounds"), "{message}");
    }

    /// Build a module hosting a two-slot table and a `dispatch` function whose
    /// `hir.exec_indirect` uses `signature`, passing one u32 constant per parameter.
    fn test_with_exec_indirect(test: &mut Test, signature: Signature) {
        test.with_function("dispatch", &[Type::U32], &[]);
        let table = ModuleBuilder::new(test.module())
            .define_function_table(Ident::from("tbl"), Visibility::Private, 2)
            .unwrap();
        let arity = signature.params.len();
        let mut builder = test.function_builder();
        let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let args = (0..arity)
            .map(|_| builder.u32(0, SourceSpan::UNKNOWN))
            .collect::<alloc::vec::Vec<_>>();
        builder
            .exec_indirect(table, signature, 1, index, args, SourceSpan::UNKNOWN)
            .unwrap();
        builder.ret(None, SourceSpan::UNKNOWN).unwrap();
    }

    /// Arguments plus the table index must fit the addressable operand stack window; the wasm
    /// frontend diagnoses this at translation, but IR from any other producer reaches codegen
    /// unchecked.
    #[test]
    fn oversized_exec_indirect_arguments_fail_legalization() {
        let mut test = Test::named("oversized_exec_indirect").in_module("m");
        let signature = Signature::new(&test.context_rc(), vec![Type::U32; 16], []);
        test_with_exec_indirect(&mut test, signature);

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("operand stack window"), "{message}");
    }

    /// The indirect-call lowering cannot apply argument extension, since the stack top holds the
    /// transient slot address while arguments are consumed.
    #[test]
    fn extension_requiring_exec_indirect_arguments_fail_legalization() {
        let mut test = Test::named("extension_exec_indirect").in_module("m");
        let mut signature = Signature::new(&test.context_rc(), [Type::U32], []);
        signature.params[0] = AbiParam::sext(Type::U32, &test.context_rc());
        test_with_exec_indirect(&mut test, signature);

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("argument extension"), "{message}");
    }
}
