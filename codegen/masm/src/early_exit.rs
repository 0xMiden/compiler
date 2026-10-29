//! Normalize structured early exits to single-exit control flow before MASM legalization.
//!
//! MASM structured blocks have no direct nonlocal branch. This pass's exit protocol works as
//! follows: each addressed loop and the function's return path have separate channels, each
//! consisting of a flag local and one local per carried value. Exits store their values and set
//! their flag; guards skip the remaining computation while an exit is pending. Crossed loops
//! stop at their next header, and the receiving loop supplies the break values as its results.
//!
//! Continue channels carry the next iteration's arguments and are cleared before the back edge.
//! A continue directly terminating its target's body folds to a yield without a channel. Loop
//! flags are initialized before each entry into the loop, so repeated entries remain independent.
//!
//! After normalization, function returns occur only at function level and no `scf.break` or
//! `scf.continue` remains. Physical locals and stack placement belong to the unified allocator.
//!
//! The existing lowering supports single-block structured regions and rejects exits originating
//! in loop condition regions. The latter is a lowering limitation: some such exits are valid SCF
//! and are supported by the interpreter. CFG restructuring must precede this pass.

use alloc::{format, rc::Rc, vec::Vec};

use midenc_dialect_arith as arith;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf as scf;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    BlockRef, Builder, BuilderExt, Context, EntityMut, HasBreakingControlFlowOpInterface,
    Immediate, Op, OpBuilder, Operation, OperationName, OperationRef, RegionExitKind,
    RegionExitTerminatorOpInterface, Report, SourceSpan, Spanned, Symbol, Type, Value, ValueRef,
    dialects::builtin::{self, BuiltinOpBuilder, attributes::LocalVariable},
    pass::{Pass, PassExecutionState, PostPassStatus},
};

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<LowerEarlyExits>(
    LowerEarlyExits::ARGUMENT,
    "lower early exits from structured control flow to single-exit form"
));

/// See the module documentation.
#[derive(Default)]
pub struct LowerEarlyExits;

impl LowerEarlyExits {
    /// Command-line/pass-pipeline argument for this pass.
    pub const ARGUMENT: &'static str = "lower-early-exits";
}

impl Pass for LowerEarlyExits {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "lower-early-exits"
    }

    fn argument(&self) -> &'static str {
        Self::ARGUMENT
    }

    fn description(&self) -> &'static str {
        "Lowers early exits from structured control flow to single-exit form"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let context = op.context_rc();
        let root = op.as_operation_ref();
        drop(op);

        let mut functions = Vec::new();
        root.borrow().prewalk_all(|nested: &Operation| {
            if let Some(function) = nested.downcast_ref::<builtin::Function>() {
                functions.push(function.as_function_ref());
            }
        });

        let mut changed = false;
        for function in functions {
            changed |= lower_function_early_exits(function, Rc::clone(&context))?;
        }

        let changed = PostPassStatus::from(changed);
        state.set_post_pass_status(changed);
        if !changed.ir_changed() {
            state.preserved_analyses_mut().preserve_all();
        }

        Ok(())
    }
}

/// Whether `op` is a return-like operation.
fn is_ret(op: &Operation) -> bool {
    op.downcast_ref::<builtin::Ret>().is_some() || op.downcast_ref::<builtin::RetImm>().is_some()
}

/// Whether `op` leaves the region it terminates, for a region further out.
///
/// Returns leave the function; structured exits leave for their resolved ancestor target.
/// [Channels::id_of] identifies the channel carrying each exit.
fn is_structured_exit(op: &Operation) -> bool {
    is_ret(op) || op.implements::<dyn RegionExitTerminatorOpInterface>()
}

/// Whether any region of `op` (transitively) contains an operation that leaves it.
fn op_regions_contain_any_exit(op: &Operation) -> bool {
    op_regions_contain(op, is_structured_exit)
}

/// Whether any region of `op` (transitively) contains an exit on `channel`.
///
/// Guards use this predicate to identify the channels that can be pending after `op`.
fn op_regions_contain_exit(channels: &Channels, op: &Operation, channel: ChannelId) -> bool {
    op_regions_contain(op, |nested| channels.id_of(nested) == Some(channel))
}

fn op_regions_contain(op: &Operation, predicate: impl Fn(&Operation) -> bool) -> bool {
    let mut found = false;
    let root = op.as_operation_ref();
    op.prewalk_all(|nested: &Operation| {
        if !OperationRef::ptr_eq(&nested.as_operation_ref(), &root) && predicate(nested) {
            found = true;
        }
    });
    found
}

/// The first operation in `block` whose regions contain a return-like op, if any.
fn first_op_with_nested_ret(block: BlockRef) -> Option<OperationRef> {
    let block = block.borrow();
    block
        .body()
        .iter()
        .find(|op| op_regions_contain(op, is_ret))
        .map(|op| op.as_operation_ref())
}

/// What the enclosing context expects a block to produce on its normal exit path.
#[derive(Clone)]
enum ExitKind {
    /// The function body itself: the terminator is (and stays) `builtin.ret`.
    Function,
    /// The ELSE chain of the function-level guards: the block's values ARE the function
    /// results (the guard chain forwards them to the final `builtin.ret`), so a return-like
    /// terminator simply converts to `scf.yield` of its values, and a pending return that set
    /// the flag deeper inside surfaces by loading the value locals in the guard's then-branch.
    YieldReturn,
    /// A REGION of a may-exit operation: the block's values flow to that operation's results,
    /// so an exiting terminator converts to the exit-channel protocol (store the carried
    /// values, set the flag, yield type-correct fillers), and enclosing guards re-check the
    /// flags.
    YieldProtocol { tys: Vec<Type> },
    /// The `after` region's own chain of loop `target`: the block's values are the loop's NEXT
    /// ITERATION arguments, which is what makes this the continue channel's analogue of
    /// [ExitKind::YieldReturn] — `channel` is that channel, when the loop has one.
    LoopBody {
        target: OperationRef,
        channel: Option<usize>,
        tys: Vec<Type>,
    },
}

impl ExitKind {
    /// The types a block on this chain terminates with.
    fn yield_types(&self, channels: &Channels) -> Vec<Type> {
        match self {
            ExitKind::Function | ExitKind::YieldReturn => channels.ret_channel().tys.clone(),
            ExitKind::YieldProtocol { tys } | ExitKind::LoopBody { tys, .. } => tys.clone(),
        }
    }

    /// The chain kind the code AFTER a guard on this chain runs on.
    fn for_tail(&self) -> ExitKind {
        match self {
            ExitKind::Function | ExitKind::YieldReturn => ExitKind::YieldReturn,
            other => other.clone(),
        }
    }

    /// Whether `exit` terminating a block on this chain IS the target loop's back edge.
    ///
    /// A `continue` that terminates its own target's body chain means exactly `scf.yield <args>`:
    /// control reaches the header with those arguments either way. Recognizing it here is what
    /// keeps the common `continue` — the one a frontend emits as the last statement of a loop
    /// body — from paying for a flag, a store and a load per iteration.
    fn is_back_edge(&self, exit: &dyn RegionExitTerminatorOpInterface) -> bool {
        let ExitKind::LoopBody { target, .. } = self else {
            return false;
        };
        exit.get_exit_kind() == RegionExitKind::Continue
            && exit.get_exit_target().is_some_and(|t| OperationRef::ptr_eq(&t, target))
    }
}

/// One exit channel: a flag local saying "an exit on this channel is pending", plus one value
/// local per value the exit carries.
struct Channel {
    flag: LocalVariable,
    slots: Vec<LocalVariable>,
    tys: Vec<Type>,
}

/// Identifies one of a function's exit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelId {
    /// The function-return channel (`did_return`).
    Return,
    /// The channel for exits of one kind addressing one loop — `did_break_L`/`did_continue_L` —
    /// at this index in [Channels::exits].
    Loop(usize),
}

/// A loop-addressed channel, and the loop it addresses.
struct LoopChannel {
    target: OperationRef,
    kind: RegionExitKind,
    channel: Channel,
}

/// Every exit channel one function needs.
struct Channels {
    context: Rc<Context>,
    /// The function-return channel, present when some region-nested `builtin.ret` needs one.
    ret: Option<Channel>,
    /// One channel per (loop, exit kind) some `scf.break`/`scf.continue` in this function
    /// addresses. A loop nothing addresses gets none, and pays nothing.
    exits: Vec<LoopChannel>,
}

impl Channels {
    fn builder(&self) -> OpBuilder {
        OpBuilder::new(Rc::clone(&self.context))
    }

    fn ret_channel(&self) -> &Channel {
        self.ret
            .as_ref()
            .expect("the return channel is only consulted when a return needs it")
    }

    fn channel(&self, id: ChannelId) -> &Channel {
        match id {
            ChannelId::Return => self.ret_channel(),
            ChannelId::Loop(index) => &self.exits[index].channel,
        }
    }

    /// The index of the channel for exits of `kind` addressing `target`, if there is one.
    fn find_loop(&self, target: &OperationRef, kind: RegionExitKind) -> Option<usize> {
        self.exits
            .iter()
            .position(|lc| lc.kind == kind && OperationRef::ptr_eq(&lc.target, target))
    }

    /// The channel `op` exits on, if `op` exits at all and has a channel.
    ///
    /// A `continue` which is its own target's back edge has no channel: it is folded to
    /// `scf.yield` instead (see [ExitKind::is_back_edge]).
    fn id_of(&self, op: &Operation) -> Option<ChannelId> {
        if is_ret(op) {
            return self.ret.is_some().then_some(ChannelId::Return);
        }
        let exit = op.as_trait::<dyn RegionExitTerminatorOpInterface>()?;
        let target = exit.get_exit_target()?;
        self.find_loop(&target, exit.get_exit_kind()).map(ChannelId::Loop)
    }
}

/// The channels a pending exit inside `op`'s regions can still be on when control reaches the
/// operation AFTER `op` — i.e. the ones the tail must be guarded on.
///
/// This is computed BEFORE `op`'s regions are rewritten, because the rewrite is exactly what
/// removes the exits it asks about.
fn outward_channels(channels: &Channels, op: &Operation, active: &[ChannelId]) -> Vec<ChannelId> {
    active
        .iter()
        .copied()
        .filter(|id| op_regions_contain_exit(channels, op, *id))
        .collect()
}

