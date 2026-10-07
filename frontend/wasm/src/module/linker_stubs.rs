//! Generic lowering for Rust linker stubs to MASM procedure calls.
//! A linker stub is detected as a function whose body consists solely of a
//! single `unreachable` instruction (plus the implicit `end`). The stub
//! function name is expected to be a fully-qualified MASM function path like
//! `miden::native_account::add_asset` and is used to locate the MASM callee.
//!
//! A diverging body is not on its own enough. An ordinary Rust function LLVM reduced to a lone
//! `unreachable` looks exactly the same, and a name section spells it as a Rust path — `core::…`,
//! `alloc::…` — which parses as a MASM path just as well. So the name must also name something
//! the compiler could plausibly bind: a compiler intrinsic, the FPI executor, or a path rooted in
//! a namespace the `miden` prefix or some linked package owns (`names_a_linked_namespace`).

use alloc::rc::Rc;
use core::{cell::RefCell, str::FromStr};

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_frontend_wasm_metadata::FrontendMetadata;
use midenc_hir::{
    Op, SmallVec, SymbolPath, ValueRef, Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{BuiltinOpBuilder, FunctionRef, ModuleBuilder, attributes::Signature},
    interner::Symbol,
};
use midenc_hir_symbol::symbols;
use midenc_package_interface::{LoweredSignature, ReturnStrategy};
use wasmparser::{FunctionBody, Operator};

use crate::{
    WasmTranslationConfig,
    error::WasmResult,
    intrinsics::{
        Intrinsic, IntrinsicsConversionResult, attach_effects_to_function, convert_intrinsics_call,
        convert_module_context_stub_call, fpi,
    },
    miden_abi::{
        effects::known_effects,
        resolve::{convert_arguments, convert_results, resolve_stub},
        transform::{fpi_indirect_return_via_pointer, no_transform, return_via_pointer},
    },
    module::{
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
        module_translation_state::ModuleTranslationState,
    },
};

/// What a linker stub's name resolves to.
// A short-lived classification that is matched immediately after construction, so the size
// imbalance between the variants has no practical cost.
#[allow(clippy::large_enum_variant)]
enum Callee {
    /// A compiler intrinsic, lowered by `crate::intrinsics`.
    Intrinsic(Intrinsic),
    /// The raw FPI executor, which the compiler lowers itself rather than calling.
    FpiIndirect,
    /// A procedure exported by a linked package, with the shape the Miden ABI rule set derives.
    Package(LoweredSignature),
}

/// Returns true if the given Wasm function body consists only of an
/// `unreachable` operator (ignoring `end`/`nop`).
pub fn is_unreachable_stub(body: &FunctionBody<'_>) -> bool {
    let mut reader = match body.get_operators_reader() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut saw_unreachable = false;
    while !reader.eof() {
        let Ok((op, _)) = reader.read_with_offset() else {
            return false;
        };
        match op {
            Operator::Unreachable => {
                saw_unreachable = true;
            }
            Operator::End | Operator::Nop => {
                // ignore
            }
            _ => return false,
        }
    }
    saw_unreachable
}

/// Whether `path`'s root namespace is the root namespace of some linked package's module tree.
///
/// This is the test that separates a binding from an unrelated diverging function. A stub naming
/// a procedure of a user MASM package (spec §9.4) is rooted in that package's own namespace, so
/// it reaches [`resolve_stub`] — including when the procedure is missing, which is how the "does
/// not name a procedure exported by any linked package" diagnostic is produced for a stale
/// binding. A name rooted in a namespace no linked package owns is not a binding at all, and is
/// left alone: `core::ptr::drop_in_place` is a `FunctionIdent` too.
fn names_a_linked_namespace(path: &SymbolPath, config: &WasmTranslationConfig) -> bool {
    let Some(namespace) = path.namespace() else {
        return false;
    };
    let Some(linked) = config.linked_packages.as_deref() else {
        return false;
    };
    // A symbol path keeps a quoted MASM component as written (`"masm-dep"`), while a package's
    // namespaces come through `PathComponent::as_str`, which strips the quotes; compare the
    // identifier, not its spelling.
    let namespace = unquoted(namespace.as_str());
    linked.iter().any(|package| package.root_namespaces().contains(namespace))
}

