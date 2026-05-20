//! Foreign procedure invocation lowering support.

use midenc_dialect_hir as hir;
use midenc_hir::{
    AddressSpace, CallConv, Felt, Immediate, Op, PointerType, SymbolNameComponent, SymbolPath,
    Type,
    dialects::builtin::{
        self,
        attributes::{Signature, TypeArrayAttr, U32ArrayAttr},
    },
    interner::{Symbol, symbols},
};
use midenc_session::diagnostics::{Report, Spanned};

use super::lowering::invocation_target_from_symbol_path;
use crate::{emit::OpEmitter, emitter::BlockEmitter, masm};

const EXECUTE_FOREIGN_PROCEDURE: &str = "execute_foreign_procedure";
const EXECUTE_FOREIGN_PROCEDURE_INDIRECT: &str = "execute_foreign_procedure_indirect";
const FPI_ABI_PREFIX_ARGS: usize = 6;
const FPI_EXEC_TOTAL_INPUTS: usize = 22;
const FPI_MAX_PADDED_ARGS: usize = 15;
const FPI_FLATTENED_ARG_OFFSETS_ATTR: &str = "fpi.flattened_arg_offsets";
const FPI_FLATTENED_ARG_TYPES_ATTR: &str = "fpi.flattened_arg_types";

/// Returns true for the direct protocol FPI executor path.
pub(super) fn is_execute_foreign_procedure_path(path: &SymbolPath) -> bool {
    let mut components = path.components().peekable();
    components.next_if_eq(&SymbolNameComponent::Root);

    matches!(
        (
            components.next().map(|component| component.as_symbol_name()),
            components.next().map(|component| component.as_symbol_name()),
            components.next().map(|component| component.as_symbol_name()),
            components.next_if(|component| component.is_leaf()).map(|component| component.as_symbol_name()),
            components.next(),
        ),
        (
            Some(symbols::Miden),
            Some(symbols::Protocol),
            Some(symbols::Tx),
            Some(function),
            None,
        ) if function.as_str() == EXECUTE_FOREIGN_PROCEDURE
    )
}

/// Returns true when a direct FPI call needs a scratch slot to pad its ABI inputs.
pub(super) fn requires_padding_scratch(op: &hir::Exec) -> bool {
    is_execute_foreign_procedure_path(op.callee().path())
        && op.arguments().len() == FPI_MAX_PADDED_ARGS + 1
}

/// Returns true for the compiler-internal indirect FPI executor path.
pub(super) fn is_execute_foreign_procedure_indirect_path(path: &SymbolPath) -> bool {
    let mut components = path.components().peekable();
    components.next_if_eq(&SymbolNameComponent::Root);

    matches!(
        (
            components.next().map(|component| component.as_symbol_name()),
            components.next().map(|component| component.as_symbol_name()),
            components.next().map(|component| component.as_symbol_name()),
            components.next_if(|component| component.is_leaf()).map(|component| component.as_symbol_name()),
            components.next(),
        ),
        (
            Some(symbols::Miden),
            Some(symbols::Protocol),
            Some(symbols::Tx),
            Some(function),
            None,
        ) if function.as_str() == EXECUTE_FOREIGN_PROCEDURE_INDIRECT
    )
}

