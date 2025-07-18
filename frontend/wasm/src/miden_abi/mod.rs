pub(crate) mod stdlib;
pub(crate) mod transform;
pub(crate) mod tx_kernel;

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{BuiltinOpBuilder, ModuleBuilder, WorldBuilder},
    interner::Symbol,
    AbiParam, FunctionType, FxHashMap, Op, Signature, SmallVec, SymbolNameComponent, SymbolPath,
    ValueRef,
};
use midenc_hir_symbol::symbols;
use transform::transform_miden_abi_call;

use crate::{
    callable::CallableFunction,
    intrinsics,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

pub(crate) type FunctionTypeMap = FxHashMap<Symbol, FunctionType>;
pub(crate) type ModuleFunctionTypeMap = FxHashMap<SymbolPath, FunctionTypeMap>;

pub fn is_miden_abi_module(path: &SymbolPath) -> bool {
    let module_path = path.without_leaf();
    is_miden_stdlib_module(&module_path) || is_miden_sdk_module(&module_path)
}

fn is_miden_sdk_module(module_path: &SymbolPath) -> bool {
    tx_kernel::signatures().contains_key(module_path)
}

fn is_miden_stdlib_module(module_path: &SymbolPath) -> bool {
    stdlib::signatures().contains_key(module_path)
}

pub fn miden_abi_function_type(path: &SymbolPath) -> FunctionType {
    const STD: &[SymbolNameComponent] =
        &[SymbolNameComponent::Root, SymbolNameComponent::Component(symbols::Std)];

    if path.is_prefixed_by(STD) {
        miden_stdlib_function_type(path)
    } else {
        miden_sdk_function_type(path)
    }
}

/// Get the target Miden ABI tx kernel function type for the given module and function id
pub fn miden_sdk_function_type(path: &SymbolPath) -> FunctionType {
    let module_path = path.without_leaf();
    let funcs = tx_kernel::signatures()
        .get(module_path.as_ref())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {module_path}"));
    funcs
        .get(&path.name())
        .cloned()
        .unwrap_or_else(|| panic!("No Miden ABI function type found for function {path}"))
}

/// Get the target Miden ABI stdlib function type for the given module and function id
fn miden_stdlib_function_type(path: &SymbolPath) -> FunctionType {
    let module_path = path.without_leaf();
    let funcs = stdlib::signatures()
        .get(module_path.as_ref())
        .unwrap_or_else(|| panic!("No Miden ABI function types found for module {module_path}"));
    funcs
        .get(&path.name())
        .cloned()
        .unwrap_or_else(|| panic!("No Miden ABI function type found for function {path}"))
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
) -> Option<SymbolPath> {
    match recover_imported_masm_module(wasm_module_id) {
        Ok(mut path) => {
            // Since `hash-1to1` is an invalid name in Wasm CM (dashed part cannot start with a digit),
            // we need to translate the CM name to the one that is expected at the linking stage
            let function_id = if wasm_function_id == "hash-one-to-one" {
                Symbol::from("hash_1to1")
            } else if wasm_function_id == "hash-two-to-one" {
                Symbol::from("hash_2to1")
            } else {
                Symbol::intern(wasm_function_id.replace("-", "_"))
            };

            path.set_name(function_id);

            Some(path)
        }
        Err(_unknown_module_id) => None,
    }
}

/// Restore module names of the intrinsics and Miden SDK
/// that were renamed to satisfy the Wasm Component Model requirements.
///
/// Returns the pre-renamed (expected at the linking stage) module name
/// or given `wasm_module_id` if the module is not an intrinsic or Miden SDK module
pub fn recover_imported_masm_module(wasm_module_id: &str) -> Result<SymbolPath, Symbol> {
    if wasm_module_id.starts_with("miden:core-intrinsics/intrinsics-mem") {
        Ok(SymbolPath::from_masm_module_id(intrinsics::mem::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-intrinsics/intrinsics-felt") {
        Ok(SymbolPath::from_masm_module_id(intrinsics::felt::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-intrinsics/intrinsics-debug") {
        Ok(SymbolPath::from_masm_module_id(intrinsics::debug::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-intrinsics/intrinsics-crypto") {
        Ok(SymbolPath::from_masm_module_id(intrinsics::crypto::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-base/account") {
        Ok(SymbolPath::from_masm_module_id(tx_kernel::account::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-base/note") {
        Ok(SymbolPath::from_masm_module_id(tx_kernel::note::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-base/tx") {
        Ok(SymbolPath::from_masm_module_id(tx_kernel::tx::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-mem") {
        Ok(SymbolPath::from_masm_module_id(stdlib::mem::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-dsa-rpo-falcon") {
        Ok(SymbolPath::from_masm_module_id(stdlib::crypto::dsa::rpo_falcon::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-hashes-blake3") {
        Ok(SymbolPath::from_masm_module_id(stdlib::crypto::hashes::blake3::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-import/stdlib-crypto-hashes-rpo") {
        Ok(SymbolPath::from_masm_module_id(stdlib::crypto::hashes::rpo::MODULE_ID))
    } else if wasm_module_id.starts_with("miden:core-import") {
        panic!("unrecovered intrinsics or Miden SDK import module ID: {wasm_module_id}")
    } else {
        // Unrecognized module ID, return as a `Symbol`
        Err(Symbol::intern(wasm_module_id))
    }
}

/// Define a synthetic wrapper functon transforming parameters, calling the Miden ABI function
/// (think written in MASM) and transforming result
pub fn define_func_for_miden_abi_transformation(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    synth_func_id: SymbolPath,
    synth_func_sig: Signature,
    import_path: SymbolPath,
) -> CallableFunction {
    let import_ft = miden_abi_function_type(&import_path);
    let import_sig = Signature::new(
        import_ft.params.into_iter().map(AbiParam::new),
        import_ft.results.into_iter().map(AbiParam::new),
    );
    let function_ref = module_builder
        .define_function(synth_func_id.name().into(), synth_func_sig.clone())
        .expect("failed to create an import function");
    let func = function_ref.borrow();
    let span = func.name().span;
    let context = func.as_operation().context_rc();
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    drop(func);
    let mut func_builder = FunctionBuilderExt::new(function_ref, &mut op_builder);
    let entry_block = func_builder.current_block();
    func_builder.seal_block(entry_block); // Declare all predecessors known.
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    let import_module_ref = world_builder
        .declare_module_tree(&import_path)
        .wrap_err("failed to create module for imports")
        .unwrap_or_else(|err| panic!("{err}"));
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let import_func_ref = import_module_builder
        .define_function(import_path.name().into(), import_sig.clone())
        .expect("failed to create an import function");
    let results =
        transform_miden_abi_call(import_func_ref, &import_path, args.as_slice(), &mut func_builder);

    let exit_block = func_builder.create_block();
    func_builder.append_block_params_for_function_returns(exit_block);
    func_builder.br(exit_block, results, span).expect("failed br");
    func_builder.seal_block(exit_block);
    func_builder.switch_to_block(exit_block);
    let arg_vals: SmallVec<[ValueRef; 1]> = {
        let borrow = exit_block.borrow();
        borrow.argument_values().collect()
    };
    func_builder.ret(arg_vals, span).expect("failed ret");

    CallableFunction::Function {
        wasm_id: synth_func_id,
        function_ref,
        signature: synth_func_sig,
    }
}
