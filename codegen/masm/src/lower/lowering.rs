use midenc_dialect_arith as arith;
use midenc_dialect_cf as cf;
use midenc_dialect_hir as hir;
use midenc_dialect_scf as scf;
use midenc_dialect_ub as ub;
use midenc_dialect_wasm as wasm;
use midenc_hir::{
    Op, Span, Symbol, SymbolTable,
    dialects::{builtin, debuginfo},
};
use midenc_session::diagnostics::{Report, Severity, Spanned};

#[cfg(test)]
use crate::emitter::FrameLayout;
use crate::masm;

/// The module containing world-level procedures when no component declares its own namespace.
pub(super) fn world_root_module(world: &builtin::World) -> masm::PathBuf {
    let body = world.body();
    let entry = body.entry();
    let mut modules = entry.body().iter().filter(|op| op.is::<builtin::Module>());
    if let Some(module) = modules.next()
        && modules.next().is_none()
    {
        return module
            .downcast_ref::<builtin::Module>()
            .unwrap()
            .path()
            .to_library_path()
            .to_absolute()
            .unwrap()
            .into_owned();
    }
    masm::PathBuf::from(masm::Path::new("::init"))
}

/// Map a HIR symbol's owner to the module where its MASM definition is emitted.
///
/// A world has no path segment of its own, but its procedures and aliases live in the chosen
/// root module. Imports, calls, and function-table fragments must all use that same mapping.
pub(super) fn symbol_module_path(symbol: &dyn Symbol) -> masm::PathBuf {
    if let Some(owner) = symbol.as_symbol_operation().nearest_symbol_table()
        && let Some(world) = owner.borrow().downcast_ref::<builtin::World>()
    {
        return world_root_module(world);
    }
    symbol
        .path()
        .without_leaf()
        .to_library_path()
        .to_absolute()
        .unwrap()
        .into_owned()
}

/// The fully qualified name of a callable in its emitted MASM module.
pub(super) fn qualified_invocation_target(
    callee: &dyn Symbol,
    span: midenc_hir::SourceSpan,
) -> masm::InvocationTarget {
    let proc_name = callee.name();
    let proc_name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
        masm::Span::new(span, proc_name.as_ref().into()),
    ));
    let module = symbol_module_path(callee);
    let qualified = masm::QualifiedProcedureName::new(module.as_path(), proc_name);
    masm::InvocationTarget::Path(masm::Span::new(span, qualified.into_inner()))
}

/// Builds a MASM invocation target for a resolved HIR callable.
///
/// `module_owner` is the HIR symbol-table owner corresponding to the emitted caller's MASM module.
/// Passing `None` disables unqualified-name emission. Private aliases outside that module are
/// followed to a reachable symbol, since MASM cannot resolve private imports by qualified path.
pub(crate) fn invocation_target_from_symbol(
    mut symbol: midenc_hir::SymbolRef,
    module_owner: Option<midenc_hir::OperationRef>,
    span: midenc_hir::SourceSpan,
) -> masm::InvocationTarget {
    loop {
        let callee = symbol.borrow();
        let alias = callee.as_symbol_operation().downcast_ref::<builtin::FunctionAlias>();
        let Some(alias) = alias.filter(|_| callee.is_private()) else {
            return qualified_invocation_target(&*callee, span);
        };

        if let Some(owner) = module_owner
            && let Some(target_owner) = alias.as_operation().nearest_symbol_table()
            && crate::legalization::share_masm_module(owner, target_owner)
        {
            return masm::InvocationTarget::Symbol(masm::Ident::from_raw_parts(Span::new(
                span,
                callee.name().as_str().into(),
            )));
        }

        // Alias verification validates the chain before emission. Follow each hop in the
        // alias's own symbol table, preserving the first accessible name.
        symbol = alias.resolve_target().expect("canonical resolution validated the alias target");
    }
}

pub(super) fn resolve_invocation_callee(
    call: &dyn midenc_hir::CallOpInterface,
) -> Result<midenc_hir::ResolvedSymbolCallee, Report> {
    call.resolve_symbol_callee().map_err(|err| {
        let mut diagnostic = call
            .as_operation()
            .context()
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(format!(
                "invalid {}: unable to resolve callee",
                call.as_operation().name()
            ))
            .with_primary_label(call.as_operation().span(), err.to_string());
        if matches!(err, midenc_hir::SymbolResolutionError::UnknownSymbol { .. }) {
            diagnostic = diagnostic.with_help(
                "Make sure that all referenced symbols are reachable via the root symbol table, \
                 and use absolute paths to refer to symbols in ancestor/sibling modules",
            );
        }
        diagnostic.into_report()
    })
}

