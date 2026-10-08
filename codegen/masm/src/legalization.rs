use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    CallConv, Context, EntityMut, Immediate, Op, Operation, OperationName, OperationRef, Overflow,
    PointerType, Report, SmallVec, SourceSpan, Symbol, SymbolRef, Type, UnsafeIntrusiveEntityRef,
    Value, ValueRef, Visibility, WalkResult,
    conversion::{
        ConversionConfig, ConversionPattern, ConversionPatternRewriter, ConversionPatternSet,
        ConversionTarget, ConvertedOperands, DynamicLegalityResult, apply_full_conversion,
    },
    dialects::{builtin, debuginfo},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind},
    traits::{Transparent, TransparentCast},
};
use midenc_session::diagnostics::{Severity, Spanned};

use crate::{HirLowering, opt::operands::MASM_STACK_WINDOW_FELTS};

/// Legality of an indirect call's signature, shared by `hir.exec_indirect` and `hir.dyncall`.
///
/// The arguments must agree with the signature in count and type: the emitter consumes one operand
/// per parameter and asserts each type, so a mismatch is unlowerable. The op verifiers check the
/// same thing, but legalization must be self-sufficient — it also runs over IR the verifier never
/// saw.
///
/// Both lowerings consume the arguments as-is while the callee's memory address sits on the stack
/// top, so they cannot emit the widening instructions a direct call would: those operate on that
/// occupied top. A parameter's extension is only a demand for them when the argument is actually
/// narrower than the parameter. Canonical-ABI flattening marks the parameters it widened —
/// `bool`/`u8`/`u16` as `zext`, `i8`/`i16` as `sext`, both to `i32` — while handing over an
/// argument that already has the flat type, which makes the extension a no-op, exactly as
/// `process_call_signature` treats it for a direct call.
///
/// Beyond that, the arguments and the operands selecting the callee must fit the addressable
/// operand stack window together: the spill analysis requires every operand of a non-branch
/// operation to be reachable at once. `selector_felts` is how many of the window those callee
/// operands take — one element for `hir.exec_indirect`'s table index, a whole word for
/// `hir.dyncall`'s procedure root — and `what` names them in diagnostics.
fn indirect_call_signature_legality(
    op: &Operation,
    signature: &midenc_hir::dialects::builtin::attributes::Signature,
    arg_types: &[midenc_hir::Type],
    selector_felts: usize,
    what: &str,
) -> DynamicLegalityResult {
    use midenc_hir::dialects::builtin::attributes::ArgumentExtension;

    if arg_types.len() != signature.params.len() {
        return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
            "operation '{}' passes {} argument(s), but its call signature declares {} parameter(s)",
            op.name(),
            arg_types.len(),
            signature.params.len()
        )));
    }
    for (index, (param, arg_ty)) in signature.params.iter().zip(arg_types.iter()).enumerate() {
        if *arg_ty == param.ty {
            continue;
        }
        if matches!(param.extension(), ArgumentExtension::None) {
            return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                "operation '{}' passes an argument of type {arg_ty} for parameter {index} of type \
                 {}; the lowering emits one operand per parameter and cannot convert between them",
                op.name(),
                param.ty
            )));
        }
        return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
            "operation '{}' cannot extend the argument for parameter {index} from {arg_ty} to {}; \
             extension is only supported when the argument already has the parameter type",
            op.name(),
            param.ty
        )));
    }
    let arg_felts: usize = signature.params.iter().map(|param| param.ty.size_in_felts()).sum();
    if selector_felts + arg_felts > MASM_STACK_WINDOW_FELTS {
        return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
            "operation '{}' schedules {arg_felts} argument field elements plus the \
             {selector_felts}-element {what}, which exceeds the {MASM_STACK_WINDOW_FELTS}-element \
             operand stack window",
            op.name()
        )));
    }
    DynamicLegalityResult::legal()
}

/// The types of an indirect call's arguments, which both `hir.exec_indirect` and `hir.dyncall`
/// hold in operand group 1 — group 0 being the table index, respectively the root word.
fn argument_types(op: &Operation) -> Vec<midenc_hir::Type> {
    op.operands()
        .group(1)
        .iter()
        .map(|operand| operand.borrow().as_value_ref().borrow().ty().clone())
        .collect()
}

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
/// If the callee path names a `builtin.function_alias`, the alias itself is returned, and
/// linkability checks apply to the alias's own visibility. HIR verification prevents non-private
/// aliases from exposing private canonical targets.
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
/// lower, and rewrites the 64-bit integer operations it would lower unsafely when they pack or
/// unpack a pair of felts. A 64-bit integer assembled from two 32-bit halves,
/// `or(zext(lo), shl(zext(hi), 32))`, becomes an `arith.join` of them, and so does one with a
/// constant half, as LLVM folds the pack of a felt beside a constant. In a block that takes the
/// high half of a 64-bit integer, `trunc(shr(_, 32))`, it and every low half taken there with
/// `trunc` become limbs of an `arith.split` of it. Neither runs a `u32` instruction (a join may
/// move its operands into place), so the pair travels through block arguments, `scf.if` results
/// and locals as the two elements it is. A 64-bit store of such a pair becomes two 32-bit stores,
/// and a 64-bit load used only as its halves two 32-bit loads. What remains unsafe is 64-bit
/// arithmetic proper on reinterpreted felts, which still traps on a felt outside the `u32` range.
///
/// # It must run before spill placement
///
/// The rewrites change live ranges: a join keeps the halves live up to the `or` it replaces, a
/// split keeps the limbs live from the first half it serves, and the store split keeps the halves
/// live up to the store. Spill placement is what keeps the operand stack within the 16 elements
/// an instruction can reach, and it can only do so for live ranges it has seen. So this pass runs
/// before it, after the last rewrite that could reshape the code (in `midenc-compile`,
/// `apply_rewrites`: after the last canonicalization, before operand sinking and spill
/// placement), and codegen, which comes after spill placement, runs [`CheckMasmLegality`]
/// instead, which rewrites nothing. Run after spill placement, this pass could leave an operand
/// deeper than an instruction reaches.
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
        let mut patterns = ConversionPatternSet::new(context.clone());
        patterns.push(SplitWideStore::new(context.clone()));
        patterns.push(SplitWideLoad::new(context.clone()));
        patterns.push(JoinAssembledHalves::new(context.clone()));
        patterns.push(JoinShiftedHalf::new(context.clone()));
        patterns.push(SplitTakenHalves::new(context));
        // The driver caps pattern applications across the whole root, against rewrite cycles,
        // and exceeding the cap is an error. These patterns apply once per site, in ordinary code
        // as much as on felts, and cannot cycle: each erases the op it matches, and none emits an
        // op another matches (32-bit loads and stores, joins, splits, casts and address
        // arithmetic). So a component is not to fail for the number of its sites.
        let mut config = ConversionConfig::default();
        config.with_max_iterations(usize::MAX);
        let result = apply_full_conversion(root, target, patterns, config)?;

        let changed = PostPassStatus::from(result.changed());
        state.set_post_pass_status(changed);
        if !changed.ir_changed() {
            state.preserved_analyses_mut().preserve_all();
        }

        Ok(())
    }
}

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<CheckMasmLegality>(
    CheckMasmLegality::ARGUMENT,
    "check that HIR is legal for MASM codegen, rewriting nothing"
));

/// Checks that IR is in the set of operations MASM codegen can lower, and rewrites nothing:
/// [`masm_legalization_target`] with no patterns.
///
/// This is what codegen runs on entry. It comes after spill placement, where nothing may be
/// rewritten (see [`LegalizeForMasm`]), so a shape that [`LegalizeForMasm`] would have rewritten is
/// an error here, which says that the legalization must run before spill placement; an op MASM
/// codegen cannot lower at all is the same error as from [`LegalizeForMasm`]. Unrealized
/// conversion casts are not reconciled, which would be a rewrite: one that is left is illegal.
#[derive(Default)]
pub struct CheckMasmLegality;

impl CheckMasmLegality {
    /// Command-line/pass-pipeline argument for this pass.
    pub const ARGUMENT: &'static str = "check-masm-legality";
}

impl Pass for CheckMasmLegality {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "check-masm-legality"
    }

    fn argument(&self) -> &'static str {
        Self::ARGUMENT
    }

    fn description(&self) -> &'static str {
        "Checks that HIR is in the set of operations supported by MASM codegen, rewriting nothing"
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
        let mut config = ConversionConfig::default();
        config.with_reconcile_unrealized_casts(false);
        apply_full_conversion(root, target, patterns, config)?;

        state.set_post_pass_status(PostPassStatus::Unchanged);
        state.preserved_analyses_mut().preserve_all();
        Ok(())
    }
}

/// Build a conversion target that represents the final IR accepted by MASM codegen.
///
/// Structural builtin operations such as modules and functions are legal containers, but their
/// nested operations are still checked. Leaf operations in explicitly supported dialects are legal
/// only when they implement `HirLowering` or `TransparentCast`, which codegen lowers by renaming
/// the operand. `builtin.unrealized_conversion_cast` is always illegal
/// as a final operation, and so are the shapes [`LegalizeForMasm`] rewrites: the `arith.bor` that
/// assembles a 64-bit integer from two 32-bit halves, the `arith.shl` that makes one of a high
/// half and a zero low half, the `arith.trunc`s that take the halves of one in a block that takes
/// its high half, and the 64-bit stores and loads of two 32-bit halves.
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
        .add_legal_op::<builtin::FunctionAlias>()
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
            indirect_call_signature_legality(
                op,
                &exec.get_signature(),
                &argument_types(op),
                /*selector_felts=*/ 1,
                "table index",
            )
        })
        .add_dynamically_legal_op::<hir::Dyncall, _>(|op| {
            let call = op
                .downcast_ref::<hir::Dyncall>()
                .expect("this legality rule is registered for hir.dyncall");
            // The lowering spills the whole root word to the cell the VM reads the callee digest
            // from, so anything but a full word would spill operands it does not own
            let num_root_felts = call.root().len();
            if num_root_felts != hir::Dyncall::ROOT_FELTS {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' selects its callee with {num_root_felts} root field \
                     element(s), but the lowering spills exactly {} of them as the root word",
                    op.name(),
                    hir::Dyncall::ROOT_FELTS
                )));
            }
            // The lowering pops one field element per root operand, which the emitter asserts,
            // and it has no conversion to apply on the way: the root is spilled verbatim to the
            // cell the VM reads the callee digest from
            if let Some((index, ty)) = call
                .root()
                .iter()
                .map(|operand| operand.borrow().as_value_ref().borrow().ty().clone())
                .enumerate()
                .find(|(_, ty)| *ty != midenc_hir::Type::Felt)
            {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' selects its callee with root element {index} of type {ty}, \
                     but the lowering spills the root word as field elements",
                    op.name()
                )));
            }
            let signature = call.get_signature();
            if signature.cc != CallConv::ComponentModel {
                return DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                    "operation '{}' uses the '{}' calling convention, but only the \
                     component-model calling convention is supported: the lowering has no \
                     return-pointer handling for the others",
                    op.name(),
                    signature.cc
                )));
            }
            indirect_call_signature_legality(
                op,
                &signature,
                &argument_types(op),
                hir::Dyncall::ROOT_FELTS,
                "procedure root",
            )
        })
        .add_dynamically_legal_op::<builtin::UnrealizedConversionCast, _>(|op| {
            DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
                "operation '{}' is temporary dialect-conversion scaffolding and must be \
                 reconciled or lowered to a real cast before MASM codegen",
                op.name()
            )))
        })
        .add_dynamically_legal_op::<hir::Store, _>(|op| {
            if Halves::stored_by(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit store of two 32-bit halves must have been split into two 32-bit \
                     stores by `legalize-for-masm`, which must run before spill placement",
                ))
            } else {
                masm_lowerable_op(op)
            }
        })
        .add_dynamically_legal_op::<hir::Load, _>(|op| {
            if TakenHalves::loaded_by(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit load used only as 32-bit halves must have been split into two \
                     32-bit loads by `legalize-for-masm`, which must run before spill placement",
                ))
            } else {
                masm_lowerable_op(op)
            }
        })
        .add_dynamically_legal_op::<arith::Bor, _>(|op| {
            if Halves::assembled_by(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit integer assembled from two 32-bit halves must have been made an \
                     'arith.join' of them by `legalize-for-masm`, which must run before spill \
                     placement",
                ))
            } else {
                masm_lowerable_op(op)
            }
        })
        .add_dynamically_legal_op::<arith::Shl, _>(|op| {
            if shifted_half(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit integer whose high half is a 32-bit value and whose low half is \
                     zero must have been made an 'arith.join' by `legalize-for-masm`, which must \
                     run before spill placement",
                ))
            } else {
                masm_lowerable_op(op)
            }
        })
        .add_dynamically_legal_op::<arith::Trunc, _>(|op| {
            if SplitHalves::at(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 32-bit half of a 64-bit integer, in a block that takes its high half, must \
                     have been made a limb of an 'arith.split' of it by `legalize-for-masm`, \
                     which must run before spill placement",
                ))
            } else {
                masm_lowerable_op(op)
            }
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

/// An op MASM codegen can lower: one with a `HirLowering`, or a transparent cast, which codegen
/// lowers by renaming its operand.
fn masm_lowerable_op(op: &Operation) -> DynamicLegalityResult {
    if op.implements::<dyn HirLowering>() || op.implements::<dyn TransparentCast>() {
        DynamicLegalityResult::legal()
    } else {
        DynamicLegalityResult::illegal_with_reason(Report::msg(format!(
            "operation '{}' is in a MASM-supported dialect but implements neither HirLowering nor \
             TransparentCast",
            op.name()
        )))
    }
}

