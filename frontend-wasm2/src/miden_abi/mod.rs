pub(crate) mod stdlib;
pub(crate) mod transform;
pub(crate) mod tx_kernel;

use std::{cell::RefCell, rc::Rc};

use midenc_dialect_hir::InstBuilder;
use midenc_hir::{
    dialects::builtin::{Function, ModuleBuilder, WorldBuilder},
    interner::Symbol,
    AbiParam, FunctionIdent, FunctionType, FxHashMap, Ident, Op, Signature, ValueRef,
};
use transform::transform_miden_abi_call;
use tx_kernel::note;

use crate::{
    intrinsics,
    module::{
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
        module_translation_state::CallableFunction,
    },
};

pub(crate) type FunctionTypeMap = FxHashMap<&'static str, FunctionType>;
pub(crate) type ModuleFunctionTypeMap = FxHashMap<&'static str, FunctionTypeMap>;

pub fn is_miden_abi_module(module_id: Symbol) -> bool {
    is_miden_stdlib_module(module_id) || is_miden_sdk_module(module_id)
}

pub fn miden_abi_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    if is_miden_stdlib_module(module_id) {
        miden_stdlib_function_type(module_id, function_id)
    } else {
        miden_sdk_function_type(module_id, function_id)
    }
}

fn is_miden_sdk_module(module_id: Symbol) -> bool {
    tx_kernel::signatures().contains_key(module_id.as_str())
}

/// Get the target Miden ABI tx kernel function type for the given module and function id
pub fn miden_sdk_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    let funcs = tx_kernel::signatures()
        .get(module_id.as_str())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {}", module_id));
    funcs.get(function_id.as_str()).cloned().unwrap_or_else(|| {
        panic!(
            "No Miden ABI function type found for function {} in module {}",
            function_id, module_id
        )
    })
}

fn is_miden_stdlib_module(module_id: Symbol) -> bool {
    stdlib::signatures().contains_key(module_id.as_str())
}

/// Get the target Miden ABI stdlib function type for the given module and function id
#[inline(always)]
fn miden_stdlib_function_type(module_id: Symbol, function_id: Symbol) -> FunctionType {
    let funcs = stdlib::signatures()
        .get(module_id.as_str())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {}", module_id));
    funcs.get(function_id.as_str()).cloned().unwrap_or_else(|| {
        panic!(
            "No Miden ABI function type found for function {} in module {}",
            function_id, module_id
        )
    })
}

/// Restore module and function names of the intrinsics and Miden SDK functions
/// that were renamed to satisfy the Wasm Component Model requirements.
///
/// Returns the pre-renamed (expected at the linking stage) module and function
/// names or given `wasm_module_id` and `wasm_function_id` ids if the function
/// is not an intrinsic or Miden SDK function
pub fn recover_imported_masm_function_id(
    wasm_module_id: &str,
    wasm_function_id: &str,
) -> FunctionIdent {
    let module_id = recover_imported_masm_module(wasm_module_id.to_string());
    // Since `hash-1to1` is an invalid name in Wasm CM (dashed part cannot start with a digit),
    // we need to translate the CM name to the one that is expected at the linking stage
    let function_id = if wasm_function_id == "hash-one-to-one" {
        "hash_1to1".to_string()
    } else if wasm_function_id == "hash-two-to-one" {
        "hash_2to1".to_string()
    } else {
        wasm_function_id.replace("-", "_")
    };
    FunctionIdent {
        module: Ident::from(module_id),
        function: Ident::from(function_id.as_str()),
    }
}

/// Restore module names of the intrinsics and Miden SDK
/// that were renamed to satisfy the Wasm Component Model requirements.
///
/// Returns the pre-renamed (expected at the linking stage) module name
/// or given `wasm_module_id` if the module is not an intrinsic or Miden SDK module
pub fn recover_imported_masm_module(wasm_module_id: String) -> Symbol {
    let module_id = if wasm_module_id.starts_with("miden:core-import/intrinsics-mem") {
        intrinsics::mem::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/intrinsics-felt") {
        intrinsics::felt::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/account") {
        tx_kernel::account::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/note") {
        note::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/tx") {
        tx_kernel::tx::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-mem") {
        stdlib::mem::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-dsa-rpo-falcon") {
        stdlib::crypto::dsa::rpo_falcon::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-hashes-blake3") {
        stdlib::crypto::hashes::blake3::MODULE_ID
    } else if wasm_module_id.starts_with("miden:core-import") {
        panic!("unrecovered intrinsics or Miden SDK import module ID: {wasm_module_id}")
    } else {
        // Unrecognized module ID, return as is
        return wasm_module_id.into();
    };
    module_id.into()
}

/// Define a synthetic wrapper functon transforming parameters, calling the Miden ABI function
/// (think written in MASM) and transforming result
pub fn define_func_for_miden_abi_transformation(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    synth_func_id: FunctionIdent,
    synth_func_sig: Signature,
    import_func_id: FunctionIdent,
) -> CallableFunction {
    let import_ft = miden_abi_function_type(
        import_func_id.module.as_symbol(),
        import_func_id.function.as_symbol(),
    );
    let import_sig = Signature::new(
        import_ft.params.into_iter().map(AbiParam::new),
        import_ft.results.into_iter().map(AbiParam::new),
    );
    let mut func_ref = module_builder
        .define_function(synth_func_id.function, synth_func_sig.clone())
        .expect("failed to create an import function");
    let mut func = func_ref.borrow_mut();
    let span = func.name().span;
    let context = func.as_operation().context_rc();
    let func = func.as_mut().downcast_mut::<Function>().unwrap();
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir2::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut func_builder = FunctionBuilderExt::new(func, &mut op_builder);
    let entry_block = func_builder.current_block();
    func_builder.seal_block(entry_block); // Declare all predecessors known.
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    let import_module_ref = if let Some(found_module_ref) =
        world_builder.find_module(import_func_id.module.as_symbol())
    {
        found_module_ref
    } else {
        world_builder
            .declare_module(import_func_id.module)
            .expect("failed to create a module for imports")
    };
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let import_func_ref = import_module_builder
        .define_function(import_func_id.function, import_sig.clone())
        .expect("failed to create an import function");
    let results = transform_miden_abi_call(
        import_func_ref,
        import_func_id,
        args.as_slice(),
        &mut func_builder,
    );

    let exit_block = func_builder.create_block();
    func_builder.append_block_params_for_function_returns(exit_block);
    func_builder.ins().br(exit_block, results, span);
    func_builder.seal_block(exit_block);
    func_builder.switch_to_block(exit_block);
    func_builder.ins().ret(None, span).expect("failed ret");

    CallableFunction {
        wasm_id: synth_func_id,
        function_ref: Some(func_ref),
        signature: synth_func_sig,
    }
}