/// Validates the direct FPI operand count before generic operand scheduling runs.
pub(super) fn validate_direct_operand_count(actual_arg_count: usize) -> Result<(), Report> {
    if actual_arg_count > FPI_MAX_PADDED_ARGS + 1 {
        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE}` lowering currently supports at most {} flattened \
             procedure input felts",
            FPI_MAX_PADDED_ARGS + 1 - FPI_ABI_PREFIX_ARGS
        )));
    }

    Ok(())
}

/// Returns the canonical ABI tuple layout attached to a compiler-internal indirect FPI call.
fn indirect_arg_layout(op: &hir::Exec) -> Result<Vec<(u32, Type)>, Report> {
    let offsets_attr = op
        .as_operation()
        .get_typed_attribute::<U32ArrayAttr>(FPI_FLATTENED_ARG_OFFSETS_ATTR);
    let types_attr = op
        .as_operation()
        .get_typed_attribute::<TypeArrayAttr>(FPI_FLATTENED_ARG_TYPES_ATTR);

    let (offsets_attr, types_attr) = match (offsets_attr, types_attr) {
        (Some(offsets_attr), Some(types_attr)) => (offsets_attr, types_attr),
        _ => {
            return Err(Report::msg(format!(
                "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` call must provide both \
                 `{FPI_FLATTENED_ARG_OFFSETS_ATTR}` and `{FPI_FLATTENED_ARG_TYPES_ATTR}`"
            )));
        }
    };

    let offsets_ref = offsets_attr.borrow();
    let offsets = offsets_ref.as_value();
    let types_ref = types_attr.borrow();
    let types = types_ref.as_value();
    if offsets.len() != types.len() {
        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` call has {} offsets and {} load types",
            offsets.len(),
            types.len()
        )));
    }

    Ok(offsets.iter().copied().zip(types.iter().cloned()).collect())
}

/// Validates the operand shape for a compiler-internal indirect FPI executor call.
fn validate_indirect_exec_operands(op: &hir::Exec) -> Result<(), Report> {
    let arg_types = op.arguments().iter().map(|arg| arg.borrow().ty()).collect::<Vec<_>>();
    validate_indirect_pointer_operand(&arg_types)
}

/// Validates that an indirect FPI executor call receives exactly one argument pointer.
fn validate_indirect_pointer_operand(arg_types: &[Type]) -> Result<(), Report> {
    if arg_types.len() != 1 {
        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` call expects exactly one canonical ABI \
             argument pointer operand, but received {} operands",
            arg_types.len()
        )));
    }

    let arg_ty = &arg_types[0];
    if arg_ty != &Type::I32 {
        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` call expects its canonical ABI argument \
             pointer operand to have type `i32`, but received `{arg_ty}`"
        )));
    }

    Ok(())
}

/// Validates the load layout for a compiler-internal indirect FPI executor call.
fn validate_indirect_arg_layout(arg_layout: &[(u32, Type)]) -> Result<(), Report> {
    let flattened_arg_count = arg_layout.len();
    if !(FPI_ABI_PREFIX_ARGS..=FPI_EXEC_TOTAL_INPUTS).contains(&flattened_arg_count) {
        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE}` indirect lowering received {flattened_arg_count} \
             flattened operands, but accepts between {FPI_ABI_PREFIX_ARGS} and \
             {FPI_EXEC_TOTAL_INPUTS}"
        )));
    }

    for (index, (byte_offset, load_ty)) in arg_layout.iter().enumerate() {
        validate_indirect_arg_load_type(index, load_ty)?;
        byte_offset_immediate(*byte_offset)?;
    }

    Ok(())
}

