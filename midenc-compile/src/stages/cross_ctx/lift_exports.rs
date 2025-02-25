//! Lifting the exports for cross-context calls.

use alloc::collections::BTreeMap;

use miden_assembly::Spanned;
use midenc_hir2::{dialects::builtin::ComponentBuilder, Abi, CallConv, FunctionType};
use midenc_session::{diagnostics::Severity, DiagnosticsHandler};

use super::{
    super::*,
    flat::{assert_core_wasm_signature_equivalence, flatten_function_type, needs_transformation},
};
use crate::{stage::Stage, CompilerResult, LinkerInput};

/// Generates lifting for exports for the cross-context calls according to the Miden ABI.
///
/// For each component export ensure a module for each interface that has a lifting
/// function, i.e. a function that takes parameters according to the cross-context Miden
/// ABI, converts them to the Wasm CABI, calls the core Wasm module exported function,
/// converts the results to the cross-context Miden ABI
///
/// After this stage all exported functons are expected to be called using the Miden ABI for
/// cross-context calls, i.e. using the stack and the advice provider for arguments and results.
pub struct LiftExportsCrossCtxStage;

impl Stage for LiftExportsCrossCtxStage {
    type Input = LinkerInput;
    type Output = LinkerInput;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let component = if let LinkerInput::Hir(component) = input {
            component
        } else {
            return Ok(input);
        };

        // dbg!(&component.exports());
        // dbg!(&component.modules().keys());

        let mut component_builder = ComponentBuilder::load(*component, &session.diagnostics);

        let mut lifted_exports: BTreeMap<InterfaceFunctionIdent, ComponentExport> = BTreeMap::new();
        let exports = component_builder.exports().clone();
        for (id, export) in exports.into_iter() {
            if let Canonical = export.function_ty.abi() {
                // skip exports that are already lifted
                lifted_exports.insert(id, export);
                continue;
            }
            if let Some(entrypoint) = &session.options.entrypoint {
                // skip the entrypoint
                let export_core_func = format!(
                    "{}::{}",
                    export.function.module.as_str(),
                    export.function.function.as_str()
                );
                if entrypoint == &export_core_func {
                    lifted_exports.insert(id, export);
                    continue;
                }
            }
            let new_export = generate_lifting_function(
                &mut component_builder,
                id,
                export,
                &session.diagnostics,
            )?;
            lifted_exports.insert(id, new_export);
        }

        let component_builder = component_builder.with_exports(lifted_exports);

        let component = component_builder.build();

        // dbg!(&component.exports());
        // dbg!(&component.modules().keys());

        Ok(LinkerInput::Hir(component.into()))
    }
}

fn generate_lifting_function(
    component_builder: &mut ComponentBuilder,
    export_id: InterfaceFunctionIdent,
    export: ComponentExport,
    diagnostics: &DiagnosticsHandler,
) -> CompilerResult<ComponentExport> {
    let export_func_sig = component_builder
        .signature(&export.function)
        .ok_or({
            let message = format!(
                "Miden CCABI export lifting generation. Cannot find signature for exported \
                 function {}",
                export.function
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?
        .clone();

    // get or create the module for the interface
    let module_id = export_id.interface.full_name;
    let mut module_builder = component_builder.module(module_id);
    let cross_ctx_export_sig = flatten_function_type(&export.function_ty, CallConv::CanonLift)
        .map_err(|e| {
            let message = format!(
                "Miden CCABI export lifting generation. Signature for exported function {} \
                 requires flattening. Error: {}",
                export.function, e
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?;
    if needs_transformation(&cross_ctx_export_sig) {
        let message = format!(
            "Miden CCABI export lifting generation. Signature for exported function {} requires \
             lifting. This is not yet supported",
            export.function
        );
        return Err(diagnostics.diagnostic(Severity::Error).with_message(message).into_report());
    }
    assert_core_wasm_signature_equivalence(&export_func_sig, &cross_ctx_export_sig);

    let mut builder = module_builder.function(export_id.function, cross_ctx_export_sig.clone())?;
    let entry = builder.current_block();
    let params = builder.block_params(entry).to_vec();

    let dfg = builder.data_flow_graph_mut();
    // import the Wasm core function
    if dfg.get_import(&export.function).is_none() {
        dfg.import_function(
            export.function.module,
            export.function.function,
            export_func_sig.clone(),
        )
        .map_err(|_e| {
            let message = format!(
                "Miden CCABI export lifting generation. lifting function with name {} in module \
                 {} with signature {export_func_sig:?} is already imported (function call) with a \
                 different signature",
                export.function.function, export.function.module
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?;
    }
    let span = export.function.function.span();
    // NOTE: handle lifting/lowering for non-scalar types
    // see https://github.com/0xPolygonMiden/compiler/pull/357
    let call = builder.ins().exec(export.function, &params, span);
    // dbg!(&sig);
    let result = builder.first_result(call);
    builder.ins().ret(Some(result), span);
    let function_id = builder.build()?;
    module_builder.build()?;
    let component_export = ComponentExport {
        function: function_id,
        function_ty: FunctionType {
            abi: Abi::Canonical,
            ..export.function_ty
        },
        ..export
    };
    Ok(component_export)
}