/// Splits a 64-bit integer store whose value is two 32-bit halves, `or(zext(lo), shl(zext(hi),
/// 32))` with the `or`'s operands in either order, one of its forms with a constant half (see
/// [`JoinAssembledHalves`]), or the `arith.join(hi, lo)` that [`JoinAssembledHalves`] or
/// [`JoinShiftedHalf`] makes of it, into a 32-bit store of each half.
///
/// The shape comes from LLVM's IR-level passes, which can carry two adjacent 32-bit values as one
/// `i64` and pack them so before storing them. (It does not come from the code generator's store
/// merging, which merges only stores of constants and loaded values, into wide constants and
/// copies.) When the halves are felts, which Rust carries in `f32` and reinterprets as `i32` with
/// a bitcast that emits nothing, the packing is wrong: the 64-bit `shl` and `or` work on 32-bit
/// limbs with `u32` instructions, which trap on a felt outside the `u32` range. A join makes it
/// safe; split, it is also cheaper: each half is stored as the element it is, with no 64-bit
/// store, and the `zext`, `shl` and `or`, or the join, go unless something else uses them. The
/// store is split whether the conversion reaches it before or after the `or` has become a join.
///
/// Only that exact shape is split: a 64-bit integer value, `u32` halves or constants that fit a
/// half, `zext` rather than `sext`, a shift by the constant 32, or a join of two `i32` or `u32`
/// limbs. The sign-only `hir.bitcast`s with which the Wasm frontend spells `i64.extend_i32_u`,
/// and the `band` with which it masks every shift count, are looked through. And the pair must
/// feed the store directly: one that reaches its store through a block argument, an `scf.if`
/// result or an `i64` local is stored with one 64-bit store, which copies its two elements
/// without interpreting them. The high half goes 4 bytes on, or one element on in element
/// space, where `intrinsics::mem::store_dw` puts the high half of a 64-bit store.
struct SplitWideStore {
    info: PatternInfo,
}

impl SplitWideStore {
    fn new(context: Rc<Context>) -> Self {
        let hir = context.get_or_register_dialect::<hir::HirDialect>();
        let mut info = PatternInfo::new(
            context.clone(),
            "split-wide-store",
            PatternKind::Operation(hir.expect_registered_name::<hir::Store>()),
            PatternBenefit::new(1),
        );
        info.with_generated_ops(
            [hir.expect_registered_name::<hir::Store>()]
                .into_iter()
                .chain(half_address_ops(&context)),
        );
        Self { info }
    }
}

impl Pattern for SplitWideStore {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl ConversionPattern for SplitWideStore {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        _operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report> {
        let Some(halves) = Halves::stored_by(&op.borrow()) else {
            return Ok(false);
        };
        let (span, addr, value) = {
            let op = op.borrow();
            let store = op.downcast_ref::<hir::Store>().expect("the halves are a store's");
            (op.span(), store.addr().as_value_ref(), store.value().as_value_ref())
        };

        // Both halves have this type: `u32` from an `or`, the limb type from a join
        let half_ty = halves.ty();
        let lo = halves.lo.materialize(rewriter, span)?;
        let lo_addr = half_address(rewriter, addr, Half::Lo, half_ty.clone(), span)?;
        rewriter.create_op::<hir::Store, _>(span, (lo_addr, lo))?;
        let hi = halves.hi.materialize(rewriter, span)?;
        let hi_addr = half_address(rewriter, addr, Half::Hi, half_ty, span)?;
        rewriter.create_op::<hir::Store, _>(span, (hi_addr, hi))?;
        rewriter.erase_op(op)?;
        erase_dead_defs(rewriter, [value])?;
        Ok(true)
    }
}

/// The two 32-bit halves a 64-bit integer is made of, where its definition says so.
struct Halves {
    lo: HalfValue,
    hi: HalfValue,
}

/// One 32-bit half of a 64-bit integer: a value, or a constant where LLVM has folded one half of
/// a pair to one.
#[derive(Copy, Clone)]
enum HalfValue {
    Value(ValueRef),
    Constant(u32),
}

impl HalfValue {
    /// The half as a value, built as a `u32` constant at the rewriter's insertion point if it is
    /// one.
    fn materialize(
        self,
        rewriter: &mut ConversionPatternRewriter,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Constant(constant) => {
                let constant =
                    rewriter.create_op::<arith::Constant, _>(span, (Immediate::U32(constant),))?;
                Ok(constant.borrow().result().as_value_ref())
            }
        }
    }
}

impl Halves {
    /// The halves that `op`, an `arith.bor`, assembles, with its operands in either order: the
    /// `or` that [`JoinAssembledHalves`] makes a join. The low side is `zext(lo)` of a `u32`, or a
    /// constant below 2^32; the high side is `shl(zext(hi), 32)`, the `join(hi, 0)` that
    /// [`JoinShiftedHalf`] makes of it, or a constant whose low 32 bits are zero. Not both sides
    /// are constants.
    fn assembled_by(op: &Operation) -> Option<Self> {
        let or = op.downcast_ref::<arith::Bor>()?;
        if !is_64bit_integer(or.result().ty()) {
            return None;
        }
        let (lhs, rhs) = (or.lhs().as_value_ref(), or.rhs().as_value_ref());
        let halves = |lo_side, hi_side| {
            let lo = zero_extended_u32(lo_side).map(HalfValue::Value).or_else(|| {
                let constant = constant_u64(lo_side)?;
                Some(HalfValue::Constant(u32::try_from(constant).ok()?))
            })?;
            let hi = shifted_left_by_32(hi_side)
                .and_then(zero_extended_u32)
                .or_else(|| joined_over_zero(hi_side))
                .map(HalfValue::Value)
                .or_else(|| {
                    let constant = constant_u64(hi_side)?;
                    let high = (constant & u64::from(u32::MAX) == 0).then_some(constant >> 32)?;
                    Some(HalfValue::Constant(high as u32))
                })?;
            let constants_only =
                matches!((lo, hi), (HalfValue::Constant(_), HalfValue::Constant(_)));
            (!constants_only).then_some(Self { lo, hi })
        };
        halves(lhs, rhs).or_else(|| halves(rhs, lhs))
    }

    /// The `i32` or `u32` limbs that `op`, an `arith.join` into a 64-bit integer, joins.
    fn joined_by(op: &Operation) -> Option<Self> {
        let join = op.downcast_ref::<arith::Join>()?;
        let limbs = join.limbs();
        if limbs.len() != 2 || !is_64bit_integer(join.result().ty()) {
            return None;
        }
        let [hi, lo] = [0, 1].map(|limb| limbs[limb].borrow().as_value_ref());
        let ty = lo.borrow().ty().clone();
        let is_32bit_integer = matches!(ty, Type::I32 | Type::U32) && *hi.borrow().ty() == ty;
        is_32bit_integer.then_some(Self {
            lo: HalfValue::Value(lo),
            hi: HalfValue::Value(hi),
        })
    }

    /// The halves of the value of `op`, a 64-bit store, that [`SplitWideStore`] stores apart.
    fn stored_by(op: &Operation) -> Option<Self> {
        let store = op.downcast_ref::<hir::Store>()?;
        let value = store.value().as_value_ref();
        if !is_64bit_integer(value.borrow().ty()) {
            return None;
        }
        let def = through_sign_casts(value).borrow().get_defining_op()?;
        let def = def.borrow();
        Self::assembled_by(&def).or_else(|| Self::joined_by(&def))
    }

    /// The type of both halves: that of a value among them, the limb type of a join or `u32`
    /// from an `or`, and `u32` for constants.
    fn ty(&self) -> Type {
        match (self.lo, self.hi) {
            (HalfValue::Value(value), _) | (_, HalfValue::Value(value)) => {
                value.borrow().ty().clone()
            }
            _ => Type::U32,
        }
    }
}

/// Makes `shl(zext(hi), 32)`, a 64-bit integer whose low half is zero, an `arith.join(hi, 0)`.
///
/// It is the pack of a pair whose low half LLVM has folded to the constant zero, as for a felt
/// beside `Felt::ZERO` in a `Word`; on a felt outside the `u32` range, the 64-bit `shl` traps as
/// the pack's does. Made a join, it is also what [`JoinAssembledHalves`] and [`SplitWideStore`]
/// take the high side of a pair from, so a pack is rewritten the same whether its `shl` or its
/// `or` is reached first. The `zext` is matched exactly as for them.
struct JoinShiftedHalf {
    info: PatternInfo,
}

impl JoinShiftedHalf {
    fn new(context: Rc<Context>) -> Self {
        let arith = context.get_or_register_dialect::<arith::ArithDialect>();
        let mut info = PatternInfo::new(
            context.clone(),
            "join-shifted-half",
            PatternKind::Operation(arith.expect_registered_name::<arith::Shl>()),
            PatternBenefit::new(1),
        );
        info.with_generated_ops([
            arith.expect_registered_name::<arith::Join>(),
            arith.expect_registered_name::<arith::Constant>(),
        ]);
        Self { info }
    }
}

impl Pattern for JoinShiftedHalf {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl ConversionPattern for JoinShiftedHalf {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        _operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report> {
        let Some(hi) = shifted_half(&op.borrow()) else {
            return Ok(false);
        };
        let (span, ty, operands) = {
            let op = op.borrow();
            let shl = op.downcast_ref::<arith::Shl>().expect("the half is a shl's");
            let operands = [shl.lhs().as_value_ref(), shl.shift().as_value_ref()];
            (op.span(), shl.result().ty().clone(), operands)
        };

        let lo = HalfValue::Constant(0).materialize(rewriter, span)?;
        let join = rewriter.create_op::<arith::Join, _>(span, ([hi, lo], ty))?;
        let join = join.borrow().result().as_value_ref();
        rewriter.replace_op(op, &[join])?;
        erase_dead_defs(rewriter, operands)?;
        Ok(true)
    }
}

/// The `u32` that `op`, an `arith.shl` with a 64-bit result, shifts into the high half, when it is
/// `shl(zext(hi), 32)`: the shift that [`JoinShiftedHalf`] makes a join.
fn shifted_half(op: &Operation) -> Option<ValueRef> {
    let shl = op.downcast_ref::<arith::Shl>()?;
    if !is_64bit_integer(shl.result().ty()) || constant_u32(shl.shift().as_value_ref()) != Some(32)
    {
        return None;
    }
    zero_extended_u32(shl.lhs().as_value_ref())
}

/// The high limb of `value`, a 64-bit integer, when it is a `join(hi, 0)` of `u32` limbs, as
/// [`JoinShiftedHalf`] makes of `shl(zext(hi), 32)`.
fn joined_over_zero(value: ValueRef) -> Option<ValueRef> {
    let join = defined_by::<arith::Join>(through_sign_casts(value))?;
    let halves = Halves::joined_by(join.borrow().as_operation())?;
    let (HalfValue::Value(hi), HalfValue::Value(lo)) = (halves.hi, halves.lo) else {
        return None;
    };
    let is_u32 = *hi.borrow().ty() == Type::U32;
    (is_u32 && constant_u32(lo) == Some(0)).then_some(hi)
}

/// Makes a 64-bit integer assembled from two 32-bit halves, `or(zext(lo), shl(zext(hi), 32))`
/// with the `or`'s operands in either order, an `arith.join(hi, lo)`, whatever its uses; and
/// likewise one with a constant half: `or(shl(zext(hi), 32), C)` with `C` below 2^32 becomes
/// `join(hi, C)`, and `or(zext(lo), C)` with the low 32 bits of `C` zero becomes
/// `join(C >> 32, lo)`, the constant a `u32` limb. (`shl(zext(hi), 32)` alone, a zero low half, is
/// [`JoinShiftedHalf`]'s.)
///
/// The shape [`SplitWideStore`] splits where it feeds a store directly; this is the rest of it.
/// When LLVM's IR-level passes pack two felts so and the packed `i64` goes on to a block argument,
/// an `scf.if` result or an `i64` local, as when two rebuilt `Word`s meet at a branch, the 64-bit
/// `shl` and `or` trap on a felt outside the `u32` range. A join runs no `u32` instruction: it
/// names the two halves the limbs of the integer, moving them into place on the operand stack if
/// need be, so the pair goes on as the two elements it is. It is cheaper for any two `u32`s, and
/// the `zext` and `shl` go unless something else uses them.
///
/// The halves are matched exactly as for [`SplitWideStore`], with the same matcher, and all are
/// `u32`, so the join needs no cast. An `or` of anything else, a third term or a constant with
/// bits in both halves beside a `zext` included, stays an `or`.
struct JoinAssembledHalves {
    info: PatternInfo,
}

impl JoinAssembledHalves {
    fn new(context: Rc<Context>) -> Self {
        let arith = context.get_or_register_dialect::<arith::ArithDialect>();
        let mut info = PatternInfo::new(
            context.clone(),
            "join-assembled-halves",
            PatternKind::Operation(arith.expect_registered_name::<arith::Bor>()),
            PatternBenefit::new(1),
        );
        info.with_generated_ops([
            arith.expect_registered_name::<arith::Join>(),
            arith.expect_registered_name::<arith::Constant>(),
        ]);
        Self { info }
    }
}

impl Pattern for JoinAssembledHalves {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl ConversionPattern for JoinAssembledHalves {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        _operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report> {
        let Some(halves) = Halves::assembled_by(&op.borrow()) else {
            return Ok(false);
        };
        let (span, ty, operands) = {
            let op = op.borrow();
            let or = op.downcast_ref::<arith::Bor>().expect("the halves are an or's");
            let operands = [or.lhs().as_value_ref(), or.rhs().as_value_ref()];
            (op.span(), or.result().ty().clone(), operands)
        };

        let hi = halves.hi.materialize(rewriter, span)?;
        let lo = halves.lo.materialize(rewriter, span)?;
        let join = rewriter.create_op::<arith::Join, _>(span, ([hi, lo], ty))?;
        let join = join.borrow().result().as_value_ref();
        rewriter.replace_op(op, &[join])?;
        erase_dead_defs(rewriter, operands)?;
        Ok(true)
    }
}

/// Splits a 64-bit integer load whose every use takes a 32-bit half of it, `trunc` for the low
/// half and `trunc(shr(_, 32))` for the high half, into a 32-bit load of each half used.
///
/// The mirror of [`SplitWideStore`]: LLVM's IR-level passes can read two adjacent 32-bit values
/// as one `i64` and take it apart so, and the 64-bit `shr` traps on the limbs of a felt pair.
/// Split, each half is loaded as the element it is, and the 64-bit load and `shr` are gone. The
/// `shr` must be logical (of a `u64`), and the same sign-only casts and masked shift count are
/// looked through. Every use must take its half directly: a load whose value also goes anywhere
/// else, such as to a successor block, out of an `scf.if` or into an `i64` local, stays one 64-bit
/// load, and [`SplitTakenHalves`] takes its halves from a split of it. Uses by debug info do not
/// count, and go with the 64-bit value. A load this pattern splits is declined by
/// [`SplitTakenHalves`], whichever of the load and its `trunc`s the conversion reaches first: two
/// 32-bit loads beat one 64-bit load and a split.
struct SplitWideLoad {
    info: PatternInfo,
}

impl SplitWideLoad {
    fn new(context: Rc<Context>) -> Self {
        let hir = context.get_or_register_dialect::<hir::HirDialect>();
        let mut info = PatternInfo::new(
            context.clone(),
            "split-wide-load",
            PatternKind::Operation(hir.expect_registered_name::<hir::Load>()),
            PatternBenefit::new(1),
        );
        info.with_generated_ops(
            [hir.expect_registered_name::<hir::Load>()]
                .into_iter()
                .chain(half_address_ops(&context)),
        );
        Self { info }
    }
}

impl Pattern for SplitWideLoad {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl ConversionPattern for SplitWideLoad {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        _operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report> {
        let Some(halves) = TakenHalves::loaded_by(&op.borrow()) else {
            return Ok(false);
        };
        let (span, addr) = {
            let op = op.borrow();
            let load = op.downcast_ref::<hir::Load>().expect("the halves are a load's");
            (op.span(), load.addr().as_value_ref())
        };