/// `component` without the quotes a MASM path puts around an identifier that needs them.
fn unquoted(component: &str) -> &str {
    component
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(component)
}

/// Whether `path` is a name the frontend recognizes as a linker stub: a compiler intrinsic, the
/// FPI executor, or a path rooted in the `miden` namespace or in a namespace some linked package
/// owns.
///
/// These are the names [`maybe_lower_linker_stub`] lowers (or reports as unresolvable) when the
/// function's body is a lone `unreachable`. Because they are recognized by name, such a function
/// can be neither exported nor renamed; module symbol resolution asks this to reject both.
pub(crate) fn names_a_linker_stub(path: &SymbolPath, config: &WasmTranslationConfig) -> bool {
    Intrinsic::try_from(path).is_ok()
        || fpi::is_fpi_indirect(path)
        || path.namespace() == Some(symbols::Miden)
        || names_a_linked_namespace(path, config)
}

/// If `body` looks like a linker stub, lowers `function_ref` to a call to the MASM callee
/// derived from the function's source name, adapting the call to the shape the Miden ABI rule set derives
/// for the callee. Returns `true` if handled, `false` otherwise.
///
/// `frontend_metadata` holds the parsed core module's frontend metadata entries; they are
/// consulted by module-context stub intrinsics (note intrinsics).
///
/// `config` carries the interfaces of the packages this session linked, against which a stub
/// that is not an intrinsic is resolved — and whose namespaces decide, per
/// [`names_a_linked_namespace`], which diverging functions are treated as stubs at all.
pub fn maybe_lower_linker_stub(
    function_ref: FunctionRef,
    source_name: Symbol,
    body: &FunctionBody<'_>,
    module_state: &mut ModuleTranslationState,
    frontend_metadata: &[FrontendMetadata],
    config: &WasmTranslationConfig,
) -> WasmResult<bool> {
    if !is_unreachable_stub(body) {
        return Ok(false);
    }

    // Parse function source name as MASM function ident: "ns::...::func"
    let name_string = source_name.as_str().to_string();
    // Expect stub export names to be fully-qualified MASM paths already (e.g. "intrinsics::felt::add").
    let func_ident = match midenc_hir::FunctionIdent::from_str(&name_string) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    let import_path: SymbolPath = SymbolPath::from_masm_function_id(func_ident);
    let context = function_ref.borrow().as_operation().context_rc();
    let stub_signature = function_ref.borrow().get_signature().clone();

    // Classify the callee: an intrinsic, the raw FPI executor, or a package export. Anything
    // else is not a stub we know how to lower, and is left alone — see
    // `names_a_linked_namespace` for why the last test asks about namespaces rather than about
    // whether any package is linked at all.
    let callee = if let Ok(intr) = Intrinsic::try_from(&import_path) {
        Callee::Intrinsic(intr)
    } else if fpi::is_fpi_indirect(&import_path) {
        Callee::FpiIndirect
    } else if import_path.namespace() == Some(symbols::Miden)
        || names_a_linked_namespace(&import_path, config)
    {
        Callee::Package(resolve_stub(&import_path, &stub_signature, config)?)
    } else {
        return Ok(false);
    };

    // Build the function body for the stub and replace it with an exec to MASM
    let span = function_ref.borrow().name().span;
    let func_builder_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder = midenc_hir::OpBuilder::new(context.clone())
        .with_listener(SSABuilderListener::new(func_builder_ctx));
    let mut fb = FunctionBuilderExt::new(function_ref, &mut op_builder);

    // Entry/args
    let entry_block = fb.current_block();
    fb.seal_block(entry_block);
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    // Declare the MASM import callee in the world and exec it in the shape the callee expects
    let results: Vec<ValueRef> = match callee {
        Callee::Intrinsic(intr) => {
            // Dispatch on how the intrinsic is lowered
            let Some(conv) = intr.conversion_result() else {
                return Ok(false);
            };
            match conv {
                IntrinsicsConversionResult::FunctionType { effects, .. } => {
                    // Declare callee and call via convert_intrinsics_call with function_ref
                    let import_module_ref = module_state
                        .world_builder
                        .declare_module_tree(&import_path.without_leaf())
                        .wrap_err("failed to create module for intrinsics imports")?;
                    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
                    let mut intrinsic_func_ref = import_module_builder
                        .define_function(
                            import_path.name().into(),
                            Visibility::Public,
                            stub_signature,
                        )
                        .wrap_err("failed to create intrinsic function ref")?;
                    {
                        let mut intrinsic_func = intrinsic_func_ref.borrow_mut();
                        attach_effects_to_function(&mut intrinsic_func, effects.iter());
                    }
                    convert_intrinsics_call(intr, Some(intrinsic_func_ref), &args, &mut fb, span)?
                        .to_vec()
                }
                // Inline conversion of intrinsic operation
                IntrinsicsConversionResult::MidenVmOp => {
                    convert_intrinsics_call(intr, None, &args, &mut fb, span)?.to_vec()
                }
                // The stub body is synthesized from module-level context (frontend metadata)
                IntrinsicsConversionResult::ModuleContextStub => convert_module_context_stub_call(
                    intr,
                    function_ref,
                    &args,
                    frontend_metadata,
                    &mut fb,
                    span,
                )?,
            }
        }
        Callee::FpiIndirect => {
            let import_ft = fpi::signature();
            let import_sig = Signature::new(&context, import_ft.params, import_ft.results);
            let import_module_ref = module_state
                .world_builder
                .declare_module_tree(&import_path.without_leaf())
                .wrap_err("failed to create module for the FPI executor import")?;
            let import_func_ref = ModuleBuilder::new(import_module_ref)
                .define_function(import_path.name().into(), Visibility::Public, import_sig)
                .wrap_err("failed to create the FPI executor import")?;
            fpi_indirect_return_via_pointer(import_func_ref, &args, &mut fb)?
        }
        Callee::Package(lowered) => {
            let import_ft = lowered.import_signature();
            let import_sig = Signature::new(&context, import_ft.params, import_ft.results);
            let import_module_ref = module_state
                .world_builder
                .declare_module_tree(&import_path.without_leaf())
                .wrap_err("failed to create module for MASM imports")?;
            let mut import_func_ref = ModuleBuilder::new(import_module_ref)
                .define_function(import_path.name().into(), Visibility::Public, import_sig)
                .wrap_err("failed to create MASM import function ref")?;
            // A manifest cannot declare effects, and the compiler treats an export's effects as
            // unknown (spec §7), except for the core-library procedures whose effects it knows
            // itself; `known_effects` is empty for every other path.
            {
                let effects = known_effects(&import_path.to_library_path());
                let mut import_func = import_func_ref.borrow_mut();
                attach_effects_to_function(&mut import_func, effects.iter());
            }
            // The stub deals in Wasm carrier types and the import is declared with the callee's
            // own types, so both directions are converted at the boundary.
            let args = convert_arguments(&lowered, &args, &mut fb, span)?;
            match &lowered.ret {
                ReturnStrategy::OutPointer(_) => {
                    return_via_pointer(import_func_ref, &lowered, &args, &mut fb)?
                }
                ReturnStrategy::Void | ReturnStrategy::Direct(_) => {
                    let results = no_transform(import_func_ref, &args, &mut fb)?;
                    convert_results(&lowered, &results, &mut fb, span)?
                }
            }
        }
    };

    // Return
    let exit_block = fb.create_block();
    fb.append_block_params_for_function_returns(exit_block);
    fb.br(exit_block, results, span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let ret_vals: SmallVec<[ValueRef; 1]> = {
        let borrow = exit_block.borrow();
        borrow.argument_values().collect()
    };
    fb.ret(ret_vals, span)?;

    Ok(true)
}