/// Validates one canonical ABI load type used by indirect FPI lowering.
fn validate_indirect_arg_load_type(index: usize, load_ty: &Type) -> Result<(), Report> {
    match load_ty {
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::Felt => Ok(()),
        other => Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` argument layout entry {index} uses \
             unsupported load type `{other}`; supported load types are i1, i8, u8, i16, u16, i32, \
             u32, and felt"
        ))),
    }
}

/// Returns the protocol executor path used after expanding an indirect FPI argument tuple.
fn execute_foreign_procedure_path() -> SymbolPath {
    SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(symbols::Miden),
        SymbolNameComponent::Component(symbols::Protocol),
        SymbolNameComponent::Component(symbols::Tx),
        SymbolNameComponent::Leaf(Symbol::intern(EXECUTE_FOREIGN_PROCEDURE)),
    ])
}

/// Appends zero padding for a direct FPI call so it matches the protocol ABI width.
pub(super) fn append_padding(
    emitter: &mut BlockEmitter<'_>,
    op: &hir::Exec,
    actual_arg_count: usize,
    span: midenc_hir::SourceSpan,
) -> Result<(), Report> {
    let padding = FPI_EXEC_TOTAL_INPUTS.checked_sub(actual_arg_count).ok_or_else(|| {
        Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE}` received {actual_arg_count} operands, but accepts at \
             most {FPI_EXEC_TOTAL_INPUTS}"
        ))
    })?;

    if padding == 0 {
        return Ok(());
    }

    if actual_arg_count > FPI_MAX_PADDED_ARGS {
        if actual_arg_count == FPI_MAX_PADDED_ARGS + 1 {
            return append_padding_with_scratch(emitter, op, padding, span);
        }

        return Err(Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE}` lowering currently supports at most {} flattened \
             procedure input felts when padding is required",
            FPI_MAX_PADDED_ARGS + 1 - FPI_ABI_PREFIX_ARGS
        )));
    }

    let mut inst_emitter = emitter.emitter();
    for _ in 0..padding {
        inst_emitter.literal(Immediate::Felt(Felt::ZERO), span);
        for _ in 0..actual_arg_count {
            inst_emitter.movup(actual_arg_count as u8, span);
        }
    }

    Ok(())
}

/// Pads a 16-operand direct FPI call by temporarily spilling the deepest operand.
fn append_padding_with_scratch(
    emitter: &mut BlockEmitter<'_>,
    op: &hir::Exec,
    padding: usize,
    span: midenc_hir::SourceSpan,
) -> Result<(), Report> {
    let scratch_slot = padding_scratch_slot(op)?;

    {
        let mut inst_emitter = emitter.emitter();
        inst_emitter.movup(FPI_MAX_PADDED_ARGS as u8, span);
        push_padding_scratch_addr(&mut inst_emitter, scratch_slot, span);
        inst_emitter.store(span);
    }

    {
        let mut inst_emitter = emitter.emitter();
        for _ in 0..padding {
            inst_emitter.literal(Immediate::Felt(Felt::ZERO), span);
            for _ in 0..FPI_MAX_PADDED_ARGS {
                inst_emitter.movup(FPI_MAX_PADDED_ARGS as u8, span);
            }
        }

        push_padding_scratch_addr(&mut inst_emitter, scratch_slot, span);
        inst_emitter.load(Type::Felt, span);
        inst_emitter.movdn(FPI_MAX_PADDED_ARGS as u8, span);
    }

    Ok(())
}

/// Returns the reserved absolute local offset used for direct FPI padding.
fn padding_scratch_slot(op: &hir::Exec) -> Result<u16, Report> {
    let mut parent = op.as_operation().parent_op();
    while let Some(parent_op) = parent {
        let parent_ref = parent_op.borrow();
        if let Some(function) = parent_ref.downcast_ref::<builtin::Function>() {
            let locals_required =
                function.locals().iter().map(|ty| ty.size_in_felts()).sum::<usize>();
            return u16::try_from(locals_required).map_err(|_| {
                Report::msg(format!(
                    "`{EXECUTE_FOREIGN_PROCEDURE}` lowering cannot address its FPI padding \
                     scratch slot"
                ))
            });
        }
        parent = parent_ref.parent_op();
    }

    Err(Report::msg(format!(
        "`{EXECUTE_FOREIGN_PROCEDURE}` lowering could not find an enclosing function for its FPI \
         padding scratch slot"
    )))
}

/// Pushes the reserved FPI padding scratch slot address onto the operand stack.
fn push_padding_scratch_addr(
    emitter: &mut OpEmitter<'_>,
    scratch_slot: u16,
    span: midenc_hir::SourceSpan,
) {
    emitter.emit(masm::Instruction::Locaddr(scratch_slot.into()), span);
    emitter.push(Type::from(PointerType::new_with_address_space(
        Type::Felt,
        AddressSpace::Element,
    )));
}

/// Converts one value loaded from an indirect FPI argument tuple to its protocol felt form.
fn canonicalize_indirect_arg_load(
    inst_emitter: &mut OpEmitter<'_>,
    load_ty: &Type,
    span: midenc_hir::SourceSpan,
) {
    match load_ty {
        Type::I8 | Type::I16 => {
            inst_emitter.sext(&Type::I32, span);
            inst_emitter.bitcast(&Type::Felt, span);
        }
        Type::Felt => {}
        _ => inst_emitter.bitcast(&Type::Felt, span),
    }
}

/// Returns the byte offset immediate used to load an indirect FPI argument.
fn byte_offset_immediate(byte_offset: u32) -> Result<Option<Immediate>, Report> {
    if byte_offset == 0 {
        return Ok(None);
    }

    let byte_offset = i32::try_from(byte_offset).map_err(|_| {
        Report::msg(format!(
            "`{EXECUTE_FOREIGN_PROCEDURE_INDIRECT}` argument layout contains byte offset \
             {byte_offset}, which does not fit in i32"
        ))
    })?;
    Ok(Some(Immediate::I32(byte_offset)))
}

/// Emits MASM for an FPI call whose canonical ABI lowered the argument list to one pointer.
pub(super) fn emit_indirect(op: &hir::Exec, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
    validate_indirect_exec_operands(op)?;
    let arg_layout = indirect_arg_layout(op)?;
    validate_indirect_arg_layout(&arg_layout)?;
    let flattened_arg_count = arg_layout.len();

    let span = op.span();
    let padding = FPI_EXEC_TOTAL_INPUTS - flattened_arg_count;
    let exec_path = execute_foreign_procedure_path();
    let callee = invocation_target_from_symbol_path(&exec_path, span);

    let mut inst_emitter = emitter.inst_emitter(op.as_operation());

    for _ in 0..padding {
        inst_emitter.literal(Immediate::Felt(Felt::ZERO), span);
        inst_emitter.swap(1, span);
    }

    // The Rust wrapper stores the account id as prefix then suffix, while the protocol executor
    // expects suffix then prefix. The rest of the flattened arguments are already in ABI order.
    let fpi_arg_order = core::iter::once(1)
        .chain(core::iter::once(0))
        .chain(2..flattened_arg_count)
        .collect::<Vec<_>>();
    for index in fpi_arg_order.into_iter().rev() {
        let (byte_offset, load_ty) = &arg_layout[index];
        let ptr_ty =
            Type::from(PointerType::new_with_address_space(load_ty.clone(), AddressSpace::Byte));
        inst_emitter.dup(0, span);
        if let Some(byte_offset) = byte_offset_immediate(*byte_offset)? {
            inst_emitter.add_imm(byte_offset, midenc_hir::Overflow::Wrapping, span);
        }
        inst_emitter.inttoptr(&ptr_ty, span);
        inst_emitter.load(load_ty.clone(), span);
        canonicalize_indirect_arg_load(&mut inst_emitter, load_ty, span);
        inst_emitter.swap(1, span);
    }
    OpEmitter::drop(&mut inst_emitter, span);

    let signature = Signature::with_convention(
        &inst_emitter.context_rc(),
        CallConv::Wasm,
        vec![Type::Felt; FPI_EXEC_TOTAL_INPUTS],
        vec![Type::Felt; FPI_EXEC_TOTAL_INPUTS - FPI_ABI_PREFIX_ARGS],
    );
    inst_emitter.exec(callee, &signature, span);

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, rc::Rc};

    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        Context, SourceSpan, TraceTarget, ValueRef,
        dialects::builtin::{self, BuiltinOpBuilder},
        formatter::PrettyPrint,
        pass::AnalysisManager,
        testing::Test,
        version::Version,
    };
    use midenc_hir_analysis::analyses::LivenessAnalysis;

    use super::*;
    use crate::{linker::LinkInfo, stack::OperandStack};

    #[test]
    fn indirect_fpi_arg_load_sign_extends_signed_narrow_values() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();

        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
            emitter.push(Type::I8);
            canonicalize_indirect_arg_load(&mut emitter, &Type::I8, SourceSpan::default());

            assert_eq!(emitter.stack_len(), 1);
            assert_eq!(emitter.stack()[0], Type::Felt);
        }

        assert!(
            !block.is_empty(),
            "signed narrow indirect FPI loads must emit sign-extension code"
        );
    }

    #[test]
    fn indirect_fpi_arg_load_bitcasts_unsigned_narrow_values_without_code() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();

        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
            emitter.push(Type::U8);
            canonicalize_indirect_arg_load(&mut emitter, &Type::U8, SourceSpan::default());

            assert_eq!(emitter.stack_len(), 1);
            assert_eq!(emitter.stack()[0], Type::Felt);
        }

        assert!(
            block.is_empty(),
            "unsigned narrow indirect FPI loads already carry the correct felt value"
        );
    }

    #[test]
    fn fpi_byte_offset_immediate_rejects_offsets_that_do_not_fit_i32() {
        let byte_offset = i32::MAX as u32 + 1;
        let err = byte_offset_immediate(byte_offset)
            .expect_err("oversized indirect FPI byte offsets must return a diagnostic");
        let message = err.to_string();

        assert!(message.contains("does not fit in i32"), "unexpected error: {message}");
    }

    #[test]
    fn indirect_fpi_pointer_operand_rejects_wrong_arity() {
        let err = validate_indirect_pointer_operand(&[])
            .expect_err("indirect FPI calls must receive one pointer operand");
        let message = err.to_string();

        assert!(message.contains("exactly one"), "unexpected error: {message}");
        assert!(message.contains("0 operands"), "unexpected error: {message}");
    }

    #[test]
    fn indirect_fpi_pointer_operand_rejects_non_i32_type() {
        let err = validate_indirect_pointer_operand(&[Type::Felt])
            .expect_err("indirect FPI pointers must be wasm i32 values");
        let message = err.to_string();

        assert!(message.contains("type `i32`"), "unexpected error: {message}");
        assert!(message.contains("felt"), "unexpected error: {message}");
    }

    #[test]
    fn indirect_fpi_arg_layout_rejects_unsupported_load_type() {
        let mut layout = vec![(0, Type::Felt); FPI_ABI_PREFIX_ARGS];
        layout[2] = (8, Type::U64);

        let err = validate_indirect_arg_layout(&layout)
            .expect_err("indirect FPI layout must reject multi-felt load types");
        let message = err.to_string();

        assert!(message.contains("unsupported load type `u64`"), "unexpected error: {message}");
    }

    #[test]
    fn direct_fpi_padding_emits_expected_movups_for_regular_widths() -> Result<(), Report> {
        for (actual_arg_count, expected_padding) in [(6, 16), (7, 15), (15, 7)] {
            let block = emit_direct_fpi_padding(actual_arg_count)?;
            let output = block.to_pretty_string();
            let movup = format!("movup.{actual_arg_count}");

            assert_eq!(
                output.matches(&movup).count(),
                actual_arg_count * expected_padding,
                "unexpected padding sequence for {actual_arg_count} operands:\n{output}"
            );
            assert_eq!(
                block.len(),
                expected_padding * (actual_arg_count + 1),
                "unexpected instruction count for {actual_arg_count} operands:\n{output}"
            );
            assert!(
                !output.contains("locaddr"),
                "regular direct FPI padding must not use the scratch slot:\n{output}"
            );
        }

        Ok(())
    }

    #[test]
    fn direct_fpi_padding_uses_scratch_for_sixteen_operands() -> Result<(), Report> {
        let block = emit_direct_fpi_padding(16)?;
        let output = block.to_pretty_string();

        assert_eq!(
            output.matches("movup.15").count(),
            91,
            "sixteen-operand padding must spill once and pad through movup.15:\n{output}"
        );
        assert_eq!(
            output.matches("locaddr.0").count(),
            2,
            "sixteen-operand padding must store and reload the scratch slot:\n{output}"
        );
        assert!(
            output.contains("exec.::intrinsics::mem::store_felt"),
            "sixteen-operand padding must spill the deepest operand:\n{output}"
        );
        assert!(
            output.contains("exec.::intrinsics::mem::load_felt"),
            "sixteen-operand padding must reload the deepest operand:\n{output}"
        );
        assert!(
            output.contains("movdn.15"),
            "sixteen-operand padding must restore the deepest operand:\n{output}"
        );

        Ok(())
    }

    #[test]
    fn direct_fpi_padding_rejects_seventeen_operands() {
        let err = emit_direct_fpi_padding(17)
            .expect_err("direct FPI padding must reject operands past the scratch-slot boundary");
        let message = err.to_string();

        assert!(
            message.contains("supports at most 10 flattened procedure input felts"),
            "unexpected error: {message}"
        );
    }

    fn emit_direct_fpi_padding(actual_arg_count: usize) -> Result<masm::Block, Report> {
        let params = vec![Type::Felt; actual_arg_count];
        let mut test = Test::new("direct_fpi_padding_test", &params, &[]);
        let function_ref = test.function();
        let span = function_ref.span();
        let exec = {
            let signature = function_ref.borrow().get_signature().clone();
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let args = {
                let entry = entry.borrow();
                entry.arguments().iter().copied().map(|arg| arg as ValueRef).collect::<Vec<_>>()
            };
            let exec = builder.exec(function_ref, signature, args, span)?;
            builder.ret(core::iter::empty::<ValueRef>(), span)?;
            exec
        };

        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;
        let link_info = LinkInfo::new(Some(builtin::ComponentId {
            namespace: "root".into(),
            name: "root".into(),
            version: Version::new(1, 0, 0),
        }));
        let mut invoked = BTreeSet::default();
        let mut stack = OperandStack::new(test.context_rc());
        for _ in 0..actual_arg_count {
            stack.push(Type::Felt);
        }

        let mut emitter = BlockEmitter {
            liveness: &liveness,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen"),
        };

        append_padding(&mut emitter, &exec.borrow(), actual_arg_count, span)?;
        Ok(emitter.into_emitted_block(span))
    }
}
