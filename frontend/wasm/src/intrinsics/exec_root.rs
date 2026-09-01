//! Stored-procedure dispatch intrinsics conversion module for WebAssembly to Miden IR.
//!
//! A guest that calls a procedure whose MAST root it reads from account storage declares one
//! extern stub per call signature under this module. The stub takes the MAST root as its first
//! four field-element parameters, then the flattened call arguments, and returns the flattened
//! result. The leaf name carries the signature the producer mangled into it and is opaque here:
//! every leaf under the module prefix converts the same way.
//!
//! The signature is therefore read from the stub itself rather than from a registry, which is
//! what the linker-stub path ([`IntrinsicsConversionResult::ModuleContextStub`]) provides.
//!
//! [`IntrinsicsConversionResult::ModuleContextStub`]: super::IntrinsicsConversionResult::ModuleContextStub

use midenc_dialect_hir::{ExecRoot, HirOpBuilder};
use midenc_hir::{
    Builder, Op, SourceSpan, SymbolNameComponent, Type, ValueRef,
    diagnostics::Report,
    dialects::builtin::{FunctionRef, attributes::Signature},
    interner::{Symbol, symbols},
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::ExecRoot),
];

/// The number of field elements addressable by MASM stack manipulation instructions.
const OPERAND_STACK_WINDOW_FELTS: usize = miden_core::program::MIN_STACK_DEPTH;

/// Synthesizes the body of a stored-procedure dispatch linker stub.
///
/// `stub_function_ref` is the stub function whose body is being synthesized; `args` are its
/// entry-block arguments and the returned values are its results.
pub(crate) fn convert_exec_root_intrinsics_stub<B: ?Sized + Builder>(
    function: Symbol,
    stub_function_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let (signature, context) = {
        let stub = stub_function_ref.borrow();
        (stub.get_signature().clone(), stub.as_operation().context_rc())
    };
    let stub_name = format!("intrinsics::exec_root::{function}");

    let params = signature.params();
    if params.len() < ExecRoot::ROOT_FELTS {
        return Err(Report::msg(format!(
            "invalid `{stub_name}` stub: expected at least {} parameter(s) for the procedure \
             root, but the stub declares {}",
            ExecRoot::ROOT_FELTS,
            params.len()
        )));
    }
    let (root_params, arg_params) = params.split_at(ExecRoot::ROOT_FELTS);

    for (index, param) in root_params.iter().enumerate() {
        if param.ty != Type::Felt {
            return Err(Report::msg(format!(
                "invalid `{stub_name}` stub: parameter {index} holds procedure root element \
                 {index} and must be a field element, but it has type '{}'",
                &param.ty
            )));
        }
    }

    for (index, param) in arg_params.iter().enumerate() {
        if param.ty.size_in_felts() != 1 {
            return Err(Report::msg(format!(
                "invalid `{stub_name}` stub: argument {index} has type '{}', which is {} field \
                 elements wide; stored-procedure arguments must be one field element each",
                &param.ty,
                param.ty.size_in_felts()
            )));
        }
    }

    // The lowering schedules the procedure root word on the operand stack on top of the
    // arguments, so together they must fit in the addressable operand stack window
    let arg_felts = arg_params.len();
    if ExecRoot::ROOT_FELTS + arg_felts > OPERAND_STACK_WINDOW_FELTS {
        return Err(Report::msg(format!(
            "invalid `{stub_name}` stub: {arg_felts} argument field elements plus the {} \
             procedure root field elements exceed Miden's {OPERAND_STACK_WINDOW_FELTS}-element \
             operand stack window",
            ExecRoot::ROOT_FELTS
        )));
    }

    let results = signature.results();
    if results.len() > 1 {
        return Err(Report::msg(format!(
            "invalid `{stub_name}` stub: expected at most one result, but the stub declares {}",
            results.len()
        )));
    }
    if let Some(result) = results.first()
        && result.ty.size_in_felts() != 1
    {
        return Err(Report::msg(format!(
            "invalid `{stub_name}` stub: the result has type '{}', which is {} field elements \
             wide; a stored-procedure result must be one field element",
            &result.ty,
            result.ty.size_in_felts()
        )));
    }

    if args.len() != params.len() {
        return Err(Report::msg(format!(
            "invalid `{stub_name}` stub: the stub declares {} parameter(s), but its body was \
             given {} argument(s)",
            params.len(),
            args.len()
        )));
    }
    let (root, call_args) = args.split_at(ExecRoot::ROOT_FELTS);

    // The dispatch contract covers the call arguments and results only: the root word names the
    // callee and is consumed by the dispatch itself.
    let dispatch_signature = Signature::with_convention(
        &context,
        signature.calling_convention(),
        arg_params.iter().map(|param| param.ty.clone()),
        results.iter().map(|result| result.ty.clone()),
    );

    let op = builder.exec_root(
        dispatch_signature,
        root.iter().copied(),
        call_args.iter().copied(),
        span,
    )?;

    let results: Vec<ValueRef> = {
        let borrow = op.borrow();
        borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
    };
    Ok(results)
}