        // What each `trunc` took its half of, for the ops left dead once it is replaced
        let mut truncated = SmallVec::<[ValueRef; 4]>::new();
        for (half, truncs) in [(Half::Lo, &halves.lo), (Half::Hi, &halves.hi)] {
            let Some(&first) = truncs.first() else {
                continue;
            };
            let half_ty = result_type(first);
            let half_addr = half_address(rewriter, addr, half, half_ty.clone(), span)?;
            let loaded = rewriter.create_op::<hir::Load, _>(span, (half_addr,))?;
            let loaded = loaded.borrow().result().as_value_ref();
            for &trunc in truncs {
                let ty = result_type(trunc);
                let replacement = if ty == half_ty {
                    loaded
                } else {
                    let cast = rewriter.create_op::<hir::Bitcast, _>(span, (loaded, ty))?;
                    cast.borrow().result().as_value_ref()
                };
                truncated.push(trunc.borrow().operands()[0].borrow().as_value_ref());
                rewriter.replace_op(trunc, &[replacement])?;
            }
        }
        // The `shr` and casts of the high half, and the 64-bit load itself, are dead now
        erase_dead_defs(rewriter, truncated)?;
        Ok(true)
    }
}

/// The uses that take a 32-bit half of a 64-bit integer: `trunc` to 32 bits for the low half, and
/// `trunc(shr(_, 32))`, a logical shift, for the high half, with the sign-only casts on the way
/// looked through.
#[derive(Default)]
struct TakenHalves {
    /// The `trunc`s of the low half
    lo: SmallVec<[OperationRef; 1]>,
    /// The `trunc`s of the high half, each of a `shr` by 32
    hi: SmallVec<[OperationRef; 1]>,
    /// Whether the integer, or a cast or `shr` of it on the way to a `trunc`, has a use that
    /// takes no half. Uses by debug info do not count.
    other_uses: bool,
}

impl TakenHalves {
    /// The uses of `value`, a 64-bit integer, that take a half of it.
    fn of(value: ValueRef) -> Self {
        let mut halves = Self::default();
        halves.sort_uses(value, Half::Lo);
        halves
    }

    /// The uses of `op`, a 64-bit load, when every use takes a half: the load that
    /// [`SplitWideLoad`] splits.
    fn loaded_by(op: &Operation) -> Option<Self> {
        let load = op.downcast_ref::<hir::Load>()?;
        let value = load.result().as_value_ref();
        if !is_64bit_integer(value.borrow().ty()) {
            return None;
        }
        let halves = Self::of(value);
        let any_taken = !halves.lo.is_empty() || !halves.hi.is_empty();
        (any_taken && !halves.other_uses).then_some(halves)
    }

    /// Sort the uses of `value`, which holds `half` in its low 32 bits, into `trunc`s of either
    /// half and other uses.
    fn sort_uses(&mut self, value: ValueRef, half: Half) {
        let (ty, users) = {
            let value = value.borrow();
            let users = value.iter_uses().map(|user| user.owner).collect::<SmallVec<[_; 4]>>();
            (value.ty().clone(), users)
        };
        for user in users {
            let op = user.borrow();
            if op.implements::<dyn Transparent>() {
                continue;
            }
            if let Some(cast) = op.downcast_ref::<hir::Bitcast>()
                && differs_only_in_signedness(&ty, cast.result().ty())
            {
                self.sort_uses(cast.result().as_value_ref(), half);
            } else if let Some(trunc) = op.downcast_ref::<arith::Trunc>()
                && matches!(trunc.result().ty(), Type::I32 | Type::U32)
            {
                match half {
                    Half::Lo => self.lo.push(user),
                    Half::Hi => self.hi.push(user),
                }
            } else if let Some(shr) = op.downcast_ref::<arith::Shr>()
                && half == Half::Lo
                && ty == Type::U64
                && constant_u32(shr.shift().as_value_ref()) == Some(32)
            {
                self.sort_uses(shr.result().as_value_ref(), Half::Hi);
            } else {
                self.other_uses = true;
            }
        }
    }
}

/// Takes the 32-bit halves of a 64-bit integer from an `arith.split` of it, in each block that
/// takes its high half: there, that `trunc(shr(_, 32))`, with a logical shift, and every other use
/// that takes a half, the low half by `trunc` to 32 bits, become limbs of one split, placed before
/// the first of them. Whatever defines the integer.
///
/// The mirror of [`JoinAssembledHalves`]. When LLVM's IR-level passes take two felts back out of
/// an `i64` that came through a block argument, an `scf.if` result or an `i64` local, the 64-bit
/// `shr` traps on a felt outside the `u32` range. A split runs no `u32` instruction: it names the
/// integer's two limbs the halves, moving the integer into place on the operand stack if need be.
/// It is cheaper for any integer, and the `shr` goes unless something else uses it.
///
/// Uses of the integer that take no half keep it: codegen copies an operand that is still live
/// after the op that uses it, so the split leaves the integer in place for them, as the `trunc`s
/// did. A block that takes only the low half is left alone: `trunc` drops the high limb with no
/// `u32` instruction, as a split would. And a load every use of which takes a half is left to
/// [`SplitWideLoad`], which this pattern declines.
struct SplitTakenHalves {
    info: PatternInfo,
}

impl SplitTakenHalves {
    fn new(context: Rc<Context>) -> Self {
        let arith = context.get_or_register_dialect::<arith::ArithDialect>();
        let hir = context.get_or_register_dialect::<hir::HirDialect>();
        let mut info = PatternInfo::new(
            context.clone(),
            "split-taken-halves",
            PatternKind::Operation(arith.expect_registered_name::<arith::Trunc>()),
            PatternBenefit::new(1),
        );
        info.with_generated_ops([
            arith.expect_registered_name::<arith::Split>(),
            hir.expect_registered_name::<hir::Bitcast>(),
        ]);
        Self { info }
    }
}

impl Pattern for SplitTakenHalves {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl ConversionPattern for SplitTakenHalves {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        _operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report> {
        let Some(SplitHalves { value, halves }) = SplitHalves::at(&op.borrow()) else {
            return Ok(false);
        };
        let first = halves
            .lo
            .iter()
            .chain(&halves.hi)
            .copied()
            .reduce(|first, trunc| {
                if trunc.borrow().is_before_in_block(&first) {
                    trunc
                } else {
                    first
                }
            })
            .expect("the `trunc` matched takes a half");