/// Lowers a HIR expression once, then selects a compact VM location when equivalent.
/// Failure always emits an unavailable location, ending any previous location for the variable.
#[cfg(test)]
pub(super) fn resolve_debug_var_location(
    expression: &midenc_hir::dialects::debuginfo::attributes::Expression,
    stack_position: Option<u8>,
    frame_size: Option<u32>,
    resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> masm::DebugVarLocation {
    resolve_debug_var_location_with_operand(
        expression,
        stack_position.map(masm::DebugLocationExpressionOp::ReadStack),
        frame_size,
        resolve_local_offset,
        resolve_global_address,
    )
}

/// Resolve an existing operand read directly, including an allocation-backed local cell.
pub(super) fn resolve_debug_var_location_with_operand(
    expression: &midenc_hir::dialects::debuginfo::attributes::Expression,
    operand: Option<masm::DebugLocationExpressionOp>,
    frame_size: Option<u32>,
    resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> masm::DebugVarLocation {
    use masm::{DebugLocationExpressionOp as Op, DebugVarLocation};
    let Some(expression) = lower_debug_location_expression_with_operand(
        expression,
        operand,
        frame_size,
        resolve_local_offset,
        resolve_global_address,
    ) else {
        return DebugVarLocation::Unavailable;
    };
    match expression.operations() {
        [Op::ReadLocal(offset)] => DebugVarLocation::Local(*offset),
        [Op::ReadMemory(address)] => DebugVarLocation::Memory(*address),
        [Op::ReadStack(position)] => DebugVarLocation::Stack(*position),
        [Op::ConstU64(value)] => miden_core::Felt::new(*value)
            .ok()
            .map(DebugVarLocation::Const)
            .unwrap_or(DebugVarLocation::Unavailable),
        [Op::ConstI64(value)] => u64::try_from(*value)
            .ok()
            .and_then(|value| miden_core::Felt::new(value).ok())
            .map(DebugVarLocation::Const)
            .unwrap_or(DebugVarLocation::Unavailable),
        [Op::FrameBaseAddress { base, byte_offset }, Op::DerefBytes] => {
            DebugVarLocation::ResolvedFrameBase {
                base: *base,
                byte_offset: *byte_offset,
            }
        }
        _ => DebugVarLocation::Expression(expression),
    }
}

/// Translates slot values and byte-address locations to the VM expression protocol.
/// A terminal VM dereference describes typed memory; a non-terminal one reads a pointer/value.
#[cfg(test)]
fn lower_debug_location_expression(
    expression: &midenc_hir::dialects::debuginfo::attributes::Expression,
    operand_stack_position: Option<u8>,
    frame_size: Option<u32>,
    resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> Option<masm::DebugLocationExpression> {
    lower_debug_location_expression_with_operand(
        expression,
        operand_stack_position.map(masm::DebugLocationExpressionOp::ReadStack),
        frame_size,
        resolve_local_offset,
        resolve_global_address,
    )
}

/// Translate a typed existing read, rather than manufacturing an operand stack position.
fn lower_debug_location_expression_with_operand(
    expression: &midenc_hir::dialects::debuginfo::attributes::Expression,
    operand: Option<masm::DebugLocationExpressionOp>,
    frame_size: Option<u32>,
    mut resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    mut resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> Option<masm::DebugLocationExpression> {
    use masm::{DebugLocationExpression, DebugLocationExpressionOp};
    use midenc_hir::dialects::debuginfo::attributes::ExpressionOp;

    let starts_with_source = expression.operations.first().is_some_and(|operation| {
        matches!(
            operation,
            ExpressionOp::LocalSlot(_)
                | ExpressionOp::OperandStackSlot(_)
                | ExpressionOp::FrameBase { .. }
                | ExpressionOp::Address { .. }
                | ExpressionOp::GlobalSlot(_)
        )
    });
    // Address results require a final typed memory read; scalar results do not.
    #[derive(Clone, Copy)]
    enum ValueKind {
        Value,
        ByteAddress,
    }

    let mut operations = Vec::with_capacity(expression.operations.len() + 1);
    let mut value_kinds = Vec::<ValueKind>::new();
    let mut operand_stack_slot = None;

    let mut depth = 0isize;
    let mut required = 0isize;
    for operation in &expression.operations {
        let (inputs, outputs) = match operation {
            ExpressionOp::Plus | ExpressionOp::Minus => (2, 1),
            ExpressionOp::Deref | ExpressionOp::PlusUConst(_) | ExpressionOp::StackValue => (1, 1),
            ExpressionOp::Piece(_)
            | ExpressionOp::BitPiece { .. }
            | ExpressionOp::Unsupported(_) => return None,
            _ => (0, 1),
        };
        required = required.max(inputs - depth);
        depth += outputs - inputs;
    }
    let needs_operand = expression.operations.is_empty() || required == 1;
    if required > 1 || (starts_with_source && required > 0) {
        return None;
    }
    if needs_operand {
        operations.push(operand?);
        value_kinds.push(ValueKind::Value);
    }

    for operation in &expression.operations {
        let target_operation = match operation {
            ExpressionOp::LocalSlot(index) => {
                let offset =
                    resolve_local_slot_offset(*index, frame_size, &mut resolve_local_offset)?;
                value_kinds.push(ValueKind::Value);
                Some(DebugLocationExpressionOp::ReadLocal(offset))
            }
            ExpressionOp::OperandStackSlot(index) => {
                if let Some(selected) = operand_stack_slot {
                    if selected != *index {
                        return None;
                    }
                } else {
                    operand_stack_slot = Some(*index);
                }
                value_kinds.push(ValueKind::Value);
                Some(operand?)
            }
            ExpressionOp::ConstU64(value) => {
                value_kinds.push(ValueKind::Value);
                Some(DebugLocationExpressionOp::ConstU64(*value))
            }
            ExpressionOp::ConstS64(value) => {
                value_kinds.push(ValueKind::Value);
                Some(DebugLocationExpressionOp::ConstI64(*value))
            }
            ExpressionOp::PlusUConst(value) if !value_kinds.is_empty() => {
                Some(DebugLocationExpressionOp::AddUnsigned(*value))
            }
            ExpressionOp::Plus if value_kinds.len() >= 2 => {
                let rhs = value_kinds.pop()?;
                let lhs = value_kinds.pop()?;
                let result = match (lhs, rhs) {
                    (ValueKind::Value, ValueKind::Value) => ValueKind::Value,
                    (ValueKind::ByteAddress, ValueKind::Value)
                    | (ValueKind::Value, ValueKind::ByteAddress) => ValueKind::ByteAddress,
                    (ValueKind::ByteAddress, ValueKind::ByteAddress) => return None,
                };
                value_kinds.push(result);
                Some(DebugLocationExpressionOp::Add)
            }
            ExpressionOp::Minus if value_kinds.len() >= 2 => {
                let rhs = value_kinds.pop()?;
                let lhs = value_kinds.pop()?;
                let result = match (lhs, rhs) {
                    (ValueKind::Value, ValueKind::Value)
                    | (ValueKind::ByteAddress, ValueKind::ByteAddress) => ValueKind::Value,
                    (ValueKind::ByteAddress, ValueKind::Value) => ValueKind::ByteAddress,
                    (ValueKind::Value, ValueKind::ByteAddress) => return None,
                };
                value_kinds.push(result);
                Some(DebugLocationExpressionOp::Sub)
            }
            ExpressionOp::Deref if !value_kinds.is_empty() => {
                Some(DebugLocationExpressionOp::DerefBytes)
            }
            ExpressionOp::StackValue if !value_kinds.is_empty() => {
                *value_kinds.last_mut()? = ValueKind::Value;
                // Keep the dereference non-terminal so the VM returns its scalar result.
                matches!(operations.last(), Some(DebugLocationExpressionOp::DerefBytes))
                    .then_some(DebugLocationExpressionOp::AddUnsigned(0))
            }
            ExpressionOp::FrameBase { base, byte_offset } => {
                let base = resolve_frame_base(
                    *base,
                    frame_size,
                    &mut resolve_local_offset,
                    &mut resolve_global_address,
                )?;
                value_kinds.push(ValueKind::ByteAddress);
                Some(DebugLocationExpressionOp::FrameBaseAddress {
                    base,
                    byte_offset: *byte_offset,
                })
            }
            ExpressionOp::Address { address } => {
                value_kinds.push(ValueKind::ByteAddress);
                Some(DebugLocationExpressionOp::ConstU64(*address))
            }
            ExpressionOp::GlobalSlot(name) => {
                let address = resolve_global_address(*name)?;
                value_kinds.push(ValueKind::Value);
                Some(DebugLocationExpressionOp::ReadMemory(address))
            }
            ExpressionOp::Piece(_)
            | ExpressionOp::BitPiece { .. }
            | ExpressionOp::Unsupported(_)
            | ExpressionOp::PlusUConst(_)
            | ExpressionOp::Plus
            | ExpressionOp::Minus
            | ExpressionOp::Deref
            | ExpressionOp::StackValue => return None,
        };
        operations.extend(target_operation);
    }

    // A location expression must produce exactly one result.
    let [result_kind] = value_kinds.as_slice() else {
        return None;
    };
    if matches!(result_kind, ValueKind::ByteAddress) {
        operations.push(DebugLocationExpressionOp::DerefBytes);
    }
    (!operations.is_empty())
        .then_some(operations)
        .and_then(|operations| DebugLocationExpression::new(operations).ok())
}

/// Resolves a logical local slot to a signed FMP-relative element offset.
fn resolve_local_slot_offset(
    index: u32,
    frame_size: Option<u32>,
    mut resolve_local_offset: impl FnMut(u32) -> Option<usize>,
) -> Option<i16> {
    let (frame_size, offset) = frame_size.zip(resolve_local_offset(index))?;
    i16::try_from(i64::try_from(offset).ok()? - i64::from(frame_size)).ok()
}

#[cfg(test)]
fn resolve_local_slot_location(
    index: u32,
    frame_size: Option<u32>,
    resolve_local_offset: impl FnMut(u32) -> Option<usize>,
) -> masm::DebugVarLocation {
    resolve_local_slot_offset(index, frame_size, resolve_local_offset)
        .map(masm::DebugVarLocation::Local)
        .unwrap_or(masm::DebugVarLocation::Unavailable)
}

/// Reads the element offset from the same frame layout used by executable local accesses.
#[cfg(test)]
fn resolve_local_slot_felt_offset(index: u32, frame: FrameLayout<'_>) -> Option<usize> {
    let offset = frame.element_offset(usize::try_from(index).ok()?)?;
    usize::try_from(offset).ok()
}

#[cfg(test)]
fn resolve_frame_base_location(
    base: midenc_hir::dialects::debuginfo::attributes::FrameBase,
    byte_offset: i64,
    frame_size: Option<u32>,
    resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> masm::DebugVarLocation {
    let Some(base) =
        resolve_frame_base(base, frame_size, resolve_local_offset, resolve_global_address)
    else {
        return masm::DebugVarLocation::Unavailable;
    };
    masm::DebugVarLocation::ResolvedFrameBase { base, byte_offset }
}

/// Resolves the slot holding the runtime byte-address frame base.
fn resolve_frame_base(
    base: midenc_hir::dialects::debuginfo::attributes::FrameBase,
    frame_size: Option<u32>,
    mut resolve_local_offset: impl FnMut(u32) -> Option<usize>,
    mut resolve_global_address: impl FnMut(midenc_hir::interner::Symbol) -> Option<u32>,
) -> Option<masm::DebugFrameBase> {
    use midenc_hir::dialects::debuginfo::attributes::FrameBase;

    match base {
        FrameBase::LocalSlot(index) => {
            let offset = resolve_local_slot_offset(index, frame_size, &mut resolve_local_offset)?;
            Some(masm::DebugFrameBase::Local(offset))
        }
        FrameBase::GlobalSlot(name) => {
            resolve_global_address(name).map(masm::DebugFrameBase::Memory)
        }
    }
}

/// Resolve an aligned source-global debug cell from the immutable linked layout.
pub(super) fn resolve_global_slot_address(
    name: midenc_hir::interner::Symbol,
    operation: &midenc_hir::Operation,
    link_info: &crate::linker::LinkInfo,
) -> Option<u32> {
    let current_module = operation.nearest_parent_op::<builtin::Module>()?;
    let symbol = current_module.borrow().get(name)?;
    let global = {
        let symbol = symbol.borrow();
        symbol
            .as_symbol_operation()
            .downcast_ref::<builtin::GlobalVariable>()?
            .as_global_var_ref()
    };
    link_info
        .globals_layout()
        .get_computed_addr(global)
        .and_then(debug_global_element_address)
}

/// Converts an aligned global storage cell to an element address without truncation.
/// Unaligned cells cannot be represented by the VM's ReadMemory operation.
fn debug_global_element_address(address: u32) -> Option<u32> {
    let pointer = crate::NativePtr::from_ptr(address);
    pointer.is_element_aligned().then_some(pointer.addr)
}

#[cfg(test)]
mod tests {
    use miden_core::Felt;
    use midenc_hir::{
        dialects::debuginfo::attributes::{Expression, ExpressionOp, FrameBase},
        interner::Symbol,
    };

    use super::{
        lower_debug_location_expression, resolve_frame_base_location, resolve_local_slot_location,
    };
    use crate::masm::{
        DebugFrameBase, DebugLocationExpression, DebugLocationExpressionOp, DebugVarLocation,
    };

    #[test]
    fn global_storage_addresses_preserve_element_coordinates() {
        assert_eq!(super::debug_global_element_address(32), Some(8));
        assert_eq!(super::debug_global_element_address(34), None);
        assert_eq!(super::debug_global_element_address(0x8000_0000), Some(0x2000_0000));
    }

    #[test]
    fn address_dereferences_preserve_each_indirection() {
        for source in [
            ExpressionOp::Address { address: 100 },
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(0),
                byte_offset: 0,
            },
        ] {
            for scalar in [false, true] {
                let mut operations = vec![source.clone(), ExpressionOp::Deref];
                if scalar {
                    operations.push(ExpressionOp::StackValue);
                }
                let expression = lower_debug_location_expression(
                    &Expression::with_ops(operations),
                    None,
                    Some(4),
                    |_| Some(0),
                    |_| None,
                )
                .unwrap();
                let value = miden_debug::resolve_variable_value(
                    &DebugVarLocation::Expression(expression),
                    &[],
                    |address| match address {
                        25 => Some(Felt::new(200).unwrap()),
                        50 => Some(Felt::new(37).unwrap()),
                        _ => None,
                    },
                    |_| Some(Felt::new(100).unwrap()),
                );
                assert_eq!(value, Some(Felt::new(if scalar { 200 } else { 37 }).unwrap()));
            }
        }
    }

    #[test]
    fn leading_constants_can_transform_an_implicit_operand() {
        for (operations, expected) in [
            (
                vec![ExpressionOp::ConstU64(4), ExpressionOp::Plus, ExpressionOp::StackValue],
                12,
            ),
            (vec![ExpressionOp::ConstU64(4), ExpressionOp::Minus, ExpressionOp::Deref], 27),
            (vec![ExpressionOp::ConstU64(4), ExpressionOp::StackValue], 4),
        ] {
            let expression = lower_debug_location_expression(
                &Expression::with_ops(operations),
                Some(0),
                Some(0),
                |_| None,
                |_| None,
            )
            .unwrap();
            assert_eq!(
                miden_debug::resolve_variable_value(
                    &DebugVarLocation::Expression(expression),
                    &[Felt::new(8).unwrap()],
                    |address| (address == 1).then_some(Felt::new(27).unwrap()),
                    |_| None,
                ),
                Some(Felt::new(expected).unwrap()),
            );
        }
    }

    #[test]
    fn compact_locations_and_unavailable_records_use_the_common_resolver() {
        use alloc::{collections::BTreeMap, rc::Rc};
        use core::cell::RefCell;

        use miden_debug::processor::trace::RowIndex;
        let global = Symbol::intern("global");
        for (operations, expected) in [
            (vec![], DebugVarLocation::Stack(0)),
            (vec![ExpressionOp::LocalSlot(0)], DebugVarLocation::Local(-4)),
            (vec![ExpressionOp::GlobalSlot(global)], DebugVarLocation::Memory(8)),
            (vec![ExpressionOp::ConstU64(7)], DebugVarLocation::Const(Felt::new(7).unwrap())),
            (vec![ExpressionOp::ConstS64(-1)], DebugVarLocation::Unavailable),
            (
                vec![ExpressionOp::GlobalSlot(Symbol::intern("missing"))],
                DebugVarLocation::Unavailable,
            ),
            (
                vec![ExpressionOp::FrameBase {
                    base: FrameBase::LocalSlot(0),
                    byte_offset: 3,
                }],
                DebugVarLocation::ResolvedFrameBase {
                    base: DebugFrameBase::Local(-4),
                    byte_offset: 3,
                },
            ),
        ] {
            let location = super::resolve_debug_var_location(
                &Expression::with_ops(operations),
                Some(0),
                Some(4),
                |_| Some(0),
                |name| (name == global).then_some(8),
            );
            assert_eq!(location, expected);
        }
        let location = super::resolve_debug_var_location(
            &Expression::with_ops(vec![ExpressionOp::GlobalSlot(global)]),
            Some(0),
            Some(4),
            |_| None,
            |_| None,
        );
        let mut tracker = miden_debug::DebugVarTracker::new(Rc::new(RefCell::new(BTreeMap::new())));
        tracker.record_events(
            RowIndex::from(1),
            vec![crate::masm::DebugVarInfo::new(
                "x",
                DebugVarLocation::Const(Felt::new(7).unwrap()),
            )],
        );
        tracker
            .record_events(RowIndex::from(2), vec![crate::masm::DebugVarInfo::new("x", location)]);
        tracker.update_to_cycle(RowIndex::from(1));
        assert!(tracker.get_variable("x").is_some());
        tracker.update_to_cycle(RowIndex::from(2));
        assert!(tracker.get_variable("x").is_none());
    }

    #[test]
    fn debug_local_offsets_use_the_shared_frame_layout() {
        let frame = super::FrameLayout::new(&[0, 2], 3);
        let resolve = |index| super::resolve_local_slot_felt_offset(index, frame);
        assert_eq!(resolve(1), Some(2));
        assert_eq!(resolve(2), None);
        assert_eq!(
            resolve_local_slot_location(1, Some(frame.aligned_size), resolve),
            DebugVarLocation::Local(2 - frame.aligned_size as i16)
        );
        assert_eq!(
            resolve_local_slot_location(2, Some(frame.aligned_size), resolve),
            DebugVarLocation::Unavailable
        );
        assert_eq!(
            resolve_frame_base_location(
                FrameBase::LocalSlot(1),
                0,
                Some(frame.aligned_size),
                resolve,
                |_| None,
            ),
            DebugVarLocation::ResolvedFrameBase {
                base: DebugFrameBase::Local(2 - frame.aligned_size as i16),
                byte_offset: 0,
            }
        );
    }

    #[test]
    fn final_frame_debug_offsets_use_final_size_and_never_wrap_signed_displacements() {
        use crate::plan::{FinalFrameLayout, FrameRef, SourceLocalLayout};
        for (size, offsets, expected) in [
            (20, vec![0, 4], vec![DebugVarLocation::Local(-20), DebugVarLocation::Local(-16)]),
            (
                65_532,
                vec![0, 65_531],
                vec![DebugVarLocation::Unavailable, DebugVarLocation::Local(-1)],
            ),
        ] {
            let finalized = FinalFrameLayout::new(
                SourceLocalLayout {
                    offsets,
                    extent: if size == 20 { 6 } else { size },
                },
                Default::default(),
                Default::default(),
                size,
            )
            .unwrap();
            let frame = super::FrameLayout::from(&finalized);
            for (index, expected) in expected.into_iter().enumerate() {
                let resolve = |index| super::resolve_local_slot_felt_offset(index, frame);
                assert_eq!(
                    resolve(index as u32),
                    Some(usize::from(
                        finalized.resolve(FrameRef::SourceLocal(index as u32)).unwrap()
                    ))
                );
                assert_eq!(
                    resolve_local_slot_location(index as u32, Some(frame.aligned_size), resolve),
                    expected
                );
            }
        }
    }

    #[test]
    fn frame_base_stack_values_return_the_address_without_dereferencing() {
        let expression = Expression::with_ops(vec![
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(0),
                byte_offset: 4,
            },
            ExpressionOp::StackValue,
        ]);
        let expression = lower_debug_location_expression(
            &expression,
            None,
            Some(4),
            |index| (index == 0).then_some(0),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            miden_debug::resolve_variable_value(
                &DebugVarLocation::Expression(expression),
                &[],
                |_| panic!("an address value must not read memory"),
                |offset| (offset == -4).then_some(Felt::new(100).unwrap()),
            ),
            Some(Felt::new(104).unwrap())
        );
    }

    #[test]
    fn local_slots_are_resolved_to_fmp_offsets() {
        assert_eq!(
            resolve_local_slot_location(2, Some(8), |index| Some(index as usize)),
            DebugVarLocation::Local(-6)
        );
        assert_eq!(
            resolve_local_slot_location(1, Some(8), |index| [0, 2].get(index as usize).copied()),
            DebugVarLocation::Local(-6)
        );
        assert_eq!(
            resolve_local_slot_location(i16::MAX as u32 + 1, Some(0), |index| {
                Some(index as usize)
            }),
            DebugVarLocation::Unavailable
        );
        assert_eq!(
            resolve_local_slot_location(0, None, |index| Some(index as usize)),
            DebugVarLocation::Unavailable
        );
        assert_eq!(
            resolve_local_slot_location(0, Some(65536), |_| Some(40000)),
            DebugVarLocation::Local(-25536)
        );
        assert_eq!(
            resolve_local_slot_location(0, Some(65536), |_| Some(0)),
            DebugVarLocation::Unavailable
        );
    }

    #[test]
    fn frame_base_slots_use_shared_miden_coordinates() {
        let local = resolve_frame_base_location(
            FrameBase::LocalSlot(2),
            28,
            Some(8),
            |index| Some(index as usize),
            |_| None,
        );
        assert_eq!(
            local,
            DebugVarLocation::ResolvedFrameBase {
                base: DebugFrameBase::Local(-6),
                byte_offset: 28,
            }
        );
        assert_eq!(
            miden_debug::resolve_variable_value(
                &local,
                &[],
                |address| (address == 32).then_some(Felt::new(13).unwrap()),
                |offset| (offset == -6).then_some(Felt::new(100).unwrap()),
            ),
            Some(Felt::new(13).unwrap())
        );

        let stack_pointer = Symbol::intern("__stack_pointer");
        let global = resolve_frame_base_location(
            FrameBase::GlobalSlot(stack_pointer),
            -4,
            Some(8),
            |index| Some(index as usize),
            |name| (name == stack_pointer).then_some(100),
        );
        assert_eq!(
            global,
            DebugVarLocation::ResolvedFrameBase {
                base: DebugFrameBase::Memory(100),
                byte_offset: -4,
            }
        );
        assert_eq!(
            miden_debug::resolve_variable_value(
                &global,
                &[],
                |address| match address {
                    100 => Some(Felt::new(20).unwrap()),
                    4 => Some(Felt::new(11).unwrap()),
                    _ => None,
                },
                |_| None,
            ),
            Some(Felt::new(11).unwrap())
        );
    }

    #[test]
    fn unrepresentable_frame_base_slots_are_unavailable() {
        for location in [
            resolve_frame_base_location(
                FrameBase::LocalSlot(0),
                0,
                None,
                |index| Some(index as usize),
                |_| None,
            ),
            resolve_frame_base_location(
                FrameBase::GlobalSlot(Symbol::intern("missing")),
                0,
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
        ] {
            assert_eq!(location, DebugVarLocation::Unavailable);
        }
    }

    #[test]
    fn compound_locations_lower_to_miden_expression_operations() {
        let local_pointer = Expression::with_ops(vec![
            ExpressionOp::LocalSlot(2),
            ExpressionOp::PlusUConst(8),
            ExpressionOp::Deref,
        ]);

        assert_eq!(
            lower_debug_location_expression(
                &local_pointer,
                None,
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
            Some(
                DebugLocationExpression::new(vec![
                    DebugLocationExpressionOp::ReadLocal(-6),
                    DebugLocationExpressionOp::AddUnsigned(8),
                    DebugLocationExpressionOp::DerefBytes,
                ])
                .unwrap(),
            )
        );

        let frame_address = Expression::with_ops(vec![
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(2),
                byte_offset: 4,
            },
            ExpressionOp::PlusUConst(8),
        ]);
        let frame_expression = lower_debug_location_expression(
            &frame_address,
            None,
            Some(8),
            |index| Some(index as usize),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            frame_expression,
            DebugLocationExpression::new(vec![
                DebugLocationExpressionOp::FrameBaseAddress {
                    base: DebugFrameBase::Local(-6),
                    byte_offset: 4,
                },
                DebugLocationExpressionOp::AddUnsigned(8),
                DebugLocationExpressionOp::DerefBytes,
            ])
            .unwrap()
        );
        assert_eq!(
            miden_debug::resolve_variable_value(
                &DebugVarLocation::Expression(frame_expression),
                &[],
                |address| (address == 27).then_some(Felt::new(13).unwrap()),
                |offset| (offset == -6).then_some(Felt::new(96).unwrap()),
            ),
            Some(Felt::new(13).unwrap())
        );

        let address_value = Expression::with_ops(vec![
            ExpressionOp::Address { address: 100 },
            ExpressionOp::StackValue,
        ]);
        assert_eq!(
            lower_debug_location_expression(
                &address_value,
                None,
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
            Some(
                DebugLocationExpression::new(vec![DebugLocationExpressionOp::ConstU64(100),])
                    .unwrap()
            )
        );
    }

    #[test]
    fn operand_transformations_capture_the_runtime_stack_position() {
        let expression = Expression::with_ops(vec![ExpressionOp::Deref]);

        assert_eq!(
            lower_debug_location_expression(
                &expression,
                Some(3),
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
            Some(
                DebugLocationExpression::new(vec![
                    DebugLocationExpressionOp::ReadStack(3),
                    DebugLocationExpressionOp::DerefBytes,
                ])
                .unwrap(),
            )
        );
        assert_eq!(
            lower_debug_location_expression(
                &expression,
                None,
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
            None
        );
    }

    #[test]
    fn dereferenced_stack_values_preserve_scalar_semantics() {
        let expression = Expression::with_ops(vec![
            ExpressionOp::Address { address: 128 },
            ExpressionOp::Deref,
            ExpressionOp::StackValue,
        ]);
        let runtime =
            lower_debug_location_expression(&expression, None, Some(8), |_| None, |_| None)
                .unwrap();
        assert_eq!(
            runtime.operations(),
            &[
                DebugLocationExpressionOp::ConstU64(128),
                DebugLocationExpressionOp::DerefBytes,
                DebugLocationExpressionOp::AddUnsigned(0),
            ]
        );
        assert_eq!(
            miden_debug::resolve_variable_value(
                &DebugVarLocation::Expression(runtime),
                &[],
                |address| (address == 32).then_some(Felt::from_u32(13)),
                |_| None,
            ),
            Some(Felt::from_u32(13))
        );
    }

    #[test]
    fn compound_frame_locations_support_unaligned_typed_memory_reads() {
        let expression = Expression::with_ops(vec![
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(0),
                byte_offset: 1,
            },
            ExpressionOp::PlusUConst(0),
        ]);
        let runtime =
            lower_debug_location_expression(&expression, None, Some(4), |_| Some(0), |_| None)
                .unwrap();
        assert_eq!(
            miden_debug::resolve_typed_variable_values(
                &DebugVarLocation::Expression(runtime),
                &midenc_hir::Type::U64,
                2,
                &[],
                |address| match address {
                    32 => Some(Felt::from_u32(0x33221100)),
                    33 => Some(Felt::from_u32(0x77665544)),
                    34 => Some(Felt::from_u32(0xbbaa9988)),
                    _ => None,
                },
                |offset| (offset == -4).then_some(Felt::from_u32(128)),
            ),
            Some(vec![Felt::from_u32(0x44332211), Felt::from_u32(0x88776655)])
        );
    }

    #[test]
    fn byte_addresses_lower_to_typed_memory_expressions() {
        let expression = Expression::with_ops(vec![ExpressionOp::Address { address: 129 }]);
        let runtime =
            lower_debug_location_expression(&expression, None, Some(4), |_| None, |_| None)
                .unwrap();
        assert_eq!(
            runtime.operations(),
            &[DebugLocationExpressionOp::ConstU64(129), DebugLocationExpressionOp::DerefBytes,]
        );
        assert_eq!(
            miden_debug::resolve_typed_variable_values(
                &DebugVarLocation::Expression(runtime),
                &midenc_hir::Type::U32,
                1,
                &[],
                |address| match address {
                    32 => Some(Felt::from_u32(0x33221100)),
                    33 => Some(Felt::from_u32(0x77665544)),
                    _ => None,
                },
                |_| None,
            ),
            Some(vec![Felt::from_u32(0x44332211)])
        );
    }

    #[test]
    fn compound_locations_reject_distinct_operand_stack_slots() {
        let expression = Expression::with_ops(vec![
            ExpressionOp::OperandStackSlot(0),
            ExpressionOp::OperandStackSlot(1),
            ExpressionOp::Plus,
        ]);

        assert_eq!(
            lower_debug_location_expression(
                &expression,
                Some(3),
                Some(8),
                |index| Some(index as usize),
                |_| None,
            ),
            None
        );
    }
}

pub(super) fn apply_debug_var_metadata(
    debug_var: &mut masm::DebugVarInfo,
    var: &midenc_hir::dialects::debuginfo::attributes::Variable,
    session: &midenc_session::Session,
) {
    use miden_assembly_syntax::debuginfo::SourceManagerExt;

    // Set arg_index if this is a parameter
    if let Some(arg_index) = var.arg_index {
        debug_var.set_arg_index(arg_index + 1); // Convert to 1-based
    }

    if let Some(ty) = var.ty.clone() {
        debug_var.set_ty(ty, None);
    }

    // Set source location
    if let Some(line) = core::num::NonZeroU32::new(var.line) {
        use miden_assembly::debuginfo::{ColumnNumber, LineNumber, Location, Uri};
        let uri = Uri::new(var.file.as_str());
        let line = LineNumber::new(line.get()).unwrap_or_default();
        let column = var.column.and_then(ColumnNumber::new).unwrap_or_default();
        let file = session.source_manager.get_by_uri(&uri);
        if let Some(file) = file {
            if let Some(span) = file.line_column_to_span(line, column) {
                let loc = Location::new(uri, span.start(), span.end());
                debug_var.set_location(loc);
            }
        } else if let Some(path) = uri.to_path()
            && let Some(file) = session.source_manager.load_file(&path).ok()
            && let Some(span) = file.line_column_to_span(line, column)
        {
            let loc = Location::new(uri, span.start(), span.end());
            debug_var.set_location(loc);
        }
    }
}

/// Source rules declare semantic inputs and effects without scheduling.
pub(crate) trait HirSelection: Op {
    /// Identify the closed source rule, retaining any immediate required to reconstruct it.
    fn selection_rule(&self) -> crate::select::SelectionRule;
}
macro_rules! final_selection {
    ($($ty:ty => $rule:ident),* $(,)?) => {$(
        impl HirSelection for $ty {
            fn selection_rule(&self) -> crate::select::SelectionRule { crate::select::SelectionRule::$rule }
        }
    )*};
}
final_selection! {
    builtin::Ret => Return, scf::If => If, scf::While => While,
    scf::IndexSwitch => Switch, scf::Yield => Yield, scf::Condition => Condition,
    ub::Unreachable => Trap, cf::CondBr => TerminalBranch,
    hir::PtrToInt => Alias, hir::IntToPtr => Alias, hir::Bitcast => Alias,
    arith::Split => Split, arith::Join => Join,
    hir::Exec => DirectCall, hir::Call => DirectCall, hir::Syscall => DirectCall,
    hir::Dyncall => Dyncall, hir::ExecIndirect => ExecIndirect, hir::ExecFpi => Fpi, hir::ProcedureRoot => ProcedureRoot,
    debuginfo::DebugValue => Debug, debuginfo::DebugDeclare => Debug, debuginfo::DebugKill => Debug,
}
impl HirSelection for arith::Constant {
    fn selection_rule(&self) -> crate::select::SelectionRule {
        crate::select::SelectionRule::Constant(*self.get_value())
    }
}
impl HirSelection for builtin::RetImm {
    fn selection_rule(&self) -> crate::select::SelectionRule {
        crate::select::SelectionRule::ReturnImmediate(*self.get_value())
    }
}

// Final ordinary definitions have no legacy scheduling callback.
macro_rules! arithmetic_selection {
    ($($ty:ty => $kind:ident),* $(,)?) => {$(
        impl HirSelection for $ty {
            fn selection_rule(&self) -> crate::select::SelectionRule {
                crate::select::SelectionRule::Arithmetic(super::units::arithmetic::Kind::$kind)
            }
        }
    )*};
}
arithmetic_selection! {
    arith::Add => Add,
    arith::AddOverflowing => AddOverflowing,
    arith::Sub => Sub,
    arith::SubOverflowing => SubOverflowing,
    arith::Mul => Mul,
    arith::MulOverflowing => MulOverflowing,
    arith::Exp => Exp,
    arith::Div => Div,
    arith::Mod => Mod,
    arith::Divmod => Divmod,
    arith::And => And,
    arith::Or => Or,
    arith::Xor => Xor,
    arith::Band => Band,
    arith::Bor => Bor,
    arith::Bxor => Bxor,
    arith::Shl => Shl,
    arith::Shr => Shr,
    arith::Rotl => Rotl,
    arith::Rotr => Rotr,
    arith::Eq => Eq,
    arith::Neq => Neq,
    arith::Gt => Gt,
    arith::Gte => Gte,
    arith::Lt => Lt,
    arith::Lte => Lte,
    arith::Min => Min,
    arith::Max => Max,
    arith::Trunc => Trunc,
    arith::Zext => Zext,
    arith::Sext => Sext,
    arith::Incr => Incr,
    arith::Neg => Neg,
    arith::Inv => Inv,
    arith::Ilog2 => Ilog2,
    arith::Pow2 => Pow2,
    arith::Not => Not,
    arith::Bnot => Bnot,
    arith::IsOdd => IsOdd,
    arith::Popcnt => Popcnt,
    arith::Clz => Clz,
    arith::Ctz => Ctz,
    arith::Clo => Clo,
    arith::Cto => Cto,
    arith::Ext2Add => Ext2Add,
    arith::Ext2Sub => Ext2Sub,
    arith::Ext2Mul => Ext2Mul,
    arith::Ext2Div => Ext2Div,
    arith::Ext2Neg => Ext2Neg,
    arith::Ext2Inv => Ext2Inv,
    hir::Cast => Cast,
    hir::Assert => Assert,
    hir::Assertz => Assertz,
    hir::AssertEq => AssertEq,
    hir::AssertU32 => AssertU32,
    cf::Select => Select,
    wasm::SignExtend => SignExtend,
    wasm::I32RemS => I32RemS,
    wasm::I64RemS => I64RemS,
    hir::ConstantPointer => ConstantPointer,
    builtin::GlobalSymbol => GlobalSymbol,
    ub::Poison => Poison,
}

// Final memory definitions retain their own immutable source authority.
macro_rules! memory_selection {
    ($($ty:ty => $kind:ident),* $(,)?) => {$(
        impl HirSelection for $ty {
            fn selection_rule(&self) -> crate::select::SelectionRule {
                crate::select::SelectionRule::Memory(super::units::memory::Kind::$kind)
            }
        }
    )*};
}
memory_selection! {
    hir::Load => Load,
    hir::Store => Store,
    hir::LoadLocal => LoadLocal,
    hir::StoreLocal => StoreLocal,
    hir::LocalAddress => LocalAddress,
    hir::MemGrow => MemGrow,
    hir::MemSize => MemSize,
    hir::MemSet => MemSet,
    hir::MemCpy => MemCpy,
    hir::MemMove => MemMove,
    wasm::I32Load8S => I32Load8S,
    wasm::I32Load16S => I32Load16S,
    wasm::I64Load8S => I64Load8S,
    wasm::I64Load16S => I64Load16S,
    wasm::I64Load32S => I64Load32S,
}

// Final fixed-stack definitions retain source-owned construction and exact tuple validation.
macro_rules! intrinsic_selection {
    ($($ty:ty => $kind:ident),* $(,)?) => {$(
        impl HirSelection for $ty {
            fn selection_rule(&self) -> crate::select::SelectionRule {
                crate::select::SelectionRule::Intrinsic(super::units::intrinsics::Kind::$kind)
            }
        }
    )*};
}
intrinsic_selection! {
    hir::Hash => Hash,
    hir::HMerge => HMerge,
    hir::MTreeGet => MTreeGet,
    hir::MTreeSet => MTreeSet,
    hir::MTreeMerge => MTreeMerge,
    hir::MTreeVerify => MTreeVerify,
    hir::HPerm => HPerm,
    hir::HornerBase => HornerBase,
    hir::HornerExt => HornerExt,
    hir::LogDeferred => LogDeferred,
    hir::FriExt2Fold4 => FriExt2Fold4,
    hir::MemStream => MemStream,
    hir::AdvicePipe => AdvicePipe,
    hir::CryptoStream => CryptoStream,
    hir::EvalCircuit => EvalCircuit,
    hir::SystemEvent => SystemEvent,
    hir::EmitEvent => EmitEvent,
    hir::EmitEventImm => EmitEventImm,
    hir::PrintLn => PrintLn,
    hir::AdvicePop => AdvicePop,
    hir::AdviceLoadWord => AdviceLoadWord,
    hir::Caller => Caller,
    hir::UnpackWord => UnpackWord,
    hir::Clk => Clk,
}