/// Load the disjunction of the flags for `ids`.
/// A single channel needs only one `hir.load_local`.
fn load_any_pending(
    channels: &Channels,
    builder: &mut OpBuilder,
    ids: &[ChannelId],
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    let mut ids = ids.iter();
    let first = ids.next().expect("a guard is only built for a non-empty channel set");
    let mut pending = builder.load_local(channels.channel(*first).flag, span)?;
    for id in ids {
        let next = builder.load_local(channels.channel(*id).flag, span)?;
        pending = builder.or(pending, next, span)?;
    }
    Ok(pending)
}

/// What a function's structured exits need from this pass.
struct ExitInventory {
    /// One entry per (loop, kind) pair some exit addresses and which needs a channel, in walk
    /// order.
    channels: Vec<(OperationRef, RegionExitKind)>,
    /// Whether the function contains a structured exit AT ALL. A function whose only exit is a
    /// back edge needs no channel but still needs the pass — folding that exit to `scf.yield` is
    /// what keeps it from reaching the emitter.
    any: bool,
}

/// Take inventory of every structured exit in `function`.
///
/// Resolution goes through [RegionExitTerminatorOpInterface], so the pass never reads `levels`
/// itself: which loop an exit names is the dialect's answer, given once, and the verifier is what
/// guarantees it exists.
fn collect_exit_targets(function: builtin::FunctionRef) -> Result<ExitInventory, Report> {
    let mut inventory = ExitInventory {
        channels: Vec::new(),
        any: false,
    };
    let mut unresolved = None;
    function.borrow().as_operation().prewalk_all(|nested: &Operation| {
        let Some(exit) = nested.as_trait::<dyn RegionExitTerminatorOpInterface>() else {
            return;
        };
        inventory.any = true;
        let Some(target) = exit.get_exit_target() else {
            unresolved.get_or_insert_with(|| nested.name().to_string());
            return;
        };
        let kind = exit.get_exit_kind();

        // A `continue` terminating its target's body IS the back edge and needs no channel.
        if kind == RegionExitKind::Continue && terminates_body_of(nested, &target) {
            return;
        }
        if !inventory
            .channels
            .iter()
            .any(|(t, k)| *k == kind && OperationRef::ptr_eq(t, &target))
        {
            inventory.channels.push((target, kind));
        }
    });

    if let Some(name) = unresolved {
        return Err(Report::msg(format!(
            "cannot lower early exit: '{name}' does not name an enclosing loop, which the \
             verifier rejects"
        )));
    }
    Ok(inventory)
}

/// Whether `op` is the terminator of `target`'s body (its `after` region's entry block).
fn terminates_body_of(op: &Operation, target: &OperationRef) -> bool {
    let Ok(while_op) = target.try_downcast_op::<scf::While>() else {
        return false;
    };
    let Some(after_entry) = while_op.borrow().after().entry_block_ref() else {
        return false;
    };
    after_entry
        .borrow()
        .body()
        .back()
        .get()
        .is_some_and(|term| OperationRef::ptr_eq(&term.as_operation_ref(), &op.as_operation_ref()))
}

/// The types the values on `target`'s `kind` channel carry.
///
/// For a break they are the loop's results (which `scf.condition` forwards on the exiting edge);
/// for a continue they are the header's block arguments — the same list `scf.yield` carries on
/// the ordinary back edge.
fn channel_types(target: &OperationRef, kind: RegionExitKind) -> Result<Vec<Type>, Report> {
    let target_op = target.borrow();
    match kind {
        RegionExitKind::Break => Ok(target_op
            .results()
            .all()
            .iter()
            .map(|result| result.borrow().ty().clone())
            .collect()),
        RegionExitKind::Continue => {
            let header = target_op
                .as_trait::<dyn HasBreakingControlFlowOpInterface>()
                .and_then(|receiver| receiver.get_continue_target_region())
                .ok_or_else(|| {
                    Report::msg(format!(
                        "cannot lower early exit: '{}' receives a continue but names no header \
                         region",
                        target_op.name()
                    ))
                })?;
            Ok(header
                .borrow()
                .entry()
                .arguments()
                .iter()
                .map(|arg| arg.borrow().ty().clone())
                .collect())
        }
    }
}

/// Rewrite `function` into single-exit form if any of its structured control flow regions
/// contain an operation that leaves them. Returns true if the function was changed.
fn lower_function_early_exits(
    mut function: builtin::FunctionRef,
    context: Rc<Context>,
) -> Result<bool, Report> {
    let entry = {
        let function = function.borrow();
        if function.is_declaration() {
            return Ok(false);
        }
        function.entry_block()
    };

    let needs_return_channel = first_op_with_nested_ret(entry).is_some();
    let inventory = collect_exit_targets(function)?;
    if !needs_return_channel && !inventory.any {
        return Ok(false);
    }

    let span = function.borrow().span();
    let ret = needs_return_channel.then(|| {
        let tys: Vec<Type> = {
            let function = function.borrow();
            let signature = function.signature();
            signature.results.iter().map(|result| result.ty.clone()).collect()
        };
        let flag = function.borrow_mut().alloc_local(Type::I1);
        let slots: Vec<LocalVariable> =
            tys.iter().map(|ty| function.borrow_mut().alloc_local(ty.clone())).collect();
        Channel { flag, slots, tys }
    });

    let mut exits = Vec::with_capacity(inventory.channels.len());
    for (target, kind) in inventory.channels {
        let tys = channel_types(&target, kind)?;
        let flag = function.borrow_mut().alloc_local(Type::I1);
        let slots: Vec<LocalVariable> =
            tys.iter().map(|ty| function.borrow_mut().alloc_local(ty.clone())).collect();
        exits.push(LoopChannel {
            target,
            kind,
            channel: Channel { flag, slots, tys },
        });
    }

    let channels = Channels {
        context,
        ret,
        exits,
    };

    // Initialize the return flag at function entry (note: insertion at block START does not
    // advance, so position after the constant before building the store). The LOOP channels are
    // cleared immediately before their loop instead, because a loop can be entered more than
    // once and a stale flag would make the next entry exit at the header.
    let mut active = Vec::new();
    if let Some(channel) = channels.ret.as_ref() {
        let mut builder = channels.builder();
        builder.set_insertion_point_to_start(entry);
        let initial = builder.i1(false, span);
        let initial_op =
            initial.borrow().get_defining_op().expect("constant must have a defining op");
        builder.set_insertion_point_after(initial_op);
        builder.store_local(channel.flag, initial, span)?;
        active.push(ChannelId::Return);
    }

    process_block(&channels, entry, ExitKind::Function, &active, span)?;
    Ok(true)
}

/// Rewrite `block` so that no operation nested inside the regions of its operations leaves it,
/// guarding all code that follows a may-exit operation with that operation's exit flags.
///
/// `active` names the channels an exit inside this block can still be pending on when control
/// reaches the block's own terminator: the function's, plus one per enclosing loop that some
/// exit addresses. A channel belonging to a loop NESTED in this block is deliberately absent —
/// such an exit is consumed by that loop's own header rewrite and never reaches here.
fn process_block(
    channels: &Channels,
    mut block: BlockRef,
    kind: ExitKind,
    active: &[ChannelId],
    span: SourceSpan,
) -> Result<(), Report> {
    let mut cursor = block.borrow().body().front().get().map(|op| op.as_operation_ref());
    while let Some(op) = cursor {
        let next = op.next();
        // BOTH questions are asked before the rewrite, because the rewrite is what removes the
        // exits they ask about.
        let (descend, guard_channels) = {
            let op = op.borrow();
            (op_regions_contain_any_exit(&op), outward_channels(channels, &op, active))
        };
        if descend {
            rewrite_op_regions(channels, op, active, span)?;
        }
        if !guard_channels.is_empty() {
            // Everything after `op` runs only if no region of `op` left: split it off and guard.
            let guard_tys = kind.yield_types(channels);
            let next = next.expect("a may-exit operation must also have a terminator after it");
            let mut tail = block.borrow_mut().split_block(next);

            let mut builder = channels.builder();
            builder.set_insertion_point_to_end(block);
            let pending = load_any_pending(channels, &mut builder, &guard_channels, span)?;
            let guard = builder.r#if(pending, &guard_tys, span)?;

            // then: the pending exit propagates outward
            {
                let then_region = guard.borrow().then_body().as_region_ref();
                let then_block = builder.create_block(then_region, None, &[]);
                builder.set_insertion_point_to_end(then_block);
                let values = pending_values(channels, &kind, &mut builder, &guard_tys, span)?;
                builder.r#yield(values, span)?;
            }

            // else: the tail of the original block continues
            let else_block = {
                let else_region = guard.borrow().else_body().as_region_ref();
                let mut else_block = builder.create_block(else_region, None, &[]);
                else_block.borrow_mut().splice_block(&mut tail.borrow_mut());
                // The (now empty) temporary block from the split is discarded
                let mut region = tail.borrow().parent().expect("split block must be attached");
                let mut region = region.borrow_mut();
                unsafe {
                    let mut cursor = region.body_mut().cursor_mut_from_ptr(tail);
                    cursor.remove();
                }
                else_block
            };

            // After the guard, produce the guard's results in the form this block's context
            // expects
            builder.set_insertion_point_to_end(block);
            let guard_results: Vec<ValueRef> = guard
                .borrow()
                .results()
                .all()
                .iter()
                .map(|result| result.borrow().as_value_ref())
                .collect();
            match &kind {
                ExitKind::Function => {
                    builder.ret(guard_results, span)?;
                }
                ExitKind::YieldReturn
                | ExitKind::YieldProtocol { .. }
                | ExitKind::LoopBody { .. } => {
                    builder.r#yield(guard_results, span)?;
                }
            }

            // The tail (now the guard's else block) yields the values the guard forwards;
            // process it recursively (it may contain further may-exit operations, and its
            // terminator — the block's original one — must convert to yield form).
            return process_block(channels, else_block, kind.for_tail(), active, span);
        }
        cursor = next;
    }

    // No operation in this block leaves it; the only remaining concern is a terminator that does.
    convert_terminator(channels, block, &kind, span)
}