        let span = first.borrow().span();
        let limb_ty = result_type(op);
        rewriter.set_insertion_point_before(first);
        let split = rewriter.create_op::<arith::Split, _>(span, (value, limb_ty.clone()))?;
        let [hi, lo] = {
            let split = split.borrow();
            let limbs = split.limbs();
            [0, 1].map(|limb| limbs[limb].borrow().as_value_ref())
        };
        // Every replacement is built before `first`, the insertion point, is replaced
        let mut replacements = SmallVec::<[(OperationRef, ValueRef); 4]>::new();
        for (limb, truncs) in [(lo, &halves.lo), (hi, &halves.hi)] {
            for &trunc in truncs {
                let ty = result_type(trunc);
                let replacement = if ty == limb_ty {
                    limb
                } else {
                    let span = trunc.borrow().span();
                    let cast = rewriter.create_op::<hir::Bitcast, _>(span, (limb, ty))?;
                    cast.borrow().result().as_value_ref()
                };
                replacements.push((trunc, replacement));
            }
        }
        // What each `trunc` took its half of, for the ops left dead once it is replaced
        let mut truncated = SmallVec::<[ValueRef; 4]>::new();
        for (trunc, replacement) in replacements {
            truncated.push(trunc.borrow().operands()[0].borrow().as_value_ref());
            rewriter.replace_op(trunc, &[replacement])?;
        }
        // The `shr` and casts of the high half are dead now; the integer is the split's operand
        erase_dead_defs(rewriter, truncated)?;
        Ok(true)
    }
}

/// The uses of a 64-bit integer that [`SplitTakenHalves`] takes from one split of it, when it
/// matches at a `trunc`: those in the `trunc`'s block that take a half, one of them the high half.
struct SplitHalves {
    /// The integer
    value: ValueRef,
    /// Its uses in the block that take a half
    halves: TakenHalves,
}

impl SplitHalves {
    fn at(op: &Operation) -> Option<Self> {
        let trunc = op.downcast_ref::<arith::Trunc>()?;
        let value = half_taken_from(trunc)?;
        let def = value.borrow().get_defining_op();
        if def.is_some_and(|def| TakenHalves::loaded_by(&def.borrow()).is_some()) {
            return None;
        }
        // Decided per block, so that splitting the halves in one block does not change whether
        // they are split in another
        let mut halves = TakenHalves::of(value);
        let block = op.parent();
        halves.lo.retain(|trunc| trunc.parent() == block);
        halves.hi.retain(|trunc| trunc.parent() == block);
        if halves.hi.is_empty() {
            return None;
        }
        // Always so, as `half_taken_from` and `TakenHalves::of` see the same shapes; if they
        // disagreed, the pattern would not replace the `trunc` it matched
        let root = op.as_operation_ref();
        let has_root = halves.lo.contains(&root) || halves.hi.contains(&root);
        has_root.then_some(Self { value, halves })
    }
}

/// The 64-bit integer of which `trunc` takes a 32-bit half: for the high half, what the logical
/// `shr` by 32 it truncates shifts, or else, for the low half, what it truncates; sign-only casts
/// looked through.
fn half_taken_from(trunc: &arith::Trunc) -> Option<ValueRef> {
    if !matches!(trunc.result().ty(), Type::I32 | Type::U32) {
        return None;
    }
    let truncated = through_sign_casts(trunc.operand().as_value_ref());
    if !is_64bit_integer(truncated.borrow().ty()) {
        return None;
    }
    Some(shifted_right_by_32(truncated).map_or(truncated, through_sign_casts))
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Half {
    Lo,
    Hi,
}

/// The ops [`half_address`] builds.
fn half_address_ops(context: &Rc<Context>) -> [OperationName; 5] {
    let hir = context.get_or_register_dialect::<hir::HirDialect>();
    let arith = context.get_or_register_dialect::<arith::ArithDialect>();
    [
        hir.expect_registered_name::<hir::Bitcast>(),
        hir.expect_registered_name::<hir::PtrToInt>(),
        hir.expect_registered_name::<hir::IntToPtr>(),
        arith.expect_registered_name::<arith::Constant>(),
        arith.expect_registered_name::<arith::Add>(),
    ]
}

/// The address of `half` of the 64-bit value `addr` points to, as a pointer to `half_ty` in the
/// same address space: `addr` itself for the low half, and for the high half `addr` plus 4 bytes,
/// or plus one element in element space, checked for overflow as `intrinsics::mem::store_dw` and
/// `load_dw` check the address of the high half.
fn half_address(
    rewriter: &mut ConversionPatternRewriter,
    addr: ValueRef,
    half: Half,
    half_ty: Type,
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    let (addrspace, stride) = match addr.borrow().ty() {
        Type::Ptr(pointer) => (pointer.addrspace(), if pointer.is_byte_pointer() { 4 } else { 1 }),
        ty => unreachable!("a memory op's address is a pointer, not {ty}"),
    };
    let half_ptr = Type::from(PointerType::new_with_address_space(half_ty, addrspace));
    let half_addr = match half {
        Half::Lo => rewriter
            .create_op::<hir::Bitcast, _>(span, (addr, half_ptr))?
            .as_operation_ref(),
        Half::Hi => {
            let base = rewriter.create_op::<hir::PtrToInt, _>(span, (addr, Type::U32))?;
            let base = base.borrow().result().as_value_ref();
            let stride =
                rewriter.create_op::<arith::Constant, _>(span, (Immediate::U32(stride),))?;
            let stride = stride.borrow().result().as_value_ref();
            let sum =
                rewriter.create_op::<arith::Add, _>(span, (base, stride, Overflow::Checked))?;
            let sum = sum.borrow().result().as_value_ref();
            rewriter
                .create_op::<hir::IntToPtr, _>(span, (sum, half_ptr))?
                .as_operation_ref()
        }
    };
    Ok(half_addr.borrow().results()[0].borrow().as_value_ref())
}

/// Erase the ops defining `values` that a split left without uses, and in turn the ops defining
/// their operands, so long as they have no effect but a read. As for region DCE, uses by debug
/// info do not keep a value alive; they are erased with it.
fn erase_dead_defs(
    rewriter: &mut ConversionPatternRewriter,
    values: impl IntoIterator<Item = ValueRef>,
) -> Result<(), Report> {
    let mut worklist = values.into_iter().collect::<SmallVec<[ValueRef; 4]>>();
    while let Some(value) = worklist.pop() {
        let Some(def) = value.borrow().get_defining_op() else {
            continue;
        };
        // An op reached twice is erased the first time
        if def.parent().is_none() {
            continue;
        }
        let operands = {
            let op = def.borrow();
            let dead = op.results().iter().all(|result| !result.borrow().has_real_uses())
                && op.would_be_trivially_dead();
            if !dead {
                continue;
            }
            op.operands()
                .iter()
                .map(|operand| operand.borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 2]>>()
        };
        // `erase_op` erases the debug users with it
        rewriter.erase_op(def)?;
        worklist.extend(operands);
    }
    Ok(())
}

fn is_64bit_integer(ty: &Type) -> bool {
    matches!(ty, Type::I64 | Type::U64)
}

/// Whether a `hir.bitcast` from `from` to `to` changes nothing but the signedness of an integer
/// of the widths the splits deal in.
fn differs_only_in_signedness(from: &Type, to: &Type) -> bool {
    matches!(
        (from, to),
        (Type::I32 | Type::U32, Type::I32 | Type::U32)
            | (Type::I64 | Type::U64, Type::I64 | Type::U64)
    )
}

/// The op of type `T` that defines `value`, if one does.
fn defined_by<T: Op>(value: ValueRef) -> Option<UnsafeIntrusiveEntityRef<T>> {
    value.borrow().get_defining_op()?.try_downcast_op::<T>().ok()
}

/// `value` with the sign-only `hir.bitcast`s that produced it looked through.
fn through_sign_casts(mut value: ValueRef) -> ValueRef {
    while let Some(cast) = defined_by::<hir::Bitcast>(value) {
        let operand = cast.borrow().operand().as_value_ref();
        if !differs_only_in_signedness(operand.borrow().ty(), value.borrow().ty()) {
            break;
        }
        value = operand;
    }
    value
}

/// The `u32` that `value`, a 64-bit integer, zero-extends.
fn zero_extended_u32(value: ValueRef) -> Option<ValueRef> {
    let zext = defined_by::<arith::Zext>(through_sign_casts(value))?;
    let half = zext.borrow().operand().as_value_ref();
    let is_u32 = *half.borrow().ty() == Type::U32;
    is_u32.then_some(half)
}

/// The value that `value` shifts left by 32 bits.
fn shifted_left_by_32(value: ValueRef) -> Option<ValueRef> {
    let shl = defined_by::<arith::Shl>(through_sign_casts(value))?;
    let shl = shl.borrow();
    (constant_u32(shl.shift().as_value_ref()) == Some(32)).then(|| shl.lhs().as_value_ref())
}

/// The `u64` that `value` shifts right by 32 bits, a logical shift.
fn shifted_right_by_32(value: ValueRef) -> Option<ValueRef> {
    let shr = defined_by::<arith::Shr>(through_sign_casts(value))?;
    let shr = shr.borrow();
    let shifted = shr.lhs().as_value_ref();
    let logical = *shifted.borrow().ty() == Type::U64;
    (logical && constant_u32(shr.shift().as_value_ref()) == Some(32)).then_some(shifted)
}

/// The value of `value` when it is a `u32` constant, or the `band` of two, which is how the Wasm
/// frontend masks every shift count and nothing folds.
fn constant_u32(value: ValueRef) -> Option<u32> {
    if let Some(constant) = defined_by::<arith::Constant>(value) {
        return constant.borrow().value().as_u32();
    }
    let band = defined_by::<arith::Band>(value)?;
    let band = band.borrow();
    Some(constant_u32(band.lhs().as_value_ref())? & constant_u32(band.rhs().as_value_ref())?)
}

/// The bits of `value`, a 64-bit integer, when it is a constant, sign-only casts looked through.
fn constant_u64(value: ValueRef) -> Option<u64> {
    let constant = defined_by::<arith::Constant>(through_sign_casts(value))?;
    let constant = constant.borrow();
    is_64bit_integer(constant.result().ty()).then(|| constant.value().bitcast_u64())?
}

fn result_type(op: OperationRef) -> Type {
    op.borrow().results()[0].borrow().ty().clone()
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format};

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_cf::ControlFlowOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;
    use midenc_expect_test::{Expect, expect};
    use midenc_hir::{
        AddressSpace, CallConv, Felt, Ident, OpBuilder, PointerType, SourceSpan, Type, ValueRef,
        Visibility,
        dialects::builtin::{
            BuiltinOpBuilder, FunctionBuilder, ModuleBuilder,
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
        assert!(message.contains("implements neither HirLowering nor TransparentCast"));
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

    /// Build a `dispatch` function whose `hir.dyncall` uses `signature`, passing one u32 constant
    /// per parameter and four felt constants as the callee root.
    fn test_with_dyncall(test: &mut Test, signature: Signature) {
        test.with_function("dispatch", &[], &[]);
        let arity = signature.params.len();
        let mut builder = test.function_builder();
        let root: [ValueRef; hir::Dyncall::ROOT_FELTS] = core::array::from_fn(|element| {
            builder.felt(Felt::new(element as u64 + 1).expect("a small felt"), SourceSpan::UNKNOWN)
        });
        let args = (0..arity)
            .map(|_| builder.u32(0, SourceSpan::UNKNOWN))
            .collect::<alloc::vec::Vec<_>>();
        builder.dyncall(root, signature, args, SourceSpan::UNKNOWN).unwrap();
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

    /// The table index takes a single element of the window, so the widest legal call is the one
    /// filling the remaining fifteen with argument field elements.
    #[test]
    fn exec_indirect_arguments_at_the_budget_pass_legalization() {
        let mut test = Test::named("budgeted_exec_indirect").in_module("m");
        let signature = Signature::new(&test.context_rc(), vec![Type::U32; 15], []);
        test_with_exec_indirect(&mut test, signature);

        test.apply_pass::<LegalizeForMasm>(true).unwrap();
    }

    /// A `hir.dyncall`'s callee root is a whole word, not one element, so it is bounded three
    /// elements tighter than an `hir.exec_indirect`. Scheduling thirteen argument field elements
    /// beside it would ask the spill analysis for seventeen reachable operands, which it cannot
    /// deliver.
    ///
    /// That analysis runs *before* this pass in the backend pipeline, so it, not this rule, is
    /// what such a call meets there — it reports one as a diagnostic of its own. This rule is what
    /// makes the bound part of the IR contract codegen accepts, for the callers that legalize
    /// without running the rewrites.
    #[test]
    fn oversized_dyncall_arguments_fail_legalization() {
        let mut test = Test::named("oversized_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            vec![Type::U32; 13],
            [],
        );
        test_with_dyncall(&mut test, signature);

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("operand stack window"), "{message}");
    }

    /// The mirror of the above at the bound itself: twelve argument field elements share the
    /// window with the root word exactly, which is the widest stored procedure the SDK frontend
    /// can produce.
    #[test]
    fn dyncall_arguments_at_the_budget_pass_legalization() {
        let mut test = Test::named("budgeted_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            vec![Type::U32; 12],
            [],
        );
        test_with_dyncall(&mut test, signature);

        test.apply_pass::<LegalizeForMasm>(true).unwrap();
    }

    /// The emitter consumes one operand per parameter, so a signature declaring more parameters
    /// than the call passes would pop an empty operand stack. The op verifier rejects this too,
    /// but legalization runs with the verifier off in the pipeline's conversion driver.
    #[test]
    fn dyncall_with_too_few_arguments_fails_legalization() {
        let mut test = Test::named("short_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            vec![Type::U32; 2],
            [],
        );
        test_with_dyncall(&mut test, signature.clone());
        // Widen the signature behind the built op, as a producer that did not go through the
        // builder could hand codegen
        set_indirect_call_signature(
            &test,
            Signature::with_convention(
                &test.context_rc(),
                CallConv::ComponentModel,
                vec![Type::U32; 3],
                [],
            ),
        );

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("2 argument(s)"), "{message}");
        assert!(message.contains("3 parameter(s)"), "{message}");
    }

    /// An argument whose type differs from its (unextended) parameter reaches an `assert_eq!` in
    /// the emitter; legalization must reject it first, as the verifier is off here.
    #[test]
    fn mistyped_dyncall_argument_fails_legalization() {
        let mut test = Test::named("mistyped_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            [Type::U32],
            [],
        );
        test_with_dyncall(&mut test, signature);
        set_indirect_call_signature(
            &test,
            Signature::with_convention(
                &test.context_rc(),
                CallConv::ComponentModel,
                [Type::Felt],
                [],
            ),
        );

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("parameter 0"), "{message}");
        assert!(message.contains("cannot convert"), "{message}");
    }

    /// The lowering spills a whole word to the cell the VM reads the callee digest from, so a
    /// narrower root operand group would spill stack elements the call does not own. The op
    /// verifier rejects this too, but legalization runs with the verifier off here.
    #[test]
    fn dyncall_with_a_partial_root_word_fails_legalization() {
        let mut test = Test::named("partial_root_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            [Type::U32],
            [],
        );
        test_with_dyncall(&mut test, signature);
        // The builder always passes a whole word, so drop one element afterwards to build the IR
        // a producer that did not go through the builder could hand codegen
        {
            let context = test.context_rc();
            let mut dyncall = find_dyncall(&test);
            let root = {
                let dyncall = dyncall.borrow();
                dyncall
                    .downcast_ref::<hir::Dyncall>()
                    .expect("the walk selected a hir.dyncall")
                    .root()
                    .iter()
                    .take(hir::Dyncall::ROOT_FELTS - 1)
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<alloc::vec::Vec<_>>()
            };
            let mut dyncall = dyncall.borrow_mut();
            let owner = dyncall.as_operation_ref();
            dyncall.operands_mut().group_mut(0).set_operands(root, owner, &context);
        }

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("3 root field element(s)"), "{message}");
    }

    /// The lowering pops the root word element by element as field elements, which the emitter
    /// asserts, so a root element of any other type would reach that assertion. The op's operand
    /// type constraint says the same thing, but legalization runs with the verifier off here.
    #[test]
    fn dyncall_with_a_mistyped_root_element_fails_legalization() {
        let mut test = Test::named("mistyped_root_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            [Type::U32],
            [],
        );
        test_with_dyncall(&mut test, signature);
        // The builder derives the root operands from felt values, so substitute the call's `u32`
        // argument for one of them afterwards, as a producer that did not go through the builder
        // could hand codegen
        {
            let context = test.context_rc();
            let mut dyncall = find_dyncall(&test);
            let root = {
                let dyncall = dyncall.borrow();
                let dyncall = dyncall
                    .downcast_ref::<hir::Dyncall>()
                    .expect("the walk selected a hir.dyncall");
                let mut root = dyncall
                    .root()
                    .iter()
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<alloc::vec::Vec<_>>();
                root[hir::Dyncall::ROOT_FELTS - 1] = dyncall
                    .arguments()
                    .iter()
                    .next()
                    .expect("the call passes one argument")
                    .borrow()
                    .as_value_ref();
                root
            };
            let mut dyncall = dyncall.borrow_mut();
            let owner = dyncall.as_operation_ref();
            dyncall.operands_mut().group_mut(0).set_operands(root, owner, &context);
        }

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("root element 3 of type u32"), "{message}");
    }

    /// Only the component-model convention crosses the context switch on the operand stack alone;
    /// the lowering has no return-pointer handling, so any other convention is unlowerable. The op
    /// verifier rejects this too, but legalization runs with the verifier off here.
    #[test]
    fn dyncall_with_a_foreign_calling_convention_fails_legalization() {
        let mut test = Test::named("wasm_cc_dyncall").in_module("m");
        let signature = Signature::with_convention(
            &test.context_rc(),
            CallConv::ComponentModel,
            [Type::U32],
            [],
        );
        test_with_dyncall(&mut test, signature);
        set_indirect_call_signature(
            &test,
            Signature::with_convention(&test.context_rc(), CallConv::Wasm, [Type::U32], []),
        );

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("component-model calling convention is supported"), "{message}");
    }

    /// Replace the signature of the one `hir.dyncall` in `test`'s primary function.
    ///
    /// The op builder derives the operands from the signature, so a call disagreeing with its
    /// signature can only be built by rewriting the signature afterwards — which is exactly the
    /// IR another producer could hand codegen.
    fn set_indirect_call_signature(test: &Test, signature: Signature) {
        let mut dyncall = find_dyncall(test);
        let mut dyncall = dyncall.borrow_mut();
        dyncall
            .downcast_mut::<hir::Dyncall>()
            .expect("the walk selected a hir.dyncall")
            .set_signature(signature);
    }

    /// The one `hir.dyncall` in `test`'s primary function.
    fn find_dyncall(test: &Test) -> OperationRef {
        let mut dyncall = None;
        let function = test.function().as_operation_ref();
        let function = function.borrow();
        function.postwalk_all(|op| {
            if op.downcast_ref::<hir::Dyncall>().is_some() {
                dyncall = Some(op.as_operation_ref());
            }
        });
        dyncall.expect("the test builds a hir.dyncall")
    }

    /// The indirect-call lowering cannot apply argument extension, since the stack top holds the
    /// transient slot address while arguments are consumed. Only a parameter that would really
    /// have to widen its argument demands it, so this passes a `u32` where an `i64` is expected.
    #[test]
    fn extension_requiring_exec_indirect_arguments_fail_legalization() {
        let mut test = Test::named("extension_exec_indirect").in_module("m");
        let mut signature = Signature::new(&test.context_rc(), [Type::I64], []);
        signature.params[0] = AbiParam::sext(Type::I64, &test.context_rc());
        test_with_exec_indirect(&mut test, signature);

        let err = test.apply_pass::<LegalizeForMasm>(false).unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("cannot extend the argument for parameter 0"), "{message}");
    }

    /// Canonical-ABI flattening marks the parameters it widened and hands over an argument that
    /// already has the flat type, so the extension is a no-op the lowering can serve — the same
    /// conclusion a direct call reaches. Rejecting these outright would make every stored
    /// procedure taking a `bool`, `u8`, `u16`, `i8` or `i16` unlowerable.
    #[test]
    fn no_op_extension_exec_indirect_arguments_pass_legalization() {
        let mut test = Test::named("no_op_extension_exec_indirect").in_module("m");
        let mut signature = Signature::new(&test.context_rc(), [Type::U32, Type::U32], []);
        signature.params[0] = AbiParam::zext(Type::U32, &test.context_rc());
        signature.params[1] = AbiParam::sext(Type::U32, &test.context_rc());
        test_with_exec_indirect(&mut test, signature);

        test.apply_pass::<LegalizeForMasm>(true).unwrap();
    }

    /// Run `LegalizeForMasm` over `test`'s function, pinning its HIR before and after.
    fn assert_legalizes(test: &Test, before: Expect, after: Expect) {
        before.assert_eq(&test.function().borrow().as_operation().to_string());
        test.apply_pass::<LegalizeForMasm>(true).unwrap();
        after.assert_eq(&test.function().borrow().as_operation().to_string());
    }

