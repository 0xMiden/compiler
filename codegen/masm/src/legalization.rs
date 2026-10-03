use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    Context, EntityMut, Immediate, Op, Operation, OperationName, OperationRef, Overflow,
    PointerType, Report, SmallVec, SourceSpan, Symbol, SymbolRef, Type, UnsafeIntrusiveEntityRef,
    Usable, Value, ValueRef, Visibility, WalkResult,
    conversion::{
        ConversionConfig, ConversionPattern, ConversionPatternRewriter, ConversionPatternSet,
        ConversionTarget, ConvertedOperands, DynamicLegalityResult, apply_full_conversion,
    },
    dialects::{builtin, debuginfo},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind},
    traits::Transparent,
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
/// lower, and splits the 64-bit memory ops it would lower unsafely when their halves are felts: a
/// 64-bit store of a value assembled from two 32-bit halves becomes two 32-bit stores, and a
/// 64-bit load used only for its halves two 32-bit loads.
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
        patterns.push(SplitWideLoad::new(context));
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
/// as a final operation, and so are the 64-bit stores and loads of two 32-bit halves that
/// [`LegalizeForMasm`] splits.
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
        .add_dynamically_legal_op::<hir::Store, _>(|op| {
            if StoreHalves::of(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit store of two 32-bit halves must be split into two 32-bit stores",
                ))
            } else {
                masm_lowerable_op(op)
            }
        })
        .add_dynamically_legal_op::<hir::Load, _>(|op| {
            if LoadHalves::of(op).is_some() {
                DynamicLegalityResult::illegal_with_reason(Report::msg(
                    "a 64-bit load used only as 32-bit halves must be split into two 32-bit loads",
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

/// Splits a 64-bit integer store whose value is two 32-bit halves, `or(zext(lo), shl(zext(hi),
/// 32))` with the `or`'s operands in either order, into a 32-bit store of each half.
///
/// LLVM's store merging produces this shape for two adjacent `i32` or `f32` stores on wasm32. On
/// Miden the merge never pays: a 64-bit store is two element stores anyway, and the `zext`, `shl`
/// and `or` that build its value are extra work. And when the halves are felts, which Rust
/// carries in `f32` and reinterprets as `i32` with a bitcast that emits nothing, the merge is
/// wrong: the 64-bit `shl` and `or` work on 32-bit limbs with `u32` instructions, which trap on a
/// felt outside the `u32` range. Split, each half is stored as the element it is.
///
/// Only that exact shape is split: a 64-bit integer value, `u32` halves, `zext` rather than
/// `sext`, a shift by the constant 32. The sign-only `hir.bitcast`s with which the Wasm frontend
/// spells `i64.extend_i32_u`, and the `band` with which it masks every shift count, are looked
/// through. The high half goes 4 bytes on, or one element on in element space, where
/// `intrinsics::mem::store_dw` puts the high half of a 64-bit store.
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
        let Some(halves) = StoreHalves::of(&op.borrow()) else {
            return Ok(false);
        };
        let (span, addr, value) = {
            let op = op.borrow();
            let store = op.downcast_ref::<hir::Store>().expect("the halves are a store's");
            (op.span(), store.addr().as_value_ref(), store.value().as_value_ref())
        };

        let lo_addr = half_address(rewriter, addr, Half::Lo, Type::U32, span)?;
        rewriter.create_op::<hir::Store, _>(span, (lo_addr, halves.lo))?;
        let hi_addr = half_address(rewriter, addr, Half::Hi, Type::U32, span)?;
        rewriter.create_op::<hir::Store, _>(span, (hi_addr, halves.hi))?;
        rewriter.erase_op(op)?;
        erase_dead_defs(rewriter, [value])?;
        Ok(true)
    }
}

/// The two `u32` halves of the value of a 64-bit store that [`SplitWideStore`] splits.
struct StoreHalves {
    lo: ValueRef,
    hi: ValueRef,
}

impl StoreHalves {
    fn of(op: &Operation) -> Option<Self> {
        let store = op.downcast_ref::<hir::Store>()?;
        let value = store.value().as_value_ref();
        if !is_64bit_integer(value.borrow().ty()) {
            return None;
        }
        let or = defined_by::<arith::Bor>(through_sign_casts(value))?;
        let (lhs, rhs) = {
            let or = or.borrow();
            (or.lhs().as_value_ref(), or.rhs().as_value_ref())
        };
        let halves = |lo, shifted| {
            Some(Self {
                lo: zero_extended_u32(lo)?,
                hi: zero_extended_u32(shifted_left_by_32(shifted)?)?,
            })
        };
        halves(lhs, rhs).or_else(|| halves(rhs, lhs))
    }
}

/// Splits a 64-bit integer load whose every use takes a 32-bit half of it, `trunc` for the low
/// half and `trunc(shr(_, 32))` for the high half, into a 32-bit load of each half used.
///
/// The mirror of [`SplitWideStore`], for the same reasons: two 32-bit loads are never dearer on
/// Miden than a 64-bit load and its `shr`, and that `shr` traps on the limbs of a felt pair. The
/// `shr` must be logical (of a `u64`), and the same sign-only casts and masked shift count are
/// looked through. Uses by debug info do not count, and go with the 64-bit value.
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
        let Some(halves) = LoadHalves::of(&op.borrow()) else {
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

/// The uses of a 64-bit load that [`SplitWideLoad`] feeds from 32-bit loads instead.
#[derive(Default)]
struct LoadHalves {
    /// The `trunc`s of the low half
    lo: SmallVec<[OperationRef; 1]>,
    /// The `trunc`s of the high half, each of a `shr` by 32
    hi: SmallVec<[OperationRef; 1]>,
}

impl LoadHalves {
    fn of(op: &Operation) -> Option<Self> {
        let load = op.downcast_ref::<hir::Load>()?;
        let value = load.result().as_value_ref();
        if !is_64bit_integer(value.borrow().ty()) {
            return None;
        }
        let mut halves = Self::default();
        halves.sort_uses(value, Half::Lo)?;
        (!halves.lo.is_empty() || !halves.hi.is_empty()).then_some(halves)
    }

    /// Sort the uses of `value`, which holds `half` in its low 32 bits, into `trunc`s of either
    /// half, failing on any other use.
    fn sort_uses(&mut self, value: ValueRef, half: Half) -> Option<()> {
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
                self.sort_uses(cast.result().as_value_ref(), half)?;
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
                self.sort_uses(shr.result().as_value_ref(), Half::Hi)?;
            } else {
                return None;
            }
        }
        Some(())
    }
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
        let (operands, debug_users) = {
            let op = def.borrow();
            let dead = op.results().iter().all(|result| !result.borrow().has_real_uses())
                && op.would_be_trivially_dead();
            if !dead {
                continue;
            }
            let operands = op
                .operands()
                .iter()
                .map(|operand| operand.borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 2]>>();
            let debug_users = op
                .results()
                .iter()
                .flat_map(|result| {
                    result.borrow().iter_uses().map(|user| user.owner).collect::<SmallVec<[_; 2]>>()
                })
                .collect::<SmallVec<[OperationRef; 2]>>();
            (operands, debug_users)
        };
        // Erased one by one first: `erase_op` would erase them itself, but while it walks the use
        // list that erasing them unlinks them from, which panics
        for user in debug_users {
            rewriter.erase_op(user)?;
        }
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

fn result_type(op: OperationRef) -> Type {
    op.borrow().results()[0].borrow().ty().clone()
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format};

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_expect_test::{Expect, expect};
    use midenc_hir::{
        AddressSpace, Ident, OpBuilder, PointerType, SourceSpan, Type, ValueRef, Visibility,
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
    /// shape LLVM's store merging produces for two adjacent `i32`/`f32` stores on wasm32, as the
    /// Wasm frontend translates it.
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
                    %14 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, byte>> }>;
                    hir.store %14, %3 : (ptr<u32, byte>, u32);
                    %15 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %16 = arith.constant 4 : u32;
                    %17 = arith.add %15, %16 <{ overflow = #builtin.overflow<checked> }>;
                    %18 = hir.int_to_ptr %17 <{ ty = #builtin.type<ptr<u32, byte>> }>;
                    hir.store %18, %6 : (ptr<u32, byte>, u32);
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
                    %8 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %8, %1 : (ptr<u32, element>, u32);
                    %9 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                    %10 = arith.constant 1 : u32;
                    %11 = arith.add %9, %10 <{ overflow = #builtin.overflow<checked> }>;
                    %12 = hir.int_to_ptr %11 <{ ty = #builtin.type<ptr<u32, element>> }>;
                    hir.store %12, %2 : (ptr<u32, element>, u32);
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
                %10 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                hir.store %10, %1 : (ptr<u32, element>, u32);
                %11 = hir.ptr_to_int %0 <{ ty = #builtin.type<u32> }>;
                %12 = arith.constant 1 : u32;
                %13 = arith.add %11, %12 <{ overflow = #builtin.overflow<checked> }>;
                %14 = hir.int_to_ptr %13 <{ ty = #builtin.type<ptr<u32, element>> }>;
                hir.store %14, %2 : (ptr<u32, element>, u32);
                %15 = hir.bitcast %0 <{ ty = #builtin.type<ptr<u32, element>> }>;
                %16 = hir.load %15;
                builtin.ret %16 : (u32);
            };"#]],
        );
    }

    /// Only the exact shape is split: halves of 32 bits, zero-extended, shifted apart by 32.
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
}