/// The values a guard's then-branch produces when an exit is pending on this chain.
fn pending_values(
    channels: &Channels,
    kind: &ExitKind,
    builder: &mut OpBuilder,
    tys: &[Type],
    span: SourceSpan,
) -> Result<Vec<ValueRef>, Report> {
    let slots = match kind {
        // On the function-result chain the guard PRODUCES the function results from the value
        // locals (the chain forwards them straight to the final `builtin.ret`)
        ExitKind::Function | ExitKind::YieldReturn => Some(&channels.ret_channel().slots),
        // On a loop body's own chain the values ARE the next iteration's arguments, so a pending
        // continue surfaces here exactly as a pending return does on the function chain
        ExitKind::LoopBody {
            channel: Some(index),
            ..
        } => Some(&channels.exits[*index].channel.slots),
        // In a nested region the values live in the locals already; the region's own consumers
        // never observe these fillers (every guard on the way out is taken)
        ExitKind::YieldProtocol { .. } | ExitKind::LoopBody { channel: None, .. } => None,
    };
    match slots {
        Some(slots) => slots
            .iter()
            .map(|slot| builder.load_local(*slot, span))
            .collect::<Result<_, _>>(),
        None => Ok(tys.iter().map(|ty| builder.poison(ty.clone(), span)).collect()),
    }
}

/// Convert `block`'s terminator, if it is one that leaves the block, into the form this chain
/// expects.
fn convert_terminator(
    channels: &Channels,
    block: BlockRef,
    kind: &ExitKind,
    span: SourceSpan,
) -> Result<(), Report> {
    let Some(terminator) = block.borrow().body().back().get().map(|op| op.as_operation_ref())
    else {
        return Ok(());
    };
    if terminator.borrow().implements::<dyn RegionExitTerminatorOpInterface>() {
        return convert_exit_to_protocol(channels, terminator, kind, span);
    }
    match kind {
        ExitKind::Function => Ok(()),
        ExitKind::YieldReturn => convert_terminator_to_direct_yield(channels, block, span),
        ExitKind::YieldProtocol { tys } | ExitKind::LoopBody { tys, .. } => {
            convert_terminator_to_protocol(channels, block, tys, span)
        }
    }
}

/// Convert a return-like terminator on the function-result chain to a direct `scf.yield` of
/// its values (the guard chain forwards them to the final `builtin.ret`).
fn convert_terminator_to_direct_yield(
    channels: &Channels,
    block: BlockRef,
    span: SourceSpan,
) -> Result<(), Report> {
    let Some(terminator) = block.borrow().body().back().get().map(|op| op.as_operation_ref())
    else {
        return Ok(());
    };
    let term = terminator.borrow();
    let ret_values: Vec<ValueRef> = if let Some(ret) = term.downcast_ref::<builtin::Ret>() {
        ret.values().iter().map(|operand| operand.borrow().as_value_ref()).collect()
    } else if let Some(ret_imm) = term.downcast_ref::<builtin::RetImm>() {
        let imm = *ret_imm.get_value();
        drop(term);
        let mut builder = channels.builder();
        builder.set_insertion_point_before(terminator);
        let op_builder = builder.create::<arith::Constant, (Immediate,)>(span);
        let constant = op_builder(imm)?;
        let value = constant.borrow().result().as_value_ref();
        builder.r#yield([value], span)?;
        remove_op(terminator);
        return Ok(());
    } else {
        return Ok(());
    };
    drop(term);

    let mut builder = channels.builder();
    builder.set_insertion_point_before(terminator);
    builder.r#yield(ret_values, span)?;
    remove_op(terminator);
    Ok(())
}

/// Detach and discard `op` from its block.
fn remove_op(op: OperationRef) {
    let mut block = op.parent().expect("operation must be attached");
    let mut block = block.borrow_mut();
    unsafe {
        let mut cursor = block.body_mut().cursor_mut_from_ptr(op);
        cursor.remove();
    }
}

/// Convert a return-like terminator of a region block to the exit-channel protocol: store the
/// returned values in the value locals, set the flag, and yield type-correct fillers.
fn convert_terminator_to_protocol(
    channels: &Channels,
    block: BlockRef,
    yield_tys: &[Type],
    span: SourceSpan,
) -> Result<(), Report> {
    let Some(terminator) = block.borrow().body().back().get().map(|op| op.as_operation_ref())
    else {
        return Ok(());
    };
    let term = terminator.borrow();
    let ret_values: Vec<ValueRef> = if let Some(ret) = term.downcast_ref::<builtin::Ret>() {
        ret.values().iter().map(|operand| operand.borrow().as_value_ref()).collect()
    } else if let Some(ret_imm) = term.downcast_ref::<builtin::RetImm>() {
        let imm = *ret_imm.get_value();
        drop(term);
        let mut builder = channels.builder();
        builder.set_insertion_point_before(terminator);
        let op_builder = builder.create::<arith::Constant, (Immediate,)>(span);
        let constant = op_builder(imm)?;
        let value = constant.borrow().result().as_value_ref();
        return store_and_flag(
            channels,
            ChannelId::Return,
            terminator,
            alloc::vec![value],
            yield_tys,
            span,
        );
    } else {
        // scf.yield / ub.unreachable / scf.condition terminators are already in the right form
        return Ok(());
    };
    drop(term);

    store_and_flag(channels, ChannelId::Return, terminator, ret_values, yield_tys, span)
}

/// Convert an `scf.break`/`scf.continue` terminator to its channel's protocol.
fn convert_exit_to_protocol(
    channels: &Channels,
    terminator: OperationRef,
    kind: &ExitKind,
    span: SourceSpan,
) -> Result<(), Report> {
    let args: Vec<ValueRef> = terminator
        .borrow()
        .operands()
        .all()
        .iter()
        .map(|operand| operand.borrow().as_value_ref())
        .collect();

    // A `continue` terminating its own target's body chain IS the back edge: yielding its
    // operands reaches the header with exactly the same arguments, so it costs nothing and
    // needs no channel.
    let is_back_edge = {
        let term = terminator.borrow();
        let exit = term
            .as_trait::<dyn RegionExitTerminatorOpInterface>()
            .expect("caller checked the interface");
        kind.is_back_edge(exit)
    };
    if is_back_edge {
        let mut builder = channels.builder();
        builder.set_insertion_point_before(terminator);
        builder.r#yield(args, span)?;
        remove_op(terminator);
        return Ok(());
    }

    let id = channels.id_of(&terminator.borrow()).ok_or_else(|| {
        Report::msg(format!(
            "cannot lower early exit: '{}' does not name an enclosing loop, which the verifier \
             rejects",
            terminator.borrow().name()
        ))
    })?;
    let (ExitKind::YieldProtocol { tys } | ExitKind::LoopBody { tys, .. }) = kind else {
        return Err(Report::msg(format!(
            "cannot lower early exit: '{}' is not inside a loop of this function",
            terminator.borrow().name()
        )));
    };
    let tys = tys.clone();
    store_and_flag(channels, id, terminator, args, &tys, span)
}

/// Store `values` in `id`'s value locals, set its flag, and replace `terminator` with a
/// type-correct filler yield.
fn store_and_flag(
    channels: &Channels,
    id: ChannelId,
    terminator: OperationRef,
    values: Vec<ValueRef>,
    yield_tys: &[Type],
    span: SourceSpan,
) -> Result<(), Report> {
    let channel = channels.channel(id);
    debug_assert_eq!(values.len(), channel.slots.len(), "an exit's arity must match its channel's");
    let mut builder = channels.builder();
    builder.set_insertion_point_before(terminator);
    for (slot, value) in channel.slots.iter().zip(values) {
        builder.store_local(*slot, value, span)?;
    }
    let flag_set = builder.i1(true, span);
    builder.store_local(channel.flag, flag_set, span)?;
    let fillers: Vec<ValueRef> =
        yield_tys.iter().map(|ty| builder.poison(ty.clone(), span)).collect();
    builder.r#yield(fillers, span)?;

    // Remove the original exiting terminator
    remove_op(terminator);
    Ok(())
}

/// Rewrite the regions of `op` (a may-exit operation) to the exit-channel protocol.
///
/// The backend supports propagation through `scf.if`, `scf.index_switch`, and `scf.while`.
/// Other region-bearing operations require their own normalization support.
fn rewrite_op_regions(
    channels: &Channels,
    op: OperationRef,
    active: &[ChannelId],
    span: SourceSpan,
) -> Result<(), Report> {
    if let Ok(if_op) = op.try_downcast_op::<scf::If>() {
        let result_tys: Vec<Type> = {
            let if_op = if_op.borrow();
            if_op
                .as_operation()
                .results()
                .all()
                .iter()
                .map(|result| result.borrow().ty().clone())
                .collect()
        };
        let regions: Vec<BlockRef> = {
            let if_op = if_op.borrow();
            [if_op.then_body().entry_block_ref(), if_op.else_body().entry_block_ref()]
                .into_iter()
                .flatten()
                .collect()
        };
        for region_entry in regions {
            process_block(
                channels,
                region_entry,
                ExitKind::YieldProtocol {
                    tys: result_tys.clone(),
                },
                active,
                span,
            )?;
        }
        return Ok(());
    }

    if let Ok(switch_op) = op.try_downcast_op::<scf::IndexSwitch>() {
        let result_tys: Vec<Type> = {
            let switch_op = switch_op.borrow();
            switch_op
                .as_operation()
                .results()
                .all()
                .iter()
                .map(|result| result.borrow().ty().clone())
                .collect()
        };
        let regions: Vec<BlockRef> = {
            let switch_op = switch_op.borrow();
            switch_op
                .as_operation()
                .regions()
                .iter()
                .filter_map(|region| region.entry_block_ref())
                .collect()
        };
        for region_entry in regions {
            process_block(
                channels,
                region_entry,
                ExitKind::YieldProtocol {
                    tys: result_tys.clone(),
                },
                active,
                span,
            )?;
        }
        return Ok(());
    }

    if let Ok(while_op) = op.try_downcast_op::<scf::While>() {
        return rewrite_while_op(channels, while_op, active, span);
    }

    Err(Report::msg(format!(
        "cannot lower early exit: an early exit inside a region of '{}' is not supported by MASM \
         codegen",
        op.borrow().name()
    )))
}