    /// Run `LegalizeForMasm` over `test`'s function, pinning its HIR, which must not change.
    fn assert_left_alone(test: &Test, hir: Expect) {
        let before = test.function().borrow().as_operation().to_string();
        hir.assert_eq(&before);
        test.apply_pass::<LegalizeForMasm>(true).unwrap();
        assert_eq!(test.function().borrow().as_operation().to_string(), before);
    }

    fn pointer_to(pointee: Type, addrspace: AddressSpace) -> Type {
        Type::from(PointerType::new_with_address_space(pointee, addrspace))
    }

    /// `i64.extend_i32_u` of the `i32` value `half`, as the Wasm frontend spells it.
    fn wasm_extend_i32_u(builder: &mut FunctionBuilder<'_, OpBuilder>, half: ValueRef) -> ValueRef {
        let span = SourceSpan::UNKNOWN;
        let half = builder.bitcast(half, Type::U32, span).unwrap();
        let half = builder.zext(half, Type::U64, span).unwrap();
        builder.bitcast(half, Type::I64, span).unwrap()
    }

    /// The shift count 32 of an `i64` shift, masked to the shift width as the Wasm frontend
    /// masks every shift count.
    fn wasm_shift_count_32(builder: &mut FunctionBuilder<'_, OpBuilder>) -> ValueRef {
        let span = SourceSpan::UNKNOWN;
        let count = builder.u32(32, span);
        let mask = builder.u32(63, span);
        builder.band(count, mask, span).unwrap()
    }

    /// A 64-bit store of a value assembled from two 32-bit halves is two 32-bit stores: the
    /// shape in which LLVM's IR-level passes store two 32-bit values they carry as one `i64`, as
    /// the Wasm frontend translates it.
    #[test]
    fn a_store_of_two_merged_halves_is_split() {
        let mut test = Test::new(
            "a_store_of_two_merged_halves_is_split",
            &[pointer_to(Type::I64, AddressSpace::Byte), Type::I32, Type::I32],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let lo = wasm_extend_i32_u(&mut builder, lo);
            let hi = wasm_extend_i32_u(&mut builder, hi);
            let count = wasm_shift_count_32(&mut builder);
            let hi = builder.shl(hi, count, span).unwrap();
            let value = builder.bor(lo, hi, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
            builtin.function public extern("C") @a_store_of_two_merged_halves_is_split(%0: ptr<i64, byte>, %1: i32, %2: i32) {
                %3 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                %4 = arith.zext %3 <{ ty = #builtin.type<u64> }>;
                %5 = hir.bitcast %4 <{ ty = #builtin.type<i64> }>;
                %6 = hir.bitcast %2 <{ ty = #builtin.type<u32> }>;
                %7 = arith.zext %6 <{ ty = #builtin.type<u64> }>;
                %8 = hir.bitcast %7 <{ ty = #builtin.type<i64> }>;
                %9 = arith.constant 32 : u32;
                %10 = arith.constant 63 : u32;
                %11 = arith.band %9, %10;
                %12 = arith.shl %8, %11;
                %13 = arith.bor %5, %12;
                hir.store %0, %13 : (ptr<i64, byte>, i64);
                builtin.ret;
            };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_store_of_two_merged_halves_is_split(%0: ptr<i64, byte>, %1: i32, %2: i32) {
                    %3 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                    %6 = hir.bitcast %2 <{ ty = #builtin.type<u32> }>;
                    %17 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, byte>> }>;
                    hir.store %17, %3 : (ptr<u32, byte>, u32);
                    %18 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %19 = arith.constant 4 : u32;
                    %20 = arith.add %18, %19 <{ overflow = #builtin.overflow<checked> }>;
                    %21 = hir.int_to_ptr %20 <{ ty = #builtin.type<ptr<u32, byte>> }>;
                    hir.store %21, %6 : (ptr<u32, byte>, u32);
                    builtin.ret;
                };"#]],
        );
    }

    /// The `or` is commutative, and the split addresses the high half one element on in element
    /// space.
    #[test]
    fn the_halves_may_come_in_either_order() {
        let mut test = Test::new(
            "the_halves_may_come_in_either_order",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let lo = builder.zext(lo, Type::U64, span).unwrap();
            let hi = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let hi = builder.shl(hi, count, span).unwrap();
            let value = builder.bor(hi, lo, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
            builtin.function public extern("C") @the_halves_may_come_in_either_order(%0: ptr<u64, element>, %1: u32, %2: u32) {
                %3 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                %4 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                %5 = arith.constant 32 : u32;
                %6 = arith.shl %4, %5;
                %7 = arith.bor %6, %3;
                hir.store %0, %7 : (ptr<u64, element>, u64);
                builtin.ret;
            };"#]],
            expect![[r#"
                builtin.function public extern("C") @the_halves_may_come_in_either_order(%0: ptr<u64, element>, %1: u32, %2: u32) {
                    %11 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %11, %1 : (ptr<u32, element>, u32);
                    %12 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %13 = arith.constant 1 : u32;
                    %14 = arith.add %12, %13 <{ overflow = #builtin.overflow<checked> }>;
                    %15 = hir.int_to_ptr %14 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %15, %2 : (ptr<u32, element>, u32);
                    builtin.ret;
                };"#]],
        );
    }

    /// A 64-bit load whose every use takes one 32-bit half of it is two 32-bit loads.
    #[test]
    fn a_load_consumed_only_as_two_halves_is_split() {
        let mut test = Test::new(
            "a_load_consumed_only_as_two_halves_is_split",
            &[pointer_to(Type::I64, AddressSpace::Byte)],
            &[Type::I32, Type::I32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let addr = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let value = builder.load(addr, span).unwrap();
            let lo = builder.trunc(value, Type::I32, span).unwrap();
            // `i64.shr_u` and `i32.wrap_i64`, as the Wasm frontend spells them
            let unsigned = builder.bitcast(value, Type::U64, span).unwrap();
            let count = wasm_shift_count_32(&mut builder);
            let shifted = builder.shr(unsigned, count, span).unwrap();
            let shifted = builder.bitcast(shifted, Type::I64, span).unwrap();
            let hi = builder.trunc(shifted, Type::I32, span).unwrap();
            builder.ret([lo, hi], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
            builtin.function public extern("C") @a_load_consumed_only_as_two_halves_is_split(%0: ptr<i64, byte>) -> (i32, i32) {
                %1 = hir.load %0;
                %2 = arith.trunc %1 <{ ty = #builtin.type<i32> }>;
                %3 = hir.bitcast %1 <{ ty = #builtin.type<u64> }>;
                %4 = arith.constant 32 : u32;
                %5 = arith.constant 63 : u32;
                %6 = arith.band %4, %5;
                %7 = arith.shr %3, %6;
                %8 = hir.bitcast %7 <{ ty = #builtin.type<i64> }>;
                %9 = arith.trunc %8 <{ ty = #builtin.type<i32> }>;
                builtin.ret %2, %9 : (i32, i32);
            };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_load_consumed_only_as_two_halves_is_split(%0: ptr<i64, byte>) -> (i32, i32) {
                    %10 = hir.bitcast %0 <{ ty = #builtin.type<ptr<i32, byte>> }>;
                    %11 = hir.load %10;
                    %12 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %13 = arith.constant 4 : u32;
                    %14 = arith.add %12, %13 <{ overflow = #builtin.overflow<checked> }>;
                    %15 = hir.int_to_ptr %14 <{ ty = #builtin.type<ptr<i32, byte>> }>;
                    %16 = hir.load %15;
                    builtin.ret %11, %16 : (i32, i32);
                };"#]],
        );
    }

    /// Debug info does not keep a 64-bit value the splits leave dead; it goes with the value.
    #[test]
    fn debug_uses_go_with_the_values_a_split_leaves_dead() {
        use midenc_hir::{
            dialects::debuginfo::{DIBuilder, DebugInfoDialect, attributes::Variable},
            interner::Symbol,
        };

        let mut test = Test::new(
            "debug_uses_go_with_the_values_a_split_leaves_dead",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32],
            &[Type::U32],
        );
        test.context().get_or_register_dialect::<DebugInfoDialect>();
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let lo = builder.zext(lo, Type::U64, span).unwrap();
            let hi = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let hi = builder.shl(hi, count, span).unwrap();
            let value = builder.bor(lo, hi, span).unwrap();
            let variable =
                Variable::new(Symbol::intern("pair"), Symbol::intern("test.rs"), 1, None);
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value(value, variable, span)
                .unwrap();
            builder.store(addr, value, span).unwrap();
            let reloaded = builder.load(addr, span).unwrap();
            let variable =
                Variable::new(Symbol::intern("reloaded"), Symbol::intern("test.rs"), 2, None);
            BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value(reloaded, variable, span)
                .unwrap();
            let lo = builder.trunc(reloaded, Type::U32, span).unwrap();
            builder.ret([lo], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
            builtin.function public extern("C") @debug_uses_go_with_the_values_a_split_leaves_dead(%0: ptr<u64, element>, %1: u32, %2: u32) -> u32 {
                %3 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                %4 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                %5 = arith.constant 32 : u32;
                %6 = arith.shl %4, %5;
                %7 = arith.bor %3, %6;
                di.debug_value %7 <{ variable = #di.variable<{ name = "pair", file = "test.rs", line = 1 }>, expression = #di.expression<[]> }> : (u64);
                hir.store %0, %7 : (ptr<u64, element>, u64);
                %8 = hir.load %0;
                di.debug_value %8 <{ variable = #di.variable<{ name = "reloaded", file = "test.rs", line = 2 }>, expression = #di.expression<[]> }> : (u64);
                %9 = arith.trunc %8 <{ ty = #builtin.type<u32> }>;
                builtin.ret %9 : (u32);
            };"#]],
            expect![[r#"
                builtin.function public extern("C") @debug_uses_go_with_the_values_a_split_leaves_dead(%0: ptr<u64, element>, %1: u32, %2: u32) -> u32 {
                    %13 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %13, %1 : (ptr<u32, element>, u32);
                    %14 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %15 = arith.constant 1 : u32;
                    %16 = arith.add %14, %15 <{ overflow = #builtin.overflow<checked> }>;
                    %17 = hir.int_to_ptr %16 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %17, %2 : (ptr<u32, element>, u32);
                    %18 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    %19 = hir.load %18;
                    builtin.ret %19 : (u32);
                };"#]],
        );
    }

    /// Only the exact shape is split, or joined: halves of 32 bits, zero-extended, shifted apart by
    /// 32.
    #[test]
    fn a_store_whose_value_is_not_two_halves_is_left_alone() {
        let mut test = Test::new(
            "a_store_whose_value_is_not_two_halves_is_left_alone",
            &[pointer_to(Type::I64, AddressSpace::Byte), Type::I32, Type::I32, Type::U16],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi, short] =
                [args[0], args[1], args[2], args[3]].map(|arg| arg as ValueRef);
            // Shifted apart by 16
            let lo_ext = wasm_extend_i32_u(&mut builder, lo);
            let hi_ext = wasm_extend_i32_u(&mut builder, hi);
            let count = builder.u32(16, span);
            let shifted = builder.shl(hi_ext, count, span).unwrap();
            let value = builder.bor(lo_ext, shifted, span).unwrap();
            builder.store(addr, value, span).unwrap();
            // Sign-extended
            let lo_ext = builder.sext(lo, Type::I64, span).unwrap();
            let hi_ext = builder.sext(hi, Type::I64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shl(hi_ext, count, span).unwrap();
            let value = builder.bor(lo_ext, shifted, span).unwrap();
            builder.store(addr, value, span).unwrap();
            // A 16-bit half
            let lo_ext = wasm_extend_i32_u(&mut builder, lo);
            let hi_ext = builder.zext(short, Type::U64, span).unwrap();
            let hi_ext = builder.bitcast(hi_ext, Type::I64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shl(hi_ext, count, span).unwrap();
            let value = builder.bor(lo_ext, shifted, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_left_alone(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_store_whose_value_is_not_two_halves_is_left_alone(%0: ptr<i64, byte>, %1: i32, %2: i32, %3: u16) {
                    %4 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                    %5 = arith.zext %4 <{ ty = #builtin.type<u64> }>;
                    %6 = hir.bitcast %5 <{ ty = #builtin.type<i64> }>;
                    %7 = hir.bitcast %2 <{ ty = #builtin.type<u32> }>;
                    %8 = arith.zext %7 <{ ty = #builtin.type<u64> }>;
                    %9 = hir.bitcast %8 <{ ty = #builtin.type<i64> }>;
                    %10 = arith.constant 16 : u32;
                    %11 = arith.shl %9, %10;
                    %12 = arith.bor %6, %11;
                    hir.store %0, %12 : (ptr<i64, byte>, i64);
                    %13 = arith.sext %1 <{ ty = #builtin.type<i64> }>;
                    %14 = arith.sext %2 <{ ty = #builtin.type<i64> }>;
                    %15 = arith.constant 32 : u32;
                    %16 = arith.shl %14, %15;
                    %17 = arith.bor %13, %16;
                    hir.store %0, %17 : (ptr<i64, byte>, i64);
                    %18 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                    %19 = arith.zext %18 <{ ty = #builtin.type<u64> }>;
                    %20 = hir.bitcast %19 <{ ty = #builtin.type<i64> }>;
                    %21 = arith.zext %3 <{ ty = #builtin.type<u64> }>;
                    %22 = hir.bitcast %21 <{ ty = #builtin.type<i64> }>;
                    %23 = arith.constant 32 : u32;
                    %24 = arith.shl %22, %23;
                    %25 = arith.bor %20, %24;
                    hir.store %0, %25 : (ptr<i64, byte>, i64);
                    builtin.ret;
                };"#]],
        );
    }

    /// A load that is also used whole stays one 64-bit load.
    #[test]
    fn a_load_with_a_use_of_the_whole_value_is_left_alone() {
        let mut test = Test::new(
            "a_load_with_a_use_of_the_whole_value_is_left_alone",
            &[pointer_to(Type::I64, AddressSpace::Byte)],
            &[Type::I32, Type::I64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let addr = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let value = builder.load(addr, span).unwrap();
            let lo = builder.trunc(value, Type::I32, span).unwrap();
            builder.ret([lo, value], span).unwrap();
        }

        assert_left_alone(
            &test,
            expect![[r#"
            builtin.function public extern("C") @a_load_with_a_use_of_the_whole_value_is_left_alone(%0: ptr<i64, byte>) -> (i32, i64) {
                %1 = hir.load %0;
                %2 = arith.trunc %1 <{ ty = #builtin.type<i32> }>;
                builtin.ret %2, %1 : (i32, i64);
            };"#]],
        );
    }

    /// Only a `trunc` to 32 bits takes a half, and only of the load or of its logical `shr` by 32,
    /// so each of these loads stays one 64-bit load. The `shr` of the high half is not lost on
    /// [`SplitTakenHalves`], though: it takes the high half of the first `shr`'s result, which
    /// is exactly that, and that half comes from a split.
    #[test]
    fn a_load_whose_halves_are_not_taken_exactly_stays_one_load() {
        let mut test = Test::new(
            "a_load_whose_halves_are_not_taken_exactly_stays_one_load",
            &[pointer_to(Type::I64, AddressSpace::Byte)],
            &[Type::I32, Type::U16, Type::U32, Type::U32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let addr = builder.entry_block().borrow().arguments()[0] as ValueRef;
            // An arithmetic `shr`, which is what a `shr` of an `i64` is in codegen
            let value = builder.load(addr, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let arithmetic = builder.trunc(shifted, Type::I32, span).unwrap();
            // A `trunc` to fewer than 32 bits
            let value = builder.load(addr, span).unwrap();
            let narrow = builder.trunc(value, Type::U16, span).unwrap();
            // A `shr` of the high half
            let value = builder.load(addr, span).unwrap();
            let unsigned = builder.bitcast(value, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(unsigned, count, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(shifted, count, span).unwrap();
            let twice = builder.trunc(shifted, Type::U32, span).unwrap();
            // A shift by 16
            let value = builder.load(addr, span).unwrap();
            let unsigned = builder.bitcast(value, Type::U64, span).unwrap();
            let count = builder.u32(16, span);
            let shifted = builder.shr(unsigned, count, span).unwrap();
            let by_16 = builder.trunc(shifted, Type::U32, span).unwrap();
            builder.ret([arithmetic, narrow, twice, by_16], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
            builtin.function public extern("C") @a_load_whose_halves_are_not_taken_exactly_stays_one_load(%0: ptr<i64, byte>) -> (i32, u16, u32, u32) {
                %1 = hir.load %0;
                %2 = arith.constant 32 : u32;
                %3 = arith.shr %1, %2;
                %4 = arith.trunc %3 <{ ty = #builtin.type<i32> }>;
                %5 = hir.load %0;
                %6 = arith.trunc %5 <{ ty = #builtin.type<u16> }>;
                %7 = hir.load %0;
                %8 = hir.bitcast %7 <{ ty = #builtin.type<u64> }>;
                %9 = arith.constant 32 : u32;
                %10 = arith.shr %8, %9;
                %11 = arith.constant 32 : u32;
                %12 = arith.shr %10, %11;
                %13 = arith.trunc %12 <{ ty = #builtin.type<u32> }>;
                %14 = hir.load %0;
                %15 = hir.bitcast %14 <{ ty = #builtin.type<u64> }>;
                %16 = arith.constant 16 : u32;
                %17 = arith.shr %15, %16;
                %18 = arith.trunc %17 <{ ty = #builtin.type<u32> }>;
                builtin.ret %4, %6, %13, %18 : (i32, u16, u32, u32);
            };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_load_whose_halves_are_not_taken_exactly_stays_one_load(%0: ptr<i64, byte>) -> (i32, u16, u32, u32) {
                    %1 = hir.load %0;
                    %2 = arith.constant 32 : u32;
                    %3 = arith.shr %1, %2;
                    %4 = arith.trunc %3 <{ ty = #builtin.type<i32> }>;
                    %5 = hir.load %0;
                    %6 = arith.trunc %5 <{ ty = #builtin.type<u16> }>;
                    %7 = hir.load %0;
                    %8 = hir.bitcast %7 <{ ty = #builtin.type<u64> }>;
                    %9 = arith.constant 32 : u32;
                    %10 = arith.shr %8, %9;
                    %19, %20 = arith.split %10  : (u64) -> (u32, u32);
                    %14 = hir.load %0;
                    %15 = hir.bitcast %14 <{ ty = #builtin.type<u64> }>;
                    %16 = arith.constant 16 : u32;
                    %17 = arith.shr %15, %16;
                    %18 = arith.trunc %17 <{ ty = #builtin.type<u32> }>;
                    builtin.ret %4, %6, %19, %18 : (i32, u16, u32, u32);
                };"#]],
        );
    }

    /// `scf.if` with a 64-bit integer result: what the Wasm frontend's `if (result i64)` is by the
    /// time of legalization, which sees structured control flow only. Each arm yields what
    /// `then` and `otherwise` build in it.
    fn if_yielding_i64(
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        cond: ValueRef,
        then: impl FnOnce(&mut FunctionBuilder<'_, OpBuilder>) -> ValueRef,
        otherwise: impl FnOnce(&mut FunctionBuilder<'_, OpBuilder>) -> ValueRef,
    ) -> ValueRef {
        let span = SourceSpan::UNKNOWN;
        let current = builder.current_block();
        let conditional = builder.r#if(cond, &[Type::I64], span).unwrap();
        let then_region = conditional.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let value = then(builder);
        builder.r#yield([value], span).unwrap();
        let else_region = conditional.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let value = otherwise(builder);
        builder.r#yield([value], span).unwrap();
        builder.switch_to_block(current);
        conditional.borrow().results()[0] as ValueRef
    }

    /// A pair `or(zext(lo), shl(zext(hi), 32))` assembled in each arm of an `scf.if` and carried
    /// out of it is joined where it is assembled. This is the form in which two rebuilt `Word`s
    /// met at a branch in sub-project 3, beyond the reach of the store split.
    #[test]
    fn a_pair_assembled_in_each_branch_is_joined() {
        let mut test = Test::new(
            "a_pair_assembled_in_each_branch_is_joined",
            &[Type::I1, Type::I32, Type::I32, Type::U32, Type::U32],
            &[Type::I64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [cond, a, b, c, d] =
                [args[0], args[1], args[2], args[3], args[4]].map(|arg| arg as ValueRef);
            let pair = if_yielding_i64(
                &mut builder,
                cond,
                // As the Wasm frontend translates `i64.extend_i32_u`, `i64.shl` and `i64.or`
                |builder| {
                    let lo = wasm_extend_i32_u(builder, a);
                    let hi = wasm_extend_i32_u(builder, b);
                    let count = wasm_shift_count_32(builder);
                    let hi = builder.shl(hi, count, span).unwrap();
                    builder.bor(lo, hi, span).unwrap()
                },
                // The high half first
                |builder| {
                    let lo = builder.zext(c, Type::U64, span).unwrap();
                    let hi = builder.zext(d, Type::U64, span).unwrap();
                    let count = builder.u32(32, span);
                    let hi = builder.shl(hi, count, span).unwrap();
                    let value = builder.bor(hi, lo, span).unwrap();
                    builder.bitcast(value, Type::I64, span).unwrap()
                },
            );
            builder.ret([pair], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_pair_assembled_in_each_branch_is_joined(%0: i1, %1: i32, %2: i32, %3: u32, %4: u32) -> i64 {
                    %5 = scf.if %0 then {
                        %6 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                        %7 = arith.zext %6 <{ ty = #builtin.type<u64> }>;
                        %8 = hir.bitcast %7 <{ ty = #builtin.type<i64> }>;
                        %9 = hir.bitcast %2 <{ ty = #builtin.type<u32> }>;
                        %10 = arith.zext %9 <{ ty = #builtin.type<u64> }>;
                        %11 = hir.bitcast %10 <{ ty = #builtin.type<i64> }>;
                        %12 = arith.constant 32 : u32;
                        %13 = arith.constant 63 : u32;
                        %14 = arith.band %12, %13;
                        %15 = arith.shl %11, %14;
                        %16 = arith.bor %8, %15;
                        scf.yield %16 : (i64);
                    } else {
                        %17 = arith.zext %3 <{ ty = #builtin.type<u64> }>;
                        %18 = arith.zext %4 <{ ty = #builtin.type<u64> }>;
                        %19 = arith.constant 32 : u32;
                        %20 = arith.shl %18, %19;
                        %21 = arith.bor %20, %17;
                        %22 = hir.bitcast %21 <{ ty = #builtin.type<i64> }>;
                        scf.yield %22 : (i64);
                    } : (i1) -> (i64);
                    builtin.ret %5 : (i64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_pair_assembled_in_each_branch_is_joined(%0: i1, %1: i32, %2: i32, %3: u32, %4: u32) -> i64 {
                    %5 = scf.if %0 then {
                        %6 = hir.bitcast %1 <{ ty = #builtin.type<u32> }>;
                        %9 = hir.bitcast %2 <{ ty = #builtin.type<u32> }>;
                        %25 = arith.join %9, %6 <{ ty = #builtin.type<i64> }>;
                        scf.yield %25 : (i64);
                    } else {
                        %28 = arith.join %4, %3 <{ ty = #builtin.type<u64> }>;
                        %22 = hir.bitcast %28 <{ ty = #builtin.type<i64> }>;
                        scf.yield %22 : (i64);
                    } : (i1) -> (i64);
                    builtin.ret %5 : (i64);
                };"#]],
        );
    }

    /// An integer that comes out of an `scf.if` and whose high half is taken, as the Wasm
    /// frontend translates `i32.wrap_i64` of it and of its `i64.shr_u` by 32, has both halves
    /// taken from one split of it, placed before the first of them.
    #[test]
    fn a_value_whose_high_half_is_taken_after_a_branch_is_split() {
        let mut test = Test::new(
            "a_value_whose_high_half_is_taken_after_a_branch_is_split",
            &[Type::I1, Type::I64, Type::I64],
            &[Type::I32, Type::I32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [cond, a, b] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let value = if_yielding_i64(&mut builder, cond, |_| a, |_| b);
            let unsigned = builder.bitcast(value, Type::U64, span).unwrap();
            let count = wasm_shift_count_32(&mut builder);
            let shifted = builder.shr(unsigned, count, span).unwrap();
            let shifted = builder.bitcast(shifted, Type::I64, span).unwrap();
            let hi = builder.trunc(shifted, Type::I32, span).unwrap();
            let lo = builder.trunc(value, Type::I32, span).unwrap();
            builder.ret([lo, hi], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_value_whose_high_half_is_taken_after_a_branch_is_split(%0: i1, %1: i64, %2: i64) -> (i32, i32) {
                    %3 = scf.if %0 then {
                        scf.yield %1 : (i64);
                    } else {
                        scf.yield %2 : (i64);
                    } : (i1) -> (i64);
                    %4 = hir.bitcast %3 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.constant 32 : u32;
                    %6 = arith.constant 63 : u32;
                    %7 = arith.band %5, %6;
                    %8 = arith.shr %4, %7;
                    %9 = hir.bitcast %8 <{ ty = #builtin.type<i64> }>;
                    %10 = arith.trunc %9 <{ ty = #builtin.type<i32> }>;
                    %11 = arith.trunc %3 <{ ty = #builtin.type<i32> }>;
                    builtin.ret %11, %10 : (i32, i32);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_value_whose_high_half_is_taken_after_a_branch_is_split(%0: i1, %1: i64, %2: i64) -> (i32, i32) {
                    %3 = scf.if %0 then {
                        scf.yield %1 : (i64);
                    } else {
                        scf.yield %2 : (i64);
                    } : (i1) -> (i64);
                    %12, %13 = arith.split %3  : (i64) -> (i32, i32);
                    builtin.ret %13, %12 : (i32, i32);
                };"#]],
        );
    }

    /// Uses of the integer that take no half keep it, whatever defines it: here a load, which the
    /// load split therefore leaves alone. The halves come from one split, with a cast where a
    /// `trunc` has another 32-bit type than the first.
    #[test]
    fn a_value_also_used_whole_keeps_it_and_takes_its_halves_from_a_split() {
        let mut test = Test::new(
            "a_value_also_used_whole_keeps_it_and_takes_its_halves_from_a_split",
            &[pointer_to(Type::U64, AddressSpace::Element)],
            &[Type::I32, Type::U32, Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let addr = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let value = builder.load(addr, span).unwrap();
            let lo = builder.trunc(value, Type::I32, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let hi = builder.trunc(shifted, Type::U32, span).unwrap();
            builder.ret([lo, hi, value], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_value_also_used_whole_keeps_it_and_takes_its_halves_from_a_split(%0: ptr<u64, element>) -> (i32, u32, u64) {
                    %1 = hir.load %0;
                    %2 = arith.trunc %1 <{ ty = #builtin.type<i32> }>;
                    %3 = arith.constant 32 : u32;
                    %4 = arith.shr %1, %3;
                    %5 = arith.trunc %4 <{ ty = #builtin.type<u32> }>;
                    builtin.ret %2, %5, %1 : (i32, u32, u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_value_also_used_whole_keeps_it_and_takes_its_halves_from_a_split(%0: ptr<u64, element>) -> (i32, u32, u64) {
                    %1 = hir.load %0;
                    %6, %7 = arith.split %1  : (u64) -> (i32, i32);
                    %8 = hir.bitcast %6 <{ ty = #builtin.type<u32> }>;
                    builtin.ret %7, %8, %1 : (i32, u32, u64);
                };"#]],
        );
    }

    /// The split is decided per block: each block that takes the high half has a split of its
    /// own, placed before the first half taken there, which the low halves taken there come from
    /// too, and a block that takes only the low half is left alone. Here the function's block
    /// takes both halves, the `then` arm only the low half and the `else` arm the high half. The
    /// `then` arm is reached while the `else` arm still takes the high half, so a rule for the
    /// whole function would split it too.
    #[test]
    fn each_block_that_takes_the_high_half_has_its_own_split() {
        let mut test = Test::new(
            "each_block_that_takes_the_high_half_has_its_own_split",
            &[Type::I1, Type::U64],
            &[Type::U32, Type::U32, Type::U32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [cond, value] = [args[0], args[1]].map(|arg| arg as ValueRef);
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let hi = builder.trunc(shifted, Type::U32, span).unwrap();
            let lo = builder.trunc(value, Type::U32, span).unwrap();
            let entry = builder.current_block();
            let conditional = builder.r#if(cond, &[Type::U32], span).unwrap();
            let then_region = conditional.borrow().then_body().as_region_ref();
            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let lo_again = builder.trunc(value, Type::U32, span).unwrap();
            builder.r#yield([lo_again], span).unwrap();
            let else_region = conditional.borrow().else_body().as_region_ref();
            let else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(else_block);
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let hi_again = builder.trunc(shifted, Type::U32, span).unwrap();
            builder.r#yield([hi_again], span).unwrap();
            builder.switch_to_block(entry);
            let either = conditional.borrow().results()[0] as ValueRef;
            builder.ret([hi, lo, either], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @each_block_that_takes_the_high_half_has_its_own_split(%0: i1, %1: u64) -> (u32, u32, u32) {
                    %2 = arith.constant 32 : u32;
                    %3 = arith.shr %1, %2;
                    %4 = arith.trunc %3 <{ ty = #builtin.type<u32> }>;
                    %5 = arith.trunc %1 <{ ty = #builtin.type<u32> }>;
                    %6 = scf.if %0 then {
                        %7 = arith.trunc %1 <{ ty = #builtin.type<u32> }>;
                        scf.yield %7 : (u32);
                    } else {
                        %8 = arith.constant 32 : u32;
                        %9 = arith.shr %1, %8;
                        %10 = arith.trunc %9 <{ ty = #builtin.type<u32> }>;
                        scf.yield %10 : (u32);
                    } : (i1) -> (u32);
                    builtin.ret %4, %5, %6 : (u32, u32, u32);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @each_block_that_takes_the_high_half_has_its_own_split(%0: i1, %1: u64) -> (u32, u32, u32) {
                    %11, %12 = arith.split %1  : (u64) -> (u32, u32);
                    %6 = scf.if %0 then {
                        %7 = arith.trunc %1 <{ ty = #builtin.type<u32> }>;
                        scf.yield %7 : (u32);
                    } else {
                        %13, %14 = arith.split %1  : (u64) -> (u32, u32);
                        scf.yield %13 : (u32);
                    } : (i1) -> (u32);
                    builtin.ret %11, %12, %6 : (u32, u32, u32);
                };"#]],
        );
    }

    /// One split serves halves of different 32-bit types: a limb is cast to the type of each
    /// `trunc` it replaces that has the other type.
    #[test]
    fn a_limb_is_cast_to_the_type_of_the_half_it_replaces() {
        let mut test = Test::new(
            "a_limb_is_cast_to_the_type_of_the_half_it_replaces",
            &[Type::U64],
            &[Type::U32, Type::I32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let lo = builder.trunc(value, Type::U32, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let hi = builder.trunc(shifted, Type::I32, span).unwrap();
            builder.ret([lo, hi], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_limb_is_cast_to_the_type_of_the_half_it_replaces(%0: u64) -> (u32, i32) {
                    %1 = arith.trunc %0 <{ ty = #builtin.type<u32> }>;
                    %2 = arith.constant 32 : u32;
                    %3 = arith.shr %0, %2;
                    %4 = arith.trunc %3 <{ ty = #builtin.type<i32> }>;
                    builtin.ret %1, %4 : (u32, i32);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_limb_is_cast_to_the_type_of_the_half_it_replaces(%0: u64) -> (u32, i32) {
                    %5, %6 = arith.split %0  : (u64) -> (u32, u32);
                    %7 = hir.bitcast %5 <{ ty = #builtin.type<i32> }>;
                    builtin.ret %6, %7 : (u32, i32);
                };"#]],
        );
    }

    /// An `or` with a third term stays an `or`: here the low half is itself an `or` of two
    /// zero-extended values. Its high side, `shl(zext(hi), 32)`, is exactly `join(hi, 0)` whatever
    /// it is or'ed with, and becomes that.
    #[test]
    fn an_or_with_a_third_term_stays_an_or() {
        let mut test = Test::new(
            "an_or_with_a_third_term_stays_an_or",
            &[Type::U32, Type::U32, Type::U32],
            &[Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [lo, third, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let lo = builder.zext(lo, Type::U64, span).unwrap();
            let third = builder.zext(third, Type::U64, span).unwrap();
            let low = builder.bor(lo, third, span).unwrap();
            let hi = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shl(hi, count, span).unwrap();
            let value = builder.bor(low, shifted, span).unwrap();
            builder.ret([value], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @an_or_with_a_third_term_stays_an_or(%0: u32, %1: u32, %2: u32) -> u64 {
                    %3 = arith.zext %0 <{ ty = #builtin.type<u64> }>;
                    %4 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.bor %3, %4;
                    %6 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %7 = arith.constant 32 : u32;
                    %8 = arith.shl %6, %7;
                    %9 = arith.bor %5, %8;
                    builtin.ret %9 : (u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @an_or_with_a_third_term_stays_an_or(%0: u32, %1: u32, %2: u32) -> u64 {
                    %3 = arith.zext %0 <{ ty = #builtin.type<u64> }>;
                    %4 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.bor %3, %4;
                    %10 = arith.constant 0 : u32;
                    %11 = arith.join %2, %10 <{ ty = #builtin.type<u64> }>;
                    %9 = arith.bor %5, %11;
                    builtin.ret %9 : (u64);
                };"#]],
        );
    }

    /// A pair of which LLVM has folded one half to a constant is joined with that constant as a
    /// limb: `shl(zext(hi), 32)` alone, of a zero low half; an `or` of it with a constant below
    /// 2^32; and an `or` of `zext(lo)` with a constant whose low 32 bits are zero. So is a pair of
    /// one `u32` with itself, through one `zext`.
    #[test]
    fn a_pair_with_a_constant_half_is_joined() {
        let mut test = Test::new(
            "a_pair_with_a_constant_half_is_joined",
            &[Type::U32, Type::U32, Type::U32, Type::U32],
            &[Type::U64, Type::U64, Type::U64, Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [a, b, c, d] = [args[0], args[1], args[2], args[3]].map(|arg| arg as ValueRef);
            let a = builder.zext(a, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let zero_low = builder.shl(a, count, span).unwrap();
            let b = builder.zext(b, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let b = builder.shl(b, count, span).unwrap();
            let seven = builder.u64(7, span);
            let constant_low = builder.bor(b, seven, span).unwrap();
            let c = builder.zext(c, Type::U64, span).unwrap();
            let five = builder.u64(5 << 32, span);
            let constant_high = builder.bor(five, c, span).unwrap();
            let d = builder.zext(d, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shl(d, count, span).unwrap();
            let itself = builder.bor(d, shifted, span).unwrap();
            builder.ret([zero_low, constant_low, constant_high, itself], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_pair_with_a_constant_half_is_joined(%0: u32, %1: u32, %2: u32, %3: u32) -> (u64, u64, u64, u64) {
                    %4 = arith.zext %0 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.constant 32 : u32;
                    %6 = arith.shl %4, %5;
                    %7 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %8 = arith.constant 32 : u32;
                    %9 = arith.shl %7, %8;
                    %10 = arith.constant 7 : u64;
                    %11 = arith.bor %9, %10;
                    %12 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %13 = arith.constant 21474836480 : u64;
                    %14 = arith.bor %13, %12;
                    %15 = arith.zext %3 <{ ty = #builtin.type<u64> }>;
                    %16 = arith.constant 32 : u32;
                    %17 = arith.shl %15, %16;
                    %18 = arith.bor %15, %17;
                    builtin.ret %6, %11, %14, %18 : (u64, u64, u64, u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_pair_with_a_constant_half_is_joined(%0: u32, %1: u32, %2: u32, %3: u32) -> (u64, u64, u64, u64) {
                    %19 = arith.constant 0 : u32;
                    %20 = arith.join %0, %19 <{ ty = #builtin.type<u64> }>;
                    %23 = arith.constant 7 : u32;
                    %24 = arith.join %1, %23 <{ ty = #builtin.type<u64> }>;
                    %25 = arith.constant 5 : u32;
                    %26 = arith.join %25, %2 <{ ty = #builtin.type<u64> }>;
                    %29 = arith.join %3, %3 <{ ty = #builtin.type<u64> }>;
                    builtin.ret %20, %24, %26, %29 : (u64, u64, u64, u64);
                };"#]],
        );
    }

    /// A store of a pair with a constant half stores the constant as a 32-bit half.
    #[test]
    fn a_store_of_a_pair_with_a_constant_half_is_split() {
        let mut test = Test::new(
            "a_store_of_a_pair_with_a_constant_half_is_split",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, hi] = [args[0], args[1]].map(|arg| arg as ValueRef);
            let hi = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let hi = builder.shl(hi, count, span).unwrap();
            let seven = builder.u64(7, span);
            let value = builder.bor(seven, hi, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_pair_with_a_constant_half_is_split(%0: ptr<u64, element>, %1: u32) {
                    %2 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %3 = arith.constant 32 : u32;
                    %4 = arith.shl %2, %3;
                    %5 = arith.constant 7 : u64;
                    %6 = arith.bor %5, %4;
                    hir.store %0, %6 : (ptr<u64, element>, u64);
                    builtin.ret;
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_pair_with_a_constant_half_is_split(%0: ptr<u64, element>, %1: u32) {
                    %9 = arith.constant 7 : u32;
                    %11 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %11, %9 : (ptr<u32, element>, u32);
                    %12 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %13 = arith.constant 1 : u32;
                    %14 = arith.add %12, %13 <{ overflow = #builtin.overflow<checked> }>;
                    %15 = hir.int_to_ptr %14 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %15, %1 : (ptr<u32, element>, u32);
                    builtin.ret;
                };"#]],
        );
    }

    /// A constant is a half only if it fits that half exactly: an `or` of one with bits in both
    /// halves beside a `zext`, of a shift by anything but 32, or of one of 2^32 or more beside the
    /// shifted half stays an `or`. (The last one's `shl(zext(c), 32)` is still a join on its own.)
    #[test]
    fn a_constant_that_is_not_a_whole_half_keeps_its_or() {
        let mut test = Test::new(
            "a_constant_that_is_not_a_whole_half_keeps_its_or",
            &[Type::U32, Type::U32, Type::U32],
            &[Type::U64, Type::U64, Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [a, b, c] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let a = builder.zext(a, Type::U64, span).unwrap();
            let both = builder.u64((5 << 32) | 7, span);
            let both_halves = builder.bor(a, both, span).unwrap();
            let b = builder.zext(b, Type::U64, span).unwrap();
            let count = builder.u32(16, span);
            let b = builder.shl(b, count, span).unwrap();
            let seven = builder.u64(7, span);
            let by_16 = builder.bor(b, seven, span).unwrap();
            let c = builder.zext(c, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let c = builder.shl(c, count, span).unwrap();
            let high = builder.u64(1 << 32, span);
            let too_wide = builder.bor(c, high, span).unwrap();
            builder.ret([both_halves, by_16, too_wide], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_constant_that_is_not_a_whole_half_keeps_its_or(%0: u32, %1: u32, %2: u32) -> (u64, u64, u64) {
                    %3 = arith.zext %0 <{ ty = #builtin.type<u64> }>;
                    %4 = arith.constant 21474836487 : u64;
                    %5 = arith.bor %3, %4;
                    %6 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %7 = arith.constant 16 : u32;
                    %8 = arith.shl %6, %7;
                    %9 = arith.constant 7 : u64;
                    %10 = arith.bor %8, %9;
                    %11 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %12 = arith.constant 32 : u32;
                    %13 = arith.shl %11, %12;
                    %14 = arith.constant 4294967296 : u64;
                    %15 = arith.bor %13, %14;
                    builtin.ret %5, %10, %15 : (u64, u64, u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_constant_that_is_not_a_whole_half_keeps_its_or(%0: u32, %1: u32, %2: u32) -> (u64, u64, u64) {
                    %3 = arith.zext %0 <{ ty = #builtin.type<u64> }>;
                    %4 = arith.constant 21474836487 : u64;
                    %5 = arith.bor %3, %4;
                    %6 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %7 = arith.constant 16 : u32;
                    %8 = arith.shl %6, %7;
                    %9 = arith.constant 7 : u64;
                    %10 = arith.bor %8, %9;
                    %16 = arith.constant 0 : u32;
                    %17 = arith.join %2, %16 <{ ty = #builtin.type<u64> }>;
                    %14 = arith.constant 4294967296 : u64;
                    %15 = arith.bor %17, %14;
                    builtin.ret %5, %10, %15 : (u64, u64, u64);
                };"#]],
        );
    }

    /// The split takes exactly the halves. Left alone: an integer of which only the low half is
    /// taken, whose `trunc` already drops the high limb with no `u32` instruction; an arithmetic
    /// `shr`, which is what a `shr` of an `i64` is in codegen; a shift by 16; and a `trunc` to
    /// fewer than 32 bits of the logical `shr` by 32.
    #[test]
    fn a_value_whose_halves_are_not_taken_exactly_is_left_alone() {
        let mut test = Test::new(
            "a_value_whose_halves_are_not_taken_exactly_is_left_alone",
            &[Type::U64, Type::I64, Type::U64, Type::U64],
            &[Type::U32, Type::I32, Type::U32, Type::U16],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [low_only, signed, by_16, narrow] =
                [args[0], args[1], args[2], args[3]].map(|arg| arg as ValueRef);
            let low_only = builder.trunc(low_only, Type::U32, span).unwrap();
            let count = builder.u32(32, span);
            let signed = builder.shr(signed, count, span).unwrap();
            let signed = builder.trunc(signed, Type::I32, span).unwrap();
            let count = builder.u32(16, span);
            let by_16 = builder.shr(by_16, count, span).unwrap();
            let by_16 = builder.trunc(by_16, Type::U32, span).unwrap();
            let count = builder.u32(32, span);
            let narrow = builder.shr(narrow, count, span).unwrap();
            let narrow = builder.trunc(narrow, Type::U16, span).unwrap();
            builder.ret([low_only, signed, by_16, narrow], span).unwrap();
        }

        assert_left_alone(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_value_whose_halves_are_not_taken_exactly_is_left_alone(%0: u64, %1: i64, %2: u64, %3: u64) -> (u32, i32, u32, u16) {
                    %4 = arith.trunc %0 <{ ty = #builtin.type<u32> }>;
                    %5 = arith.constant 32 : u32;
                    %6 = arith.shr %1, %5;
                    %7 = arith.trunc %6 <{ ty = #builtin.type<i32> }>;
                    %8 = arith.constant 16 : u32;
                    %9 = arith.shr %2, %8;
                    %10 = arith.trunc %9 <{ ty = #builtin.type<u32> }>;
                    %11 = arith.constant 32 : u32;
                    %12 = arith.shr %3, %11;
                    %13 = arith.trunc %12 <{ ty = #builtin.type<u16> }>;
                    builtin.ret %4, %7, %10, %13 : (u32, i32, u32, u16);
                };"#]],
        );
    }

    /// Values a rewrite keeps live longer do not hold it back: the legalization runs before spill
    /// placement, which spills what does not fit. Here the `zext`s of one pair are also used
    /// whole, and the `shr` of another integer's high half is too: the first pair is still a join,
    /// with its store split, and the high half still comes from a split, the `zext`s and the `shr`
    /// staying for their other uses.
    #[test]
    fn halves_whose_way_is_also_used_elsewhere_are_still_rewritten() {
        let mut test = Test::new(
            "halves_whose_way_is_also_used_elsewhere_are_still_rewritten",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32, Type::U64],
            &[Type::U64, Type::U64, Type::U32, Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi, value] =
                [args[0], args[1], args[2], args[3]].map(|arg| arg as ValueRef);
            let lo_ext = builder.zext(lo, Type::U64, span).unwrap();
            let hi_ext = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shl(hi_ext, count, span).unwrap();
            let pair = builder.bor(lo_ext, shifted, span).unwrap();
            builder.store(addr, pair, span).unwrap();
            let count = builder.u32(32, span);
            let high = builder.shr(value, count, span).unwrap();
            let high_half = builder.trunc(high, Type::U32, span).unwrap();
            builder.ret([lo_ext, hi_ext, high_half, high], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @halves_whose_way_is_also_used_elsewhere_are_still_rewritten(%0: ptr<u64, element>, %1: u32, %2: u32, %3: u64) -> (u64, u64, u32, u64) {
                    %4 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %6 = arith.constant 32 : u32;
                    %7 = arith.shl %5, %6;
                    %8 = arith.bor %4, %7;
                    hir.store %0, %8 : (ptr<u64, element>, u64);
                    %9 = arith.constant 32 : u32;
                    %10 = arith.shr %3, %9;
                    %11 = arith.trunc %10 <{ ty = #builtin.type<u32> }>;
                    builtin.ret %4, %5, %11, %10 : (u64, u64, u32, u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @halves_whose_way_is_also_used_elsewhere_are_still_rewritten(%0: ptr<u64, element>, %1: u32, %2: u32, %3: u64) -> (u64, u64, u32, u64) {
                    %4 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %15 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %15, %1 : (ptr<u32, element>, u32);
                    %16 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %17 = arith.constant 1 : u32;
                    %18 = arith.add %16, %17 <{ overflow = #builtin.overflow<checked> }>;
                    %19 = hir.int_to_ptr %18 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %19, %2 : (ptr<u32, element>, u32);
                    %9 = arith.constant 32 : u32;
                    %10 = arith.shr %3, %9;
                    %20, %21 = arith.split %3  : (u64) -> (u32, u32);
                    builtin.ret %4, %5, %20, %10 : (u64, u64, u32, u64);
                };"#]],
        );
    }

    /// A store of a joined pair of 32-bit integers is split, as a store of the `or` it was made of
    /// is. A join of felts, which the frontends build for the canonical ABI, is not split: its
    /// 64-bit store copies the two elements as they are.
    #[test]
    fn a_store_of_a_joined_pair_is_split() {
        let mut test = Test::new(
            "a_store_of_a_joined_pair_is_split",
            &[
                pointer_to(Type::U64, AddressSpace::Element),
                Type::U32,
                Type::U32,
                Type::Felt,
                Type::Felt,
            ],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi, felt_lo, felt_hi] =
                [args[0], args[1], args[2], args[3], args[4]].map(|arg| arg as ValueRef);
            let value = builder.join([hi, lo], Type::U64, span).unwrap();
            builder.store(addr, value, span).unwrap();
            let value = builder.join([felt_hi, felt_lo], Type::U64, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_joined_pair_is_split(%0: ptr<u64, element>, %1: u32, %2: u32, %3: felt, %4: felt) {
                    %5 = arith.join %2, %1 <{ ty = #builtin.type<u64> }>;
                    hir.store %0, %5 : (ptr<u64, element>, u64);
                    %6 = arith.join %4, %3 <{ ty = #builtin.type<u64> }>;
                    hir.store %0, %6 : (ptr<u64, element>, u64);
                    builtin.ret;
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_joined_pair_is_split(%0: ptr<u64, element>, %1: u32, %2: u32, %3: felt, %4: felt) {
                    %7 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %7, %1 : (ptr<u32, element>, u32);
                    %8 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %9 = arith.constant 1 : u32;
                    %10 = arith.add %8, %9 <{ overflow = #builtin.overflow<checked> }>;
                    %11 = hir.int_to_ptr %10 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %11, %2 : (ptr<u32, element>, u32);
                    %6 = arith.join %4, %3 <{ ty = #builtin.type<u64> }>;
                    hir.store %0, %6 : (ptr<u64, element>, u64);
                    builtin.ret;
                };"#]],
        );
    }

    /// The store split wins over the join, whichever of the store and the `or` the conversion
    /// reaches first. Reached after the `or`, as it always is in structured control flow, a store
    /// is split as a store of the join the `or` became, and the join goes. Reached before it, it is
    /// split as a store of the `or`. Here the first pair is assembled and stored in one block, and
    /// the second pair's store is in a block that comes first in its region, although the block
    /// that assembles the pair dominates it.
    #[test]
    fn the_store_split_wins_whichever_op_is_reached_first() {
        let mut test = Test::new(
            "the_store_split_wins_whichever_op_is_reached_first",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32, Type::I1],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi, cond] =
                [args[0], args[1], args[2], args[3]].map(|arg| arg as ValueRef);
            let reached_first = builder.create_block();
            let assembles = builder.create_block();
            builder.cond_br(cond, assembles, [], assembles, [], span).unwrap();
            builder.switch_to_block(assembles);
            let assemble = |builder: &mut FunctionBuilder<'_, OpBuilder>| {
                let lo = builder.zext(lo, Type::U64, span).unwrap();
                let hi = builder.zext(hi, Type::U64, span).unwrap();
                let count = builder.u32(32, span);
                let hi = builder.shl(hi, count, span).unwrap();
                builder.bor(lo, hi, span).unwrap()
            };
            let first = assemble(&mut builder);
            builder.store(addr, first, span).unwrap();
            let second = assemble(&mut builder);
            builder.cond_br(cond, reached_first, [], reached_first, [], span).unwrap();
            builder.switch_to_block(reached_first);
            builder.store(addr, second, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @the_store_split_wins_whichever_op_is_reached_first(%0: ptr<u64, element>, %1: u32, %2: u32, %3: i1) {
                    cf.cond_br %3 ^block2, ^block2 : (i1);
                ^block1:
                    hir.store %0, %13 : (ptr<u64, element>, u64);
                    builtin.ret;
                ^block2:
                    %4 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %6 = arith.constant 32 : u32;
                    %7 = arith.shl %5, %6;
                    %8 = arith.bor %4, %7;
                    hir.store %0, %8 : (ptr<u64, element>, u64);
                    %9 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %10 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %11 = arith.constant 32 : u32;
                    %12 = arith.shl %10, %11;
                    %13 = arith.bor %9, %12;
                    cf.cond_br %3 ^block1, ^block1 : (i1);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @the_store_split_wins_whichever_op_is_reached_first(%0: ptr<u64, element>, %1: u32, %2: u32, %3: i1) {
                    cf.cond_br %3 ^block2, ^block2 : (i1);
                ^block1:
                    %14 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %14, %1 : (ptr<u32, element>, u32);
                    %15 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %16 = arith.constant 1 : u32;
                    %17 = arith.add %15, %16 <{ overflow = #builtin.overflow<checked> }>;
                    %18 = hir.int_to_ptr %17 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %18, %2 : (ptr<u32, element>, u32);
                    builtin.ret;
                ^block2:
                    %22 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %22, %1 : (ptr<u32, element>, u32);
                    %23 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %24 = arith.constant 1 : u32;
                    %25 = arith.add %23, %24 <{ overflow = #builtin.overflow<checked> }>;
                    %26 = hir.int_to_ptr %25 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %26, %2 : (ptr<u32, element>, u32);
                    cf.cond_br %3 ^block1, ^block1 : (i1);
                };"#]],
        );
    }

    /// A pair stored and also used elsewhere is joined for that use, and its store is split all
    /// the same: the halves live on to the store beside the join, which spill placement, after
    /// this pass, allows for.
    #[test]
    fn a_store_of_a_pair_also_used_elsewhere_is_split() {
        let mut test = Test::new(
            "a_store_of_a_pair_also_used_elsewhere_is_split",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32],
            &[Type::U64],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            let lo = builder.zext(lo, Type::U64, span).unwrap();
            let hi = builder.zext(hi, Type::U64, span).unwrap();
            let count = builder.u32(32, span);
            let hi = builder.shl(hi, count, span).unwrap();
            let value = builder.bor(lo, hi, span).unwrap();
            builder.store(addr, value, span).unwrap();
            builder.ret([value], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_pair_also_used_elsewhere_is_split(%0: ptr<u64, element>, %1: u32, %2: u32) -> u64 {
                    %3 = arith.zext %1 <{ ty = #builtin.type<u64> }>;
                    %4 = arith.zext %2 <{ ty = #builtin.type<u64> }>;
                    %5 = arith.constant 32 : u32;
                    %6 = arith.shl %4, %5;
                    %7 = arith.bor %3, %6;
                    hir.store %0, %7 : (ptr<u64, element>, u64);
                    builtin.ret %7 : (u64);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @a_store_of_a_pair_also_used_elsewhere_is_split(%0: ptr<u64, element>, %1: u32, %2: u32) -> u64 {
                    %10 = arith.join %2, %1 <{ ty = #builtin.type<u64> }>;
                    %11 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %11, %1 : (ptr<u32, element>, u32);
                    %12 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %13 = arith.constant 1 : u32;
                    %14 = arith.add %12, %13 <{ overflow = #builtin.overflow<checked> }>;
                    %15 = hir.int_to_ptr %14 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %15, %2 : (ptr<u32, element>, u32);
                    builtin.ret %10 : (u64);
                };"#]],
        );
    }

    /// The load split wins over the split of its value, whichever of the load and its `trunc`s
    /// the conversion reaches first: two 32-bit loads beat a 64-bit load and a split. Here the
    /// block taking the halves comes first in its region, although the block that loads the pair
    /// dominates it.
    #[test]
    fn the_load_split_wins_whichever_op_is_reached_first() {
        let mut test = Test::new(
            "the_load_split_wins_whichever_op_is_reached_first",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::I1],
            &[Type::U32, Type::U32],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, cond] = [args[0], args[1]].map(|arg| arg as ValueRef);
            let reached_first = builder.create_block();
            let loads = builder.create_block();
            builder.cond_br(cond, loads, [], loads, [], span).unwrap();
            builder.switch_to_block(loads);
            let value = builder.load(addr, span).unwrap();
            builder.cond_br(cond, reached_first, [], reached_first, [], span).unwrap();
            builder.switch_to_block(reached_first);
            let lo = builder.trunc(value, Type::U32, span).unwrap();
            let count = builder.u32(32, span);
            let shifted = builder.shr(value, count, span).unwrap();
            let hi = builder.trunc(shifted, Type::U32, span).unwrap();
            builder.ret([lo, hi], span).unwrap();
        }

        assert_legalizes(
            &test,
            expect![[r#"
                builtin.function public extern("C") @the_load_split_wins_whichever_op_is_reached_first(%0: ptr<u64, element>, %1: i1) -> (u32, u32) {
                    cf.cond_br %1 ^block2, ^block2 : (i1);
                ^block1:
                    %3 = arith.trunc %2 <{ ty = #builtin.type<u32> }>;
                    %4 = arith.constant 32 : u32;
                    %5 = arith.shr %2, %4;
                    %6 = arith.trunc %5 <{ ty = #builtin.type<u32> }>;
                    builtin.ret %3, %6 : (u32, u32);
                ^block2:
                    %2 = hir.load %0;
                    cf.cond_br %1 ^block1, ^block1 : (i1);
                };"#]],
            expect![[r#"
                builtin.function public extern("C") @the_load_split_wins_whichever_op_is_reached_first(%0: ptr<u64, element>, %1: i1) -> (u32, u32) {
                    cf.cond_br %1 ^block2, ^block2 : (i1);
                ^block1:
                    builtin.ret %8, %13 : (u32, u32);
                ^block2:
                    %7 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    %8 = hir.load %7;
                    %9 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %10 = arith.constant 1 : u32;
                    %11 = arith.add %9, %10 <{ overflow = #builtin.overflow<checked> }>;
                    %12 = hir.int_to_ptr %11 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    %13 = hir.load %12;
                    cf.cond_br %1 ^block1, ^block1 : (i1);
                };"#]],
        );
    }

    /// The conversion driver's default cap of 1024 rewrites, a hard error, does not bound the
    /// rewrites: a component is not to fail to compile for the number of its sites.
    #[test]
    fn more_split_sites_than_the_default_rewrite_cap_legalize() {
        const SITES: usize = 1025;
        let mut test = Test::new(
            "more_split_sites_than_the_default_rewrite_cap_legalize",
            &[pointer_to(Type::U64, AddressSpace::Element), Type::U32, Type::U32],
            &[],
        );
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let args = builder.entry_block().borrow().arguments().to_vec();
            let [addr, lo, hi] = [args[0], args[1], args[2]].map(|arg| arg as ValueRef);
            // Each pair, assembled and stored, is a site of its own
            for _ in 0..SITES {
                let lo = builder.zext(lo, Type::U64, span).unwrap();
                let hi = builder.zext(hi, Type::U64, span).unwrap();
                let count = builder.u32(32, span);
                let hi = builder.shl(hi, count, span).unwrap();
                let value = builder.bor(lo, hi, span).unwrap();
                builder.store(addr, value, span).unwrap();
            }
            builder.ret(None, span).unwrap();
        }

        test.apply_pass::<LegalizeForMasm>(true).unwrap();
        let hir = test.function().borrow().as_operation().to_string();
        assert_eq!(hir.matches("hir.store").count(), 2 * SITES);
        assert_eq!(hir.matches(": (ptr<u32, element>, u32)").count(), 2 * SITES);
        assert!(!hir.contains("arith.bor") && !hir.contains("arith.join"), "{hir}");
    }
}