/// Rewrite an `scf.while` some exit leaves, or which contains a loop-carried exit of its own.
///
/// The body's exits convert to their channel's protocol like any other region, and the BEFORE
/// region (the condition) is rewritten to check the escaping flags first:
///
/// ```text
/// before { <ops>; scf.condition(%c, %fwd..) }
/// =>
/// before {
///     %did = hir.load_local <flag>            // or `%did = or(<flag>, <flag>..)`
///     %g:N = scf.if %did { yield false, <exit values> } else { <ops>; yield %c, %fwd.. }
///     scf.condition(%g#0, %g#1..)
/// }
/// ```
///
/// so an iteration that returned, or broke out of this loop or an outer one, exits the loop
/// before the body can run again. On the exiting edge `scf.condition` forwards the loop's
/// RESULTS, which is why `scf.break %v` yields `%v` there: a break addressed to this loop is the
/// one channel whose values this rewrite reads back out.
fn rewrite_while_op(
    channels: &Channels,
    while_op: midenc_hir::UnsafeIntrusiveEntityRef<scf::While>,
    active: &[ChannelId],
    span: SourceSpan,
) -> Result<(), Report> {
    let op_ref = while_op.borrow().as_operation().as_operation_ref();
    let (mut before_entry, after_entry) = {
        let while_op = while_op.borrow();
        (
            while_op
                .before()
                .entry_block_ref()
                .expect("scf.while must have a 'before' region"),
            while_op
                .after()
                .entry_block_ref()
                .expect("scf.while must have an 'after' region"),
        )
    };

    let break_channel = channels.find_loop(&op_ref, RegionExitKind::Break);
    let continue_channel = channels.find_loop(&op_ref, RegionExitKind::Continue);

    // The channels whose pending exit must stop this loop: everything that escapes it, plus a
    // break addressed to it. A CONTINUE addressed to it is deliberately absent — re-entering the
    // header is what the loop does anyway once the body's chain is guarded.
    let mut escaping = active.to_vec();
    if let Some(index) = break_channel {
        escaping.push(ChannelId::Loop(index));
    }
    let guard_channels = outward_channels(channels, &op_ref.borrow(), &escaping);

    // Exits from the condition to this loop or an ancestor are unsupported: this rewrite
    // consumes pending exits at header entry, but cannot propagate an exit raised inside it.
    {
        let mut fenced = escaping.clone();
        if let Some(index) = continue_channel {
            fenced.push(ChannelId::Loop(index));
        }
        let before = before_entry.borrow();
        let condition_exits = before.body().iter().any(|op| {
            fenced.iter().any(|id| {
                channels.id_of(&op) == Some(*id) || op_regions_contain_exit(channels, &op, *id)
            })
        });
        if condition_exits {
            return Err(Report::msg(
                "cannot lower early exit: an early exit inside a loop CONDITION is not supported \
                 by MASM codegen",
            ));
        }
    }

    // Exits consumed by loops nested inside the condition do not leave it. Normalize those
    // regions without guarding the outer condition's tail or changing its terminator.
    let nested: Vec<OperationRef> = before_entry
        .borrow()
        .body()
        .iter()
        .filter(|op| op_regions_contain_any_exit(op))
        .map(|op| op.as_operation_ref())
        .collect();
    for op in nested {
        rewrite_op_regions(channels, op, &[], span)?;
    }

    // Clear both channels on every entry. Break flags may survive an earlier entry into a
    // nested loop; continue flags must be initialized before the first body guard reads them.
    // Physical procedure locals can retain values from earlier calls.
    for index in [break_channel, continue_channel].into_iter().flatten() {
        let mut builder = channels.builder();
        builder.set_insertion_point_before(op_ref);
        let clear = builder.i1(false, span);
        builder.store_local(channels.exits[index].channel.flag, clear, span)?;
    }

    // The loop body yields the next iteration's condition-region arguments
    let body_yield_tys: Vec<Type> = {
        let before = before_entry.borrow();
        before.arguments().iter().map(|arg| arg.borrow().ty().clone()).collect()
    };
    let mut body_active = active.to_vec();
    body_active.extend(break_channel.map(ChannelId::Loop));
    body_active.extend(continue_channel.map(ChannelId::Loop));
    process_block(
        channels,
        after_entry,
        ExitKind::LoopBody {
            target: op_ref,
            channel: continue_channel,
            tys: body_yield_tys,
        },
        &body_active,
        span,
    )?;

    // The continue channel is consumed by the back edge, so it is cleared at the END of the body
    // — after every guard that reads it, and before the header, which does not test it at all.
    if let Some(index) = continue_channel {
        let terminator = after_entry
            .borrow()
            .body()
            .back()
            .get()
            .map(|op| op.as_operation_ref())
            .expect("'after' region must have a terminator");
        let mut builder = channels.builder();
        builder.set_insertion_point_before(terminator);
        let clear = builder.i1(false, span);
        builder.store_local(channels.exits[index].channel.flag, clear, span)?;
    }

    // A loop nothing escapes needs no header guard: its own `continue`s are already handled.
    if guard_channels.is_empty() {
        return Ok(());
    }

    // Rewrite the condition region to exit the loop when an escaping flag is set
    let condition_op = before_entry
        .borrow()
        .body()
        .back()
        .get()
        .map(|op| op.as_operation_ref())
        .expect("'before' region must have a terminator");
    let (cond_value, forwarded): (ValueRef, Vec<ValueRef>) = {
        let condition = condition_op
            .try_downcast_op::<scf::Condition>()
            .map_err(|_| Report::msg("'before' region of scf.while must end in scf.condition"))?;
        let condition = condition.borrow();
        (
            condition.condition().as_value_ref(),
            condition
                .forwarded()
                .iter()
                .map(|operand| operand.borrow().as_value_ref())
                .collect(),
        )
    };
    let forwarded_tys: Vec<Type> =
        forwarded.iter().map(|value| value.borrow().ty().clone()).collect();

    // Move the ENTIRE original condition body (including its terminator, for now) aside
    let first_op = before_entry
        .borrow()
        .body()
        .front()
        .get()
        .map(|op| op.as_operation_ref())
        .expect("'before' region cannot be empty");
    let mut moved = before_entry.borrow_mut().split_block(first_op);

    let mut builder = channels.builder();
    builder.set_insertion_point_to_end(before_entry);
    let pending = load_any_pending(channels, &mut builder, &guard_channels, span)?;
    let mut guard_tys = alloc::vec![Type::I1];
    guard_tys.extend(forwarded_tys.iter().cloned());
    let guard = builder.r#if(pending, &guard_tys, span)?;
    {
        let then_region = guard.borrow().then_body().as_region_ref();
        let then_block = builder.create_block(then_region, None, &[]);
        builder.set_insertion_point_to_end(then_block);
        let mut values = alloc::vec![builder.i1(false, span)];
        match break_channel.filter(|index| guard_channels.contains(&ChannelId::Loop(*index))) {
            // A break addressed to THIS loop carries the loop's results, and this edge is where
            // `scf.condition` forwards them
            Some(index) => {
                for slot in channels.exits[index].channel.slots.iter() {
                    values.push(builder.load_local(*slot, span)?);
                }
            }
            // Only escaping channels remain, and a pending exit on one of them means nothing
            // downstream ever observes this loop's results
            None => {
                for ty in &forwarded_tys {
                    values.push(builder.poison(ty.clone(), span));
                }
            }
        }
        builder.r#yield(values, span)?;
    }
    {
        let else_region = guard.borrow().else_body().as_region_ref();
        let mut else_block = builder.create_block(else_region, None, &[]);
        else_block.borrow_mut().splice_block(&mut moved.borrow_mut());
        // Discard the temporary block from the split
        let mut region = moved.borrow().parent().expect("split block must be attached");
        let mut region = region.borrow_mut();
        unsafe {
            let mut cursor = region.body_mut().cursor_mut_from_ptr(moved);
            cursor.remove();
        }
        // Replace the moved scf.condition terminator with a yield of (condition, forwarded..)
        builder.set_insertion_point_before(condition_op);
        let mut values = alloc::vec![cond_value];
        values.extend(forwarded.iter().copied());
        builder.r#yield(values, span)?;
        let mut else_block_mut = else_block.borrow_mut();
        unsafe {
            let mut cursor = else_block_mut.body_mut().cursor_mut_from_ptr(condition_op);
            cursor.remove();
        }
    }

    // And the new condition: scf.condition(%g#0, %g#1..)
    builder.set_insertion_point_to_end(before_entry);
    let guard_results: Vec<ValueRef> = guard
        .borrow()
        .results()
        .all()
        .iter()
        .map(|result| result.borrow().as_value_ref())
        .collect();
    let new_cond = guard_results[0];
    let new_forwarded = guard_results[1..].to_vec();
    builder.condition(new_cond, new_forwarded, span)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;
    use midenc_hir::{
        SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, formatter::PrettyPrint,
        testing::Test,
    };

    use super::*;
    use crate::select::{
        SelectEnv,
        tests::{Parsed, execute_code, ready},
    };

    /// Select, allocate, verify, and execute the normalized function.
    fn emit_masm(test: &Test, inputs: &[usize], expected: &[usize]) -> Result<String, Report> {
        let parsed = Parsed {
            context: test.context_rc(),
            function: test.function(),
        };
        let (plan, frame, body) = ready(&parsed, &SelectEnv::default());
        let procedure = crate::verify::verify(&plan, &frame, body)?
            .into_masm(crate::masm::ProcedureName::new("normalized").unwrap())?
            .to_pretty_string();
        let mut results = expected.to_vec();
        results.extend([901, 902]);
        let program = format!(
            "{procedure} begin push.902 push.901 {} exec.normalized {} end",
            crate::plan::tests::lowering_units::push_top(inputs),
            crate::plan::tests::lowering_units::assert_top(&results)
        );
        execute_code(&program)?;
        Ok(procedure)
    }

    /// Build `fn(): u32` counting `0, 1, 2, ..` in an `scf.while`, breaking out of it with the
    /// current counter when it reaches `stop_at`.
    ///
    /// The loop's own condition is `true`, so **the break is the only way out**: a lowering that
    /// drops the exit produces a loop that never terminates, and one that mis-wires the exit
    /// value produces the wrong result.
    fn build_loop_with_break(test: &mut Test, stop_at: u32) {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let init = builder.u32(0, span);
        let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
        {
            let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
            let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
            builder.switch_to_block(before_entry);
            let keep_going = builder.i1(true, span);
            builder.condition(keep_going, [carried], span).unwrap();
        }
        {
            let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
            let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
            builder.switch_to_block(after_entry);
            let limit = builder.u32(stop_at, span);
            let done = builder.eq(carried, limit, span).unwrap();
            let conditional = builder.r#if(done, &[], span).unwrap();
            {
                let then_region = conditional.borrow().then_body().as_region_ref();
                let then_block = builder.create_block_in_region(then_region);
                builder.switch_to_block(then_block);
                builder.r#break(1, [carried], span).unwrap();
            }
            {
                let else_region = conditional.borrow().else_body().as_region_ref();
                let else_block = builder.create_block_in_region(else_region);
                builder.switch_to_block(else_block);
                builder.r#yield([], span).unwrap();
            }
            builder.switch_to_block(after_entry);
            let one = builder.u32(1, span);
            let next = builder.add(carried, one, span).unwrap();
            builder.r#yield([next], span).unwrap();
        }
        builder.switch_to_block(entry);
        let result = while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
        builder.ret([result], span).unwrap();
    }

    #[test]
    fn legality_explains_that_structured_exits_require_normalization() {
        let mut test = Test::new("unnormalized_break", &[], &[Type::U32]);
        build_loop_with_break(&mut test, 3);
        let error = test
            .apply_pass::<crate::CheckMasmLegality>(true)
            .expect_err("selection requires normalized exits");
        assert!(format!("{error:?}").contains("lower-early-exits"), "{error:?}");
    }

    #[test]
    fn aggregate_return_payloads_survive_normalization() {
        let ty = Type::from(midenc_hir::StructType::new([Type::U32, Type::U32]));
        let mut test = Test::new("aggregate_exit", &[Type::I1, ty.clone(), ty.clone()], &[ty]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let flag = entry.borrow().arguments()[0].borrow().as_value_ref();
            let value = entry.borrow().arguments()[1].borrow().as_value_ref();
            let conditional = builder.r#if(flag, &[], span).unwrap();
            let then_region = conditional.borrow().then_body().as_region_ref();
            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            builder.ret([value], span).unwrap();
            let else_region = conditional.borrow().else_body().as_region_ref();
            let else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(else_block);
            builder.r#yield([], span).unwrap();
            builder.switch_to_block(entry);
            let normal = entry.borrow().arguments()[2].borrow().as_value_ref();
            builder.ret([normal], span).unwrap();
        }
        test.apply_pass::<LowerEarlyExits>(true).unwrap();
        test.apply_pass::<crate::LegalizeForMasm>(true).unwrap();
        emit_masm(&test, &[1, 11, 23, 37, 41], &[11, 23]).unwrap();
        emit_masm(&test, &[0, 11, 23, 37, 41], &[37, 41]).unwrap();
    }

    #[test]
    fn aggregate_break_payloads_survive_normalization() {
        for (ty, values) in [
            (Type::from(midenc_hir::StructType::new([Type::U32, Type::U32])), vec![11, 23]),
            (Type::from(midenc_hir::ArrayType::new(Type::U8, 9)), vec![11, 23, 37]),
            (
                Type::from(midenc_hir::ArrayType::new(Type::Felt, 4)),
                vec![0xffff_ffff_0000_0000, 23, 37, 41],
            ),
        ] {
            let mut test =
                Test::new("aggregate_break", std::slice::from_ref(&ty), std::slice::from_ref(&ty));
            {
                let span = SourceSpan::UNKNOWN;
                let mut builder = test.function_builder();
                let entry = builder.entry_block();
                let value = entry.borrow().arguments()[0].borrow().as_value_ref();
                let target = builder.r#while([value], &[ty], span).unwrap();
                let header = target.borrow().before().entry_block_ref().unwrap();
                let body = target.borrow().after().entry_block_ref().unwrap();
                builder.switch_to_block(header);
                let value = header.borrow().arguments()[0].borrow().as_value_ref();
                let condition = builder.i1(true, span);
                builder.condition(condition, [value], span).unwrap();
                builder.switch_to_block(body);
                let value = body.borrow().arguments()[0].borrow().as_value_ref();
                builder.r#break(1, [value], span).unwrap();
                builder.switch_to_block(entry);
                let result = target.borrow().results().all()[0].borrow().as_value_ref();
                builder.ret([result], span).unwrap();
            }
            test.apply_pass::<LowerEarlyExits>(true).unwrap();
            test.apply_pass::<crate::LegalizeForMasm>(true).unwrap();
            emit_masm(&test, &values, &values).unwrap();
        }
    }

    /// Build `fn(): u32` whose loop holds a `levels = 1` break inside an `scf.if` in the
    /// condition region, which is valid SCF but unsupported by this lowering.
    fn build_loop_with_break_in_the_condition(test: &mut Test) {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let init = builder.u32(0, span);
        let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
        {
            let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
            let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
            builder.switch_to_block(before_entry);
            let limit = builder.u32(3, span);
            let done = builder.eq(carried, limit, span).unwrap();
            let conditional = builder.r#if(done, &[], span).unwrap();
            {
                let then_region = conditional.borrow().then_body().as_region_ref();
                let then_block = builder.create_block_in_region(then_region);
                builder.switch_to_block(then_block);
                builder.r#break(1, [carried], span).unwrap();
            }
            {
                let else_region = conditional.borrow().else_body().as_region_ref();
                let else_block = builder.create_block_in_region(else_region);
                builder.switch_to_block(else_block);
                builder.r#yield([], span).unwrap();
            }
            builder.switch_to_block(before_entry);
            let keep_going = builder.i1(true, span);
            builder.condition(keep_going, [carried], span).unwrap();
        }
        {
            let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
            let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
            builder.switch_to_block(after_entry);
            let one = builder.u32(1, span);
            let next = builder.add(carried, one, span).unwrap();
            builder.r#yield([next], span).unwrap();
        }
        builder.switch_to_block(entry);
        let result = while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
        builder.ret([result], span).unwrap();
    }

    /// A break originating in its target's condition is valid SCF and interpretable, but
    /// unsupported by this normalization.
    #[test]
    fn an_exit_inside_a_loop_condition_is_fenced() {
        let mut test = Test::new("break_in_while_condition", &[], &[Type::U32]);
        build_loop_with_break_in_the_condition(&mut test);

        // The restriction belongs to codegen; the dialect accepts this exit.
        test.function()
            .as_operation_ref()
            .borrow()
            .recursively_verify()
            .expect("a break in a loop's before region is legal IR");

        // The interpreter executes the break before the body runs at counter 3.
        assert_eq!(interpret(&test, &[]).unwrap(), [3]);

        let err = test
            .apply_pass::<LowerEarlyExits>(true)
            .expect_err("an exit in the loop CONDITION must be refused, loudly");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("an early exit inside a loop CONDITION is not supported"),
            "the refusal must name the shape it refuses: {rendered}"
        );
    }

    /// A loop in another loop's condition may consume its own exits without leaving the
    /// enclosing condition. Normalize those exits on every evaluation of the condition.
    #[test]
    fn self_contained_loop_in_a_condition_normalizes_its_exits() {
        let mut test = Test::new("loop_in_while_condition", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let init = builder.u32(0, span);
            let outer = builder.r#while([init], &[Type::U32], span).unwrap();
            let outer_header = outer.borrow().before().entry_block_ref().unwrap();
            let outer_body = outer.borrow().after().entry_block_ref().unwrap();

            builder.switch_to_block(outer_header);
            let current = outer_header.borrow().arguments()[0].borrow().as_value_ref();
            let inner = builder.r#while([current], &[Type::U32], span).unwrap();
            let inner_header = inner.borrow().before().entry_block_ref().unwrap();
            let inner_body = inner.borrow().after().entry_block_ref().unwrap();

            builder.switch_to_block(inner_header);
            let carried = inner_header.borrow().arguments()[0].borrow().as_value_ref();
            let keep_going = builder.i1(true, span);
            builder.condition(keep_going, [carried], span).unwrap();

            builder.switch_to_block(inner_body);
            let carried = inner_body.borrow().arguments()[0].borrow().as_value_ref();
            let one = builder.u32(1, span);
            let next = builder.add(carried, one, span).unwrap();
            builder.r#break(1, [next], span).unwrap();

            builder.switch_to_block(outer_header);
            let next = inner.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            let limit = builder.u32(3, span);
            let keep_going = builder.lt(next, limit, span).unwrap();
            builder.condition(keep_going, [next], span).unwrap();

            builder.switch_to_block(outer_body);
            let next = outer_body.borrow().arguments()[0].borrow().as_value_ref();
            builder.r#yield([next], span).unwrap();

            builder.switch_to_block(entry);
            let result = outer.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        test.function().as_operation_ref().borrow().recursively_verify().unwrap();
        assert_eq!(interpret(&test, &[]).unwrap(), [3]);
        test.apply_pass::<LowerEarlyExits>(true).unwrap();
        test.function().borrow().as_operation().prewalk_all(|op: &Operation| {
            assert!(
                !op.implements::<dyn RegionExitTerminatorOpInterface>(),
                "normalization left an exit inside the condition: {op}"
            );
        });
        assert_single_block_regions(&test);
        assert_eq!(interpret(&test, &[]).unwrap(), [3]);
        emit_masm(&test, &[], &[3]).unwrap();
    }

    /// The `scf.if` guarding a block's tail, if the block ends in one.
    ///
    /// Every exit channel this pass installs is read back by a guard, so "which value does the
    /// guard branch on, and what does its exiting arm produce" is the question that separates a
    /// wired-up channel from a dropped one.
    fn tail_guard(block: BlockRef) -> midenc_hir::UnsafeIntrusiveEntityRef<scf::If> {
        let terminator =
            block.borrow().body().back().get().map(|op| op.as_operation_ref()).unwrap();
        let value = terminator.borrow().operands().all()[0].borrow().as_value_ref();
        value
            .borrow()
            .get_defining_op()
            .expect("the terminator's operand must be an op result")
            .try_downcast_op::<scf::If>()
            .unwrap_or_else(|_| panic!("the block's tail must be guarded by an scf.if"))
    }

    /// The `local_variable` index an `scf.if`'s condition is loaded from.
    fn guarded_on_local(guard: &midenc_hir::UnsafeIntrusiveEntityRef<scf::If>) -> usize {
        let condition = guard.borrow().condition().as_value_ref();
        let load = condition.borrow().get_defining_op().expect("the flag must be an op result");
        let load = load.try_downcast_op::<midenc_dialect_hir::LoadLocal>().unwrap_or_else(|_| {
            panic!("a guard branches on a flag LOCAL, not on a recomputed value")
        });
        load.borrow().get_local().as_usize()
    }

    /// The `index`-th `scf.while` in `test`'s function, in walk order.
    fn the_while_op(test: &Test, index: usize) -> midenc_hir::UnsafeIntrusiveEntityRef<scf::While> {
        let function = test.function();
        let function = function.borrow();
        let mut while_ops = Vec::new();
        function.as_operation().prewalk_all(|op: &Operation| {
            if op.downcast_ref::<scf::While>().is_some() {
                while_ops.push(op.as_operation_ref());
            }
        });
        while_ops
            .get(index)
            .unwrap_or_else(|| panic!("expected at least {} scf.while ops", index + 1))
            .try_downcast_op::<scf::While>()
            .unwrap_or_else(|_| unreachable!())
    }

    /// The printed operations of `block`, one per line.
    fn block_text(block: BlockRef) -> alloc::string::String {
        let block = block.borrow();
        block
            .body()
            .iter()
            .map(|op| alloc::format!("{op}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Evaluate `test`'s function on `args` with the HIR interpreter.
    ///
    /// Comparing execution before and after normalization checks that the channel protocol
    /// preserves the native exits' semantics.
    fn interpret(test: &Test, args: &[Immediate]) -> Result<Vec<u32>, Report> {
        use midenc_hir_eval::HirEvaluator;

        let mut evaluator = HirEvaluator::new(test.context_rc());
        let function = test.function();
        let callable = function.borrow();
        let arguments = args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>();
        let results = evaluator.eval_callable(&*callable, arguments)?;
        Ok(results
            .iter()
            .map(|value| match value {
                midenc_hir_eval::Value::Immediate(imm) => {
                    imm.as_u64().expect("expected an integer result") as u32
                }
                other => panic!("expected an immediate result, got {other:?}"),
            })
            .collect())
    }

    /// Check that normalization splices away every temporary block introduced by splitting.
    fn assert_single_block_regions(test: &Test) {
        let function = test.function();
        let function = function.borrow();
        let mut offenders = Vec::new();
        function.as_operation().prewalk_all(|op: &Operation| {
            for (index, region) in op.regions().iter().enumerate() {
                let blocks = region.body().iter().count();
                if blocks != 1 {
                    offenders.push(alloc::format!("{}'s region #{index}: {blocks}", op.name()));
                }
            }
        });
        assert!(
            offenders.is_empty(),
            "every region must still have exactly one block, but: {offenders:?}"
        );
    }

    /// The printed exiting arm of `while_op`'s header guard — what the loop produces on the edge
    /// that leaves it.
    fn header_guard_exiting_arm(
        while_op: &midenc_hir::UnsafeIntrusiveEntityRef<scf::While>,
    ) -> alloc::string::String {
        let condition_op = while_op.borrow().condition_op().expect("loop lost its header");
        let guard = condition_op
            .borrow()
            .condition()
            .as_value_ref()
            .borrow()
            .get_defining_op()
            .expect("the rewritten loop condition must be an op result")
            .try_downcast_op::<scf::If>()
            .unwrap_or_else(|_| panic!("each crossed loop's header must be guarded"));
        let then_block = guard.borrow().then_body().entry_block_ref().unwrap();
        block_text(then_block)
    }

    /// A `scf.break` reaches MASM codegen: the pass must rewrite it into the loop-carried-flag
    /// form, so that the emitter never sees one.
    ///
    /// The channel is `did_break_L` (local 0) plus one value local (local 1) for the loop's
    /// single result, and the two ends of it are asserted separately: the break STORES, and the
    /// loop header READS BACK — a lowering that wired only one end would still emit MASM.
    #[test]
    fn break_in_while_body_lowers_to_the_exit_channel() {
        let mut original = Test::new("break_in_while_body_before", &[], &[Type::U32]);
        build_loop_with_break(&mut original, 3);
        // EXECUTED, before the pass: the interpreter runs `scf.break` natively
        assert_eq!(interpret(&original, &[]).unwrap(), [3]);

        let mut test = Test::new("break_in_while_body", &[], &[Type::U32]);
        build_loop_with_break(&mut test, 3);

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        // EXECUTED, after the pass: the SAME answer out of the flag-and-locals form. A rewrite
        // whose shape is plausible and whose value is wrong dies here and nowhere else.
        assert_eq!(interpret(&test, &[]).unwrap(), [3]);

        let hir = test.function().borrow().as_operation().to_string();
        // Nothing structured-exiting survives the pass: this is what the emitter's Report is for
        assert_eq!(hir.matches("scf.break").count(), 0, "{hir}");
        // The channel: a flag local and the target loop's one result local. There is no
        // `did_return` — this function returns only at its end, and an unused channel is a
        // wasted local in every program the pass touches.
        assert!(hir.contains("local_variable<0, i1>"), "{hir}");
        assert!(hir.contains("local_variable<1, u32>"), "{hir}");

        let while_op = the_while_op(&test, 0);
        // The header exits the loop when the flag is set, and forwards the break's value as the
        // loop's result on exactly that edge
        let condition_op =
            while_op.borrow().condition_op().expect("rewritten loop lost its header");
        let header_guard = condition_op
            .borrow()
            .condition()
            .as_value_ref()
            .borrow()
            .get_defining_op()
            .expect("the rewritten loop condition must be an op result")
            .try_downcast_op::<scf::If>()
            .unwrap_or_else(|_| panic!("the loop condition must be produced by the exit guard"));
        assert_eq!(guarded_on_local(&header_guard), 0, "{hir}");
        let exiting = header_guard_exiting_arm(&while_op);
        assert!(
            exiting.contains("hir.load_local"),
            "the exiting edge forwards the break's value\n{exiting}\n{hir}"
        );

        // The break itself stores into that channel before yielding a filler; the third store is
        // the flag CLEAR, which sits before the loop so that a second entry into it (a loop
        // nested in another) does not exit at its header on a flag the first entry left set.
        assert_eq!(hir.matches("hir.store_local").count(), 3, "{hir}");
        assert!(hir.contains("ub.poison"), "{hir}");
        let entry_text = block_text(test.function().borrow().entry_block());
        let clear_at = entry_text.find("hir.store_local");
        let loop_at = entry_text.find("scf.while");
        assert!(
            matches!((clear_at, loop_at), (Some(clear), Some(loop_at)) if clear < loop_at),
            "the break flag must be cleared BEFORE the loop\n{entry_text}"
        );

        // And it all emits: the loop is one `while.true`, the flag is a memory round trip
        assert_single_block_regions(&test);
        let masm = emit_masm(&test, &[], &[3]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 1, "{masm}");
    }

    /// A `levels = 2` break leaves BOTH loops, and each one is rewritten for it: the inner loop
    /// gets a header guard for a channel it does not own, and the outer loop gets the guard that
    /// reads the break's value back out.
    ///
    /// This is the shape the interpreter's nested-break witness executes, at the other end of the
    /// pipeline. A pass that resolved `levels` one loop too shallow still lowers, still emits, and
    /// still terminates — it just leaves the outer loop running.
    #[test]
    fn nested_break_rewrites_every_loop_it_crosses() {
        let mut test = Test::new("nested_break", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let outer_init = builder.u32(0, span);
            let outer = builder.r#while([outer_init], &[Type::U32], span).unwrap();
            {
                let before_entry = outer.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let keep_going = builder.i1(true, span);
                builder.condition(keep_going, [carried], span).unwrap();
            }
            let outer_body = outer.borrow().after().entry_block_ref().unwrap();
            let outer_carried = outer_body.borrow().arguments()[0].borrow().as_value_ref();
            builder.switch_to_block(outer_body);
            let inner_init = builder.u32(0, span);
            let inner = builder.r#while([inner_init], &[Type::U32], span).unwrap();
            {
                let before_entry = inner.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let keep_going = builder.i1(true, span);
                builder.condition(keep_going, [carried], span).unwrap();
            }
            {
                let after_entry = inner.borrow().after().entry_block_ref().unwrap();
                let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(after_entry);
                let limit = builder.u32(2, span);
                let done = builder.eq(carried, limit, span).unwrap();
                let conditional = builder.r#if(done, &[], span).unwrap();
                {
                    let then_region = conditional.borrow().then_body().as_region_ref();
                    let then_block = builder.create_block_in_region(then_region);
                    builder.switch_to_block(then_block);
                    // levels = 2: leave the INNER loop and the OUTER one, with `carried`
                    builder.r#break(2, [carried], span).unwrap();
                }
                {
                    let else_region = conditional.borrow().else_body().as_region_ref();
                    let else_block = builder.create_block_in_region(else_region);
                    builder.switch_to_block(else_block);
                    builder.r#yield([], span).unwrap();
                }
                builder.switch_to_block(after_entry);
                let one = builder.u32(1, span);
                let next = builder.add(carried, one, span).unwrap();
                builder.r#yield([next], span).unwrap();
            }
            builder.switch_to_block(outer_body);
            let inner_result =
                inner.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            let sum = builder.add(outer_carried, inner_result, span).unwrap();
            builder.r#yield([sum], span).unwrap();
            builder.switch_to_block(entry);
            let result = outer.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        // EXECUTED, before the pass: the interpreter unwinds the `levels = 2` break itself
        assert_eq!(interpret(&test, &[]).unwrap(), [2]);

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        // EXECUTED, after: the same answer. A pass that resolved `levels` one loop too shallow
        // leaves the outer loop running and answers differently here.
        assert_eq!(interpret(&test, &[]).unwrap(), [2]);

        let hir = test.function().borrow().as_operation().to_string();
        assert_eq!(hir.matches("scf.break").count(), 0, "{hir}");
        // ONE channel, for the loop the break actually addresses. A pass that opened a channel
        // per crossed loop would allocate four locals here.
        assert!(hir.contains("local_variable<0, i1>"), "{hir}");
        assert!(hir.contains("local_variable<1, u32>"), "{hir}");
        assert!(!hir.contains("local_variable<2,"), "{hir}");

        // BOTH loops are rewritten, and both headers read the SAME flag: the inner one because
        // the exit crosses it, the outer one because the exit ends there.
        let outer = the_while_op(&test, 0);
        let inner = the_while_op(&test, 1);
        for while_op in [&outer, &inner] {
            let condition_op = while_op.borrow().condition_op().expect("loop lost its header");
            let guard = condition_op
                .borrow()
                .condition()
                .as_value_ref()
                .borrow()
                .get_defining_op()
                .unwrap()
                .try_downcast_op::<scf::If>()
                .unwrap_or_else(|_| panic!("each crossed loop's header must be guarded"));
            assert_eq!(guarded_on_local(&guard), 0, "{hir}");
        }
        // Only the OUTER header reads the value back: the inner loop's results are never
        // observed on the exiting path, so it forwards a filler instead.
        let outer_exiting = header_guard_exiting_arm(&outer);
        let inner_exiting = header_guard_exiting_arm(&inner);
        assert!(outer_exiting.contains("hir.load_local"), "{outer_exiting}\n{hir}");
        assert!(!inner_exiting.contains("hir.load_local"), "{inner_exiting}\n{hir}");
        assert!(inner_exiting.contains("ub.poison"), "{inner_exiting}\n{hir}");

        assert_single_block_regions(&test);
        let masm = emit_masm(&test, &[], &[2]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 2, "{masm}");
    }

    /// **A loop that BREAKS is entered again, and must break again.**
    ///
    /// The inner loop's only exit is a break; the outer loop runs it twice. The break's value
    /// depends on the outer counter, so a second entry that exits at the header on a flag the
    /// first entry left set returns the FIRST entry's value — the right shape, the wrong number,
    /// and nothing red anywhere else.
    ///
    /// This is the case the pre-loop flag clear exists for, and it is the only one in which that
    /// clear is observable at runtime: with it the answer is 3 + 4 = **7**; without it the second
    /// entry re-reads the stale slot and the answer is 3 + 3 = **6**.
    #[test]
    fn a_loop_that_breaks_is_entered_again_and_breaks_again() {
        let mut test = Test::new("re_entered_break", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let acc_init = builder.u32(0, span);
            let i_init = builder.u32(0, span);
            let outer = builder.r#while([acc_init, i_init], &[Type::U32, Type::U32], span).unwrap();
            {
                let before_entry = outer.borrow().before().entry_block_ref().unwrap();
                let (acc, i) = {
                    let block = before_entry.borrow();
                    (
                        block.arguments()[0].borrow().as_value_ref(),
                        block.arguments()[1].borrow().as_value_ref(),
                    )
                };
                builder.switch_to_block(before_entry);
                let two = builder.u32(2, span);
                let more = builder.lt(i, two, span).unwrap();
                builder.condition(more, [acc, i], span).unwrap();
            }
            {
                let after_entry = outer.borrow().after().entry_block_ref().unwrap();
                let (acc, i) = {
                    let block = after_entry.borrow();
                    (
                        block.arguments()[0].borrow().as_value_ref(),
                        block.arguments()[1].borrow().as_value_ref(),
                    )
                };
                builder.switch_to_block(after_entry);
                let inner_init = builder.u32(0, span);
                let inner = builder.r#while([inner_init], &[Type::U32], span).unwrap();
                {
                    let before_entry = inner.borrow().before().entry_block_ref().unwrap();
                    let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                    builder.switch_to_block(before_entry);
                    // The inner loop's ONLY way out is the break
                    let always = builder.i1(true, span);
                    builder.condition(always, [carried], span).unwrap();
                }
                {
                    let inner_body = inner.borrow().after().entry_block_ref().unwrap();
                    let carried = inner_body.borrow().arguments()[0].borrow().as_value_ref();
                    builder.switch_to_block(inner_body);
                    let three = builder.u32(3, span);
                    let done = builder.eq(carried, three, span).unwrap();
                    let conditional = builder.r#if(done, &[], span).unwrap();
                    {
                        let then_region = conditional.borrow().then_body().as_region_ref();
                        let then_block = builder.create_block_in_region(then_region);
                        builder.switch_to_block(then_block);
                        // The value DEPENDS on which outer iteration this is
                        let value = builder.add(carried, i, span).unwrap();
                        builder.r#break(1, [value], span).unwrap();
                    }
                    {
                        let else_region = conditional.borrow().else_body().as_region_ref();
                        let else_block = builder.create_block_in_region(else_region);
                        builder.switch_to_block(else_block);
                        builder.r#yield([], span).unwrap();
                    }
                    builder.switch_to_block(inner_body);
                    let one = builder.u32(1, span);
                    let next = builder.add(carried, one, span).unwrap();
                    builder.r#yield([next], span).unwrap();
                }
                builder.switch_to_block(after_entry);
                let inner_result =
                    inner.borrow().as_operation().results().all()[0].borrow().as_value_ref();
                let acc_next = builder.add(acc, inner_result, span).unwrap();
                let one = builder.u32(1, span);
                let i_next = builder.add(i, one, span).unwrap();
                builder.r#yield([acc_next, i_next], span).unwrap();
            }
            builder.switch_to_block(entry);
            let acc_final =
                outer.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([acc_final], span).unwrap();
        }

        // EXECUTED, before the pass: 3 on the first outer iteration, 4 on the second
        assert_eq!(interpret(&test, &[]).unwrap(), [7]);

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        // EXECUTED, after: the same. Dropping the pre-loop clear gives 6 here.
        assert_eq!(interpret(&test, &[]).unwrap(), [7]);

        assert_single_block_regions(&test);
        let masm = emit_masm(&test, &[], &[7]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 2, "{masm}");
    }

    /// A nested continue skips the body tail and carries arguments to the next iteration.
    /// Clearing its flag at the back edge allows subsequent iterations to execute normally.
    #[test]
    fn continue_in_while_body_carries_the_next_iteration() {
        let mut test = Test::new("continue_in_while_body", &[Type::I1], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let skip = entry.borrow().arguments()[0].borrow().as_value_ref();
            let init = builder.u32(0, span);
            let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
            {
                let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let limit = builder.u32(5, span);
                let keep_going = builder.lt(carried, limit, span).unwrap();
                builder.condition(keep_going, [carried], span).unwrap();
            }
            {
                let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
                let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(after_entry);
                let one = builder.u32(1, span);
                let next = builder.add(carried, one, span).unwrap();
                let conditional = builder.r#if(skip, &[], span).unwrap();
                {
                    let then_region = conditional.borrow().then_body().as_region_ref();
                    let then_block = builder.create_block_in_region(then_region);
                    builder.switch_to_block(then_block);
                    builder.r#continue(1, [next], span).unwrap();
                }
                {
                    let else_region = conditional.borrow().else_body().as_region_ref();
                    let else_block = builder.create_block_in_region(else_region);
                    builder.switch_to_block(else_block);
                    builder.r#yield([], span).unwrap();
                }
                builder.switch_to_block(after_entry);
                let ten = builder.u32(10, span);
                let skipped = builder.add(next, ten, span).unwrap();
                builder.r#yield([skipped], span).unwrap();
            }
            builder.switch_to_block(entry);
            let result =
                while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        // EXECUTED, before the pass, in BOTH polarities: with the continue taken every
        // iteration the loop counts to 5; without it the first iteration jumps to 11
        assert_eq!(interpret(&test, &[Immediate::I1(true)]).unwrap(), [5]);
        assert_eq!(interpret(&test, &[Immediate::I1(false)]).unwrap(), [11]);

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        // EXECUTED, after: both answers survive. A flag never cleared at the end of the body
        // would take the continue path forever after the first one, and this is where that shows.
        assert_eq!(interpret(&test, &[Immediate::I1(true)]).unwrap(), [5]);
        assert_eq!(interpret(&test, &[Immediate::I1(false)]).unwrap(), [11]);

        let hir = test.function().borrow().as_operation().to_string();
        assert_eq!(hir.matches("scf.continue").count(), 0, "{hir}");
        // The continue channel carries the loop's ITER ARGS, not its results — here they have the
        // same type, so the count is what says the channel exists at all
        assert!(hir.contains("local_variable<0, i1>"), "{hir}");
        assert!(hir.contains("local_variable<1, u32>"), "{hir}");

        let while_op = the_while_op(&test, 0);
        // The HEADER IS UNTOUCHED: a continue re-enters it, so nothing about the loop's exit
        // condition changes. This is the assertion that separates continue from break.
        let condition_op = while_op.borrow().condition_op().expect("loop lost its header");
        let condition = condition_op.borrow().condition().as_value_ref();
        let header = condition.borrow().get_defining_op().unwrap();
        assert!(
            header.borrow().downcast_ref::<scf::If>().is_none(),
            "a continue must not add a header guard, got '{}'\n{hir}",
            header.borrow().name()
        );

        // The body's tail is guarded on the continue flag, and its exiting arm produces the
        // continue's operands as the next iteration's arguments
        let body = while_op.borrow().after().entry_block_ref().unwrap();
        let guard = tail_guard(body);
        assert_eq!(guarded_on_local(&guard), 0, "{hir}");
        let continuing = guard.borrow().then_body().entry_block_ref().unwrap();
        let continuing = block_text(continuing);
        assert!(continuing.contains("hir.load_local"), "{continuing}\n{hir}");

        // And the flag is cleared at the END of the body: without this the loop would take the
        // continue path forever after the first one
        let body_ops = block_text(body);
        let clear_at_end =
            body_ops.lines().rev().take(3).any(|line| line.contains("hir.store_local"));
        assert!(clear_at_end, "the continue flag must be cleared at the end of the body\n{hir}");

        // The first iteration must also start with a clear flag; physical locals may retain
        // values from an earlier call. Interpreter locals alone cannot exercise this condition.
        let entry_text = block_text(test.function().borrow().entry_block());
        let clear_at = entry_text.find("hir.store_local");
        let loop_at = entry_text.find("scf.while");
        assert!(
            matches!((clear_at, loop_at), (Some(clear), Some(loop_at)) if clear < loop_at),
            "the continue flag must be cleared BEFORE the loop\n{entry_text}"
        );

        assert_single_block_regions(&test);
        let masm = emit_masm(&test, &[1], &[5]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 1, "{masm}");
    }

    /// A return and a break in the same loop retain independent flags and payloads.
    /// Execute both winning paths in the interpreter and the VM.
    #[test]
    fn a_return_and_a_break_in_one_loop_keep_their_own_channels() {
        fn build(test: &mut Test, bail_at: u32, break_at: u32) {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let init = builder.u32(0, span);
            let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
            {
                let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let keep_going = builder.i1(true, span);
                builder.condition(keep_going, [carried], span).unwrap();
            }
            {
                let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
                let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(after_entry);

                // The `?` shape: a `builtin.ret` nested in an `scf.if`
                let bail_limit = builder.u32(bail_at, span);
                let bail = builder.eq(carried, bail_limit, span).unwrap();
                let bail_if = builder.r#if(bail, &[], span).unwrap();
                {
                    let then_region = bail_if.borrow().then_body().as_region_ref();
                    let then_block = builder.create_block_in_region(then_region);
                    builder.switch_to_block(then_block);
                    let hundred = builder.u32(100, span);
                    let value = builder.add(carried, hundred, span).unwrap();
                    builder.ret([value], span).unwrap();
                }
                {
                    let else_region = bail_if.borrow().else_body().as_region_ref();
                    let else_block = builder.create_block_in_region(else_region);
                    builder.switch_to_block(else_block);
                    builder.r#yield([], span).unwrap();
                }

                // And the break, in the same body, on the same loop
                builder.switch_to_block(after_entry);
                let break_limit = builder.u32(break_at, span);
                let stop = builder.eq(carried, break_limit, span).unwrap();
                let break_if = builder.r#if(stop, &[], span).unwrap();
                {
                    let then_region = break_if.borrow().then_body().as_region_ref();
                    let then_block = builder.create_block_in_region(then_region);
                    builder.switch_to_block(then_block);
                    builder.r#break(1, [carried], span).unwrap();
                }
                {
                    let else_region = break_if.borrow().else_body().as_region_ref();
                    let else_block = builder.create_block_in_region(else_region);
                    builder.switch_to_block(else_block);
                    builder.r#yield([], span).unwrap();
                }

                builder.switch_to_block(after_entry);
                let one = builder.u32(1, span);
                let next = builder.add(carried, one, span).unwrap();
                builder.r#yield([next], span).unwrap();
            }
            builder.switch_to_block(entry);
            let result =
                while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        // The RETURN channel wins: the `?` fires at j = 2, before the break at j = 4
        let mut bails = Test::new("compose_bails", &[], &[Type::U32]);
        build(&mut bails, 2, 4);
        assert_eq!(interpret(&bails, &[]).unwrap(), [102]);
        bails.apply_pass::<LowerEarlyExits>(true).unwrap();
        assert_eq!(interpret(&bails, &[]).unwrap(), [102]);
        emit_masm(&bails, &[], &[102]).unwrap();

        // The BREAK channel wins: the break fires at j = 3, and the `?` never does
        let mut breaks = Test::new("compose_breaks", &[], &[Type::U32]);
        build(&mut breaks, 9, 3);
        assert_eq!(interpret(&breaks, &[]).unwrap(), [3]);
        breaks.apply_pass::<LowerEarlyExits>(true).unwrap();
        assert_eq!(interpret(&breaks, &[]).unwrap(), [3]);

        // The header stops for either channel: `did_return || did_break_L`.
        let hir = breaks.function().borrow().as_operation().to_string();
        assert!(hir.contains("arith.or"), "{hir}");
        assert!(hir.contains("local_variable<0, i1>"), "{hir}");
        assert!(hir.contains("local_variable<2, i1>"), "{hir}");

        assert_single_block_regions(&breaks);
        let masm = emit_masm(&breaks, &[], &[3]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 1, "{masm}");
    }

    /// A `continue` that already terminates its target's body IS the back edge: it folds to
    /// `scf.yield`, opening no channel and costing no local.
    #[test]
    fn a_back_edge_continue_folds_to_a_yield() {
        let mut test = Test::new("back_edge_continue", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let init = builder.u32(0, span);
            let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
            {
                let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let limit = builder.u32(5, span);
                let keep_going = builder.lt(carried, limit, span).unwrap();
                builder.condition(keep_going, [carried], span).unwrap();
            }
            {
                let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
                let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(after_entry);
                let one = builder.u32(1, span);
                let next = builder.add(carried, one, span).unwrap();
                builder.r#continue(1, [next], span).unwrap();
            }
            builder.switch_to_block(entry);
            let result =
                while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        assert_eq!(interpret(&test, &[]).unwrap(), [5]);

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        assert_eq!(interpret(&test, &[]).unwrap(), [5]);

        let hir = test.function().borrow().as_operation().to_string();
        assert_eq!(hir.matches("scf.continue").count(), 0, "{hir}");
        assert!(hir.contains("scf.yield"), "{hir}");
        // No channel: no flag, no value local, no store
        assert!(!hir.contains("local_variable"), "{hir}");
        assert!(!hir.contains("hir.store_local"), "{hir}");

        assert_single_block_regions(&test);
        let masm = emit_masm(&test, &[], &[5]).unwrap();
        assert_eq!(masm.matches("while.true").count(), 1, "{masm}");
    }

    /// `return` under a conditional becomes single-exit form: the region's ret converts to the
    /// exit-channel protocol (store value + flag), the tail is guarded on the flag, and exactly
    /// one `builtin.ret` remains, at function level.
    #[test]
    fn return_under_a_conditional_lowers_to_single_exit_form() {
        let mut test = Test::new("return_under_a_conditional_lowers", &[Type::I1], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let flag = entry.borrow().arguments()[0].borrow().as_value_ref();
            let conditional = builder.r#if(flag, &[], span).unwrap();
            {
                let then_region = conditional.borrow().then_body().as_region_ref();
                let then_block = builder.create_block_in_region(then_region);
                builder.switch_to_block(then_block);
                let early = builder.u32(100, span);
                builder.ret([early], span).unwrap();
            }
            {
                let else_region = conditional.borrow().else_body().as_region_ref();
                let else_block = builder.create_block_in_region(else_region);
                builder.switch_to_block(else_block);
                builder.r#yield([], span).unwrap();
            }
            builder.switch_to_block(entry);
            let fallthrough = builder.u32(7, span);
            builder.ret([fallthrough], span).unwrap();
        }

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        let output = test.function().borrow().as_operation().to_string();
        // The nested ret is gone: exactly one `builtin.ret` remains
        assert_eq!(output.matches("builtin.ret").count(), 1, "{output}");
        // The exit channel: an i1 flag local plus the u32 value local
        assert!(output.contains("local_variable<0, i1>"), "{output}");
        assert!(output.contains("local_variable<1, u32>"), "{output}");
        // The tail is guarded: the flag is re-loaded and branched on
        assert!(output.contains("hir.load_local"), "{output}");
    }

    /// A return in the body guards the next header evaluation, so the condition cannot run
    /// again after the function has returned.
    #[test]
    fn return_in_while_body_guards_the_loop_condition() {
        let mut test = Test::new("return_in_while_body_guards", &[Type::I1], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let flag = entry.borrow().arguments()[0].borrow().as_value_ref();
            let init = builder.u32(0, span);
            let while_op = builder.r#while([init], &[Type::U32], span).unwrap();
            {
                let before_entry = while_op.borrow().before().entry_block_ref().unwrap();
                let carried = before_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(before_entry);
                let keep_going = builder.i1(true, span);
                builder.condition(keep_going, [carried], span).unwrap();
            }
            {
                let after_entry = while_op.borrow().after().entry_block_ref().unwrap();
                let carried = after_entry.borrow().arguments()[0].borrow().as_value_ref();
                builder.switch_to_block(after_entry);
                let conditional = builder.r#if(flag, &[], span).unwrap();
                {
                    let then_region = conditional.borrow().then_body().as_region_ref();
                    let then_block = builder.create_block_in_region(then_region);
                    builder.switch_to_block(then_block);
                    let early = builder.u32(100, span);
                    builder.ret([early], span).unwrap();
                }
                {
                    let else_region = conditional.borrow().else_body().as_region_ref();
                    let else_block = builder.create_block_in_region(else_region);
                    builder.switch_to_block(else_block);
                    builder.r#yield([], span).unwrap();
                }
                builder.switch_to_block(after_entry);
                builder.r#yield([carried], span).unwrap();
            }
            builder.switch_to_block(entry);
            let result =
                while_op.borrow().as_operation().results().all()[0].borrow().as_value_ref();
            builder.ret([result], span).unwrap();
        }

        test.apply_pass::<LowerEarlyExits>(true).unwrap();

        let output = test.function().borrow().as_operation().to_string();
        // Single-exit form: the nested ret is gone, the exit channel exists
        assert_eq!(output.matches("builtin.ret").count(), 1, "{output}");
        assert!(output.contains("local_variable<0, i1>"), "{output}");
        assert!(output.contains("local_variable<1, u32>"), "{output}");
        // The protocol's type-correct fillers appear (condition guard + body protocol)
        assert!(output.contains("ub.poison"), "{output}");

        // The CONDITION REGION splice, structurally: the surviving while's before region
        // still terminates in `scf.condition`, whose condition operand is now produced by
        // the flag-guard `scf.if` (yield false when the flag is set, else the original
        // condition), and the guard's own ops load the flag first.
        let function = test.function();
        let function = function.borrow();
        let mut while_ops = Vec::new();
        function.as_operation().prewalk_all(|op: &Operation| {
            if op.downcast_ref::<scf::While>().is_some() {
                while_ops.push(op.as_operation_ref());
            }
        });
        assert_eq!(while_ops.len(), 1, "{output}");
        let while_op = while_ops.pop().unwrap().try_downcast_op::<scf::While>().ok().unwrap();
        let condition_op =
            while_op.borrow().condition_op().expect("rewritten loop lost its header");
        let cond_value = condition_op.borrow().condition().as_value_ref();
        let guard = cond_value
            .borrow()
            .get_defining_op()
            .expect("the rewritten loop condition must be an op result");
        assert!(
            guard.borrow().downcast_ref::<scf::If>().is_some(),
            "the loop condition must be produced by the did_return guard, got '{}' in\n{output}",
            guard.borrow().name()
        );
        assert!(output.contains("hir.load_local"), "{output}");
    }

    /// A function with no nested returns is untouched (the pass reports no change).
    #[test]
    fn single_exit_functions_are_untouched() {
        let mut test = Test::new("single_exit_functions_are_untouched", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let value = builder.u32(7, span);
            builder.ret([value], span).unwrap();
        }

        let before = test.function().borrow().as_operation().to_string();
        test.apply_pass::<LowerEarlyExits>(false).unwrap();
        let after = test.function().borrow().as_operation().to_string();
        assert_eq!(before, after);
    }
}
