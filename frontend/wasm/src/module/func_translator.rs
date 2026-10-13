//! Stand-alone WebAssembly to Miden IR translator.
//!
//! This module defines the `FuncTranslator` type which can translate a single WebAssembly
//! function to Miden IR guided by a `FuncEnvironment` which provides information about the
//! WebAssembly module and the runtime environment.
//!
//! Based on Cranelift's Wasm -> CLIF translator v11.0.0

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use cranelift_entity::EntityRef;
use midenc_hir::{
    BlockRef, Builder, Context, Op, Type,
    diagnostics::{ColumnNumber, LineNumber, SourceContent, Uri},
    dialects::{
        builtin::{BuiltinOpBuilder, FunctionRef},
        debuginfo::attributes::InlineCallFrame,
    },
};
use midenc_session::{
    Session,
    diagnostics::{DiagnosticsHandler, IntoDiagnostic, SourceManagerExt, SourceSpan},
};
use wasmparser::{FuncValidator, FunctionBody, WasmModuleResources};

use super::{
    debug_info::{FunctionDebugInfo, FunctionDeclaration},
    function_builder_ext::SSABuilderListener,
    module_env::ParsedModule,
    module_translation_state::ModuleTranslationState,
    types::ModuleTypesBuilder,
};
use crate::{
    code_translator::translate_operator,
    error::WasmResult,
    module::{
        func_translation_state::FuncTranslationState,
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt},
        module_env::DwarfReader,
        types::{convert_valtype, ir_type},
    },
    ssa::Variable,
};

/// WebAssembly to Miden IR function translator.
///
/// A `FuncTranslator` is used to translate a binary WebAssembly function into Miden IR guided
/// by a `FuncEnvironment` object. A single translator instance can be reused to translate multiple
/// functions which will reduce heap allocation traffic.
pub struct FuncTranslator {
    func_ctx: Rc<RefCell<FunctionBuilderContext>>,
    state: FuncTranslationState,
}

impl FuncTranslator {
    /// Create a new translator.
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            func_ctx: Rc::new(RefCell::new(FunctionBuilderContext::new(context))),
            state: FuncTranslationState::new(),
        }
    }

    /// Translate a binary WebAssembly function from a `FunctionBody`.
    #[allow(clippy::too_many_arguments)]
    pub fn translate_body(
        &mut self,
        body: &FunctionBody<'_>,
        // mod_func_builder: &mut FunctionBuilder<'_>,
        func: FunctionRef,
        module_state: &mut ModuleTranslationState,
        module: &ParsedModule<'_>,
        mod_types: &ModuleTypesBuilder,
        addr2line: &addr2line::Context<DwarfReader<'_>>,
        session: &Session,
        func_validator: &mut FuncValidator<impl WasmModuleResources>,
        config: &crate::WasmTranslationConfig,
        debug_info: Option<Rc<RefCell<FunctionDebugInfo>>>,
    ) -> WasmResult<()> {
        let context = func.borrow().as_operation().context_rc();
        let mut op_builder = midenc_hir::OpBuilder::new(context)
            .with_listener(SSABuilderListener::new(self.func_ctx.clone()));
        let mut builder = FunctionBuilderExt::new(func, &mut op_builder);

        // Keep a clone for FrameBase variable declaration below
        let debug_info_ref = debug_info.clone();

        if let Some(info) = debug_info.clone() {
            builder.set_debug_metadata(info);
        }

        self.state.set_debug_info(debug_info);

        let entry_block = builder.current_block();
        builder.seal_block(entry_block); // Declare all predecessors known.

        let num_params = declare_parameters(&mut builder, entry_block);

        // Set up the translation state with a single pushed control block representing the whole
        // function and its return values.
        let exit_block = builder.create_block();
        builder.append_block_params_for_function_returns(exit_block);
        {
            let signature = builder.signature();
            self.state.initialize(&signature, exit_block);
        }

        let mut reader = body.get_locals_reader().into_diagnostic()?;

        let total_wasm_vars = parse_local_decls(
            &mut reader,
            &mut builder,
            num_params,
            func_validator,
            &session.diagnostics,
        )?;

        // Declare extra SSA variables for FrameBase-only debug entries (e.g. local `sum`
        // in debug builds that lives in linear memory, not a WASM local).
        // Use declare_var_only to avoid allocating HIR locals that would inflate
        // num_locals and corrupt FMP offset calculations.
        if let Some(info) = debug_info_ref.as_ref() {
            let locals_len = info.borrow().locals.len();
            if locals_len > total_wasm_vars {
                for idx in total_wasm_vars..locals_len {
                    let var = Variable::new(idx);
                    builder.declare_var_only(var, Type::I32);
                }
            }
        }

        let mut reader = body.get_operators_reader().into_diagnostic()?;
        parse_function_body(
            &mut reader,
            &mut builder,
            &mut self.state,
            module_state,
            module,
            mod_types,
            addr2line,
            session,
            func_validator,
            config,
        )?;

        builder.finalize();
        Ok(())
    }
}

/// Declare local variables for the signature parameters that correspond to WebAssembly locals.
///
/// Return the number of local variables declared.
fn declare_parameters<B: ?Sized + Builder>(
    builder: &mut FunctionBuilderExt<'_, B>,
    entry_block: BlockRef,
) -> usize {
    use midenc_dialect_hir::HirOpBuilder;
    let sig_len = builder.signature().params().len();
    let mut next_local = 0;
    for i in 0..sig_len {
        let abi_param = builder.signature().params()[i].clone();
        let var = Variable::new(next_local);
        let local = builder.declare_local(var, abi_param.ty);
        next_local += 1;

        let param_value = entry_block.borrow().arguments()[i];
        builder.def_var(var, param_value);
        builder.register_parameter(var, param_value);
        builder.store_local(local, param_value, SourceSpan::SYNTHETIC).unwrap();
    }
    next_local
}

/// Parse the local variable declarations that precede the function body.
///
/// Declare local variables, starting from `num_params`.
/// Returns the total number of declared variables (params + locals).
fn parse_local_decls<B: ?Sized + Builder>(
    reader: &mut wasmparser::LocalsReader<'_>,
    builder: &mut FunctionBuilderExt<'_, B>,
    num_params: usize,
    validator: &mut FuncValidator<impl WasmModuleResources>,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<usize> {
    let mut next_local = num_params;
    let local_count = reader.get_count();

    for _ in 0..local_count {
        let pos = reader.original_position();
        let (count, ty) = reader.read().into_diagnostic()?;
        validator.define_locals(pos, count, ty).into_diagnostic()?;
        declare_locals(builder, count, ty, &mut next_local, diagnostics)?;
    }

    Ok(next_local)
}

/// Declare `count` local variables of the same type, starting from `next_local`.
///
/// Fail if too many locals are declared in the function, or if the type is not valid for a local.
fn declare_locals<B: ?Sized + Builder>(
    builder: &mut FunctionBuilderExt<'_, B>,
    count: u32,
    wasm_type: wasmparser::ValType,
    next_local: &mut usize,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    let ty = ir_type(convert_valtype(wasm_type), diagnostics)?;
    for _ in 0..count {
        let var = Variable::new(*next_local);
        let _local = builder.declare_local(var, ty.clone());
        *next_local += 1;
    }
    Ok(())
}

/// Parse the function body in `reader`.
///
/// This assumes that the local variable declarations have already been parsed and function
/// arguments and locals are declared in the builder.
#[allow(clippy::too_many_arguments)]
fn parse_function_body<B: ?Sized + Builder>(
    reader: &mut wasmparser::OperatorsReader<'_>,
    builder: &mut FunctionBuilderExt<'_, B>,
    state: &mut FuncTranslationState,
    module_state: &mut ModuleTranslationState,
    module: &ParsedModule<'_>,
    mod_types: &ModuleTypesBuilder,
    addr2line: &addr2line::Context<DwarfReader<'_>>,
    session: &Session,
    func_validator: &mut FuncValidator<impl WasmModuleResources>,
    config: &crate::WasmTranslationConfig,
) -> WasmResult<()> {
    // The control stack is initialized with a single block representing the whole function.
    debug_assert_eq!(state.control_stack.len(), 1, "State not initialized");

    let func_name = builder.name();
    let mut end_span = SourceSpan::SYNTHETIC;
    // Track the last valid span to use as a fallback for instructions without DWARF debug info.
    let mut last_valid_span = SourceSpan::UNKNOWN;
    while !reader.eof() {
        let pos = reader.original_position();
        let (op, offset) = reader.read_with_offset().into_diagnostic()?;
        func_validator.op(pos, &op).into_diagnostic()?;

        let dwarf_offset = module.wasm_file.dwarf_offset(offset);
        let resolved = resolve_instruction_debug_context(addr2line, dwarf_offset, session, config)?;
        let span = resolved.span;
        builder.set_inline_calls(resolved.inline_calls);
        if !span.is_unknown() {
            last_valid_span = span;
        } else {
            log::debug!(target: "module-parser",
                "failed to locate span for instruction at offset {offset} in function {func_name}"
            );
        }

        let effective_span = if span.is_unknown() {
            if !last_valid_span.is_unknown() {
                log::debug!(target: "module-parser",
                    "using last valid span as fallback for {:?} at offset {offset} in function {func_name}", op
                );
                last_valid_span
            } else {
                SourceSpan::SYNTHETIC
            }
        } else {
            span
        };
        builder.record_debug_span(effective_span);

        if state.reachable && !builder.is_unreachable() {
            builder.apply_location_schedule(dwarf_offset, effective_span, &state.stack);
        }

        // Track the span of every END we observe, so we have a span to assign to the return we
        // place in the final exit block
        if let wasmparser::Operator::End = op {
            end_span = effective_span;
        }

        translate_operator(
            &op,
            builder,
            state,
            module_state,
            &module.module,
            mod_types,
            &session.diagnostics,
            effective_span,
        )?;
    }

    // The final `End` operator left us in the exit block where we need to manually add a return
    // instruction.
    //
    // If the exit block is unreachable, it may not have the correct arguments, so we would
    // generate a return instruction that doesn't match the signature.
    if state.reachable && !builder.is_unreachable() {
        builder.set_inline_calls(Vec::new());
        builder.ret(state.stack.first().cloned(), end_span)?;
    }

    // Discard any remaining values on the stack. Either we just returned them,
    // or the end of the function is unreachable.
    state.stack.clear();

    Ok(())
}

#[derive(Default)]
struct FrameLocation {
    path: PathBuf,
    line: u32,
    column: u32,
}

struct ResolvedInstructionDebugContext {
    span: SourceSpan,
    inline_calls: Vec<InlineCallFrame>,
}

struct ResolvedFrame {
    name: String,
    linkage_name: Option<String>,
    declaration: FunctionDeclaration,
    location: FrameLocation,
}

fn resolve_instruction_debug_context(
    addr2line: &addr2line::Context<DwarfReader<'_>>,
    offset: u64,
    session: &Session,
    config: &crate::WasmTranslationConfig,
) -> WasmResult<ResolvedInstructionDebugContext> {
    let mut frames = addr2line.find_frames(offset).skip_all_loads().into_diagnostic()?;
    let unit = addr2line.find_dwarf_and_unit(offset).skip_all_loads();
    let mut resolved_frames = Vec::new();
    let mut span = SourceSpan::UNKNOWN;

    while let Some(frame) = frames.next().into_diagnostic()? {
        // Source availability affects instruction spans, never frame identity or adjacency.
        if let Some(location) = &frame.location {
            let resolved_span = resolve_source_span(location, session, config);
            if span.is_unknown() {
                span = resolved_span;
            }
        }
        // addr2line also returns location-only records when no function DIE is available.
        if frame.dw_die_offset.is_none() {
            continue;
        }
        let linkage_name = frame
            .function
            .as_ref()
            .and_then(|function| function.raw_name().ok())
            .map(|name| name.into_owned());
        let name = frame
            .function
            .as_ref()
            .and_then(|function| function.demangle().ok())
            .map(|name| name.into_owned())
            .or_else(|| linkage_name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        let location = frame
            .location
            .map(|location| FrameLocation {
                path: location
                    .file
                    .map(|file| remap_source_path(Path::new(file), config))
                    .unwrap_or_default(),
                line: location.line.unwrap_or_default(),
                column: location.column.unwrap_or_default(),
            })
            .unwrap_or_default();
        let mut declaration = match (unit, frame.dw_die_offset) {
            (Some(unit), Some(offset)) => {
                FunctionDeclaration::resolve(unit, offset).into_diagnostic()?
            }
            _ => FunctionDeclaration::default(),
        };
        declaration.file = declaration.file.map(|file| {
            remap_source_path(Path::new(file.as_str()), config)
                .to_string_lossy()
                .into_owned()
                .into()
        });
        resolved_frames.push(ResolvedFrame {
            name,
            linkage_name,
            declaration,
            location,
        });
    }

    let inline_calls = inline_call_chain(&resolved_frames);

    Ok(ResolvedInstructionDebugContext { span, inline_calls })
}

fn inline_call_chain(resolved_frames: &[ResolvedFrame]) -> Vec<InlineCallFrame> {
    resolved_frames
        .windows(2)
        .map(|frames| {
            let callee = &frames[0];
            let caller = &frames[1];
            InlineCallFrame {
                name: callee.name.clone().into(),
                linkage_name: callee.linkage_name.clone().map(Into::into),
                file: callee.declaration.file.unwrap_or(midenc_hir::interner::symbols::Empty),
                line: callee.declaration.line.unwrap_or_default(),
                column: callee.declaration.column.unwrap_or_default(),
                call_file: caller.location.path.to_string_lossy().into_owned().into(),
                call_line: caller.location.line,
                call_column: caller.location.column,
            }
        })
        .collect()
}

fn resolve_source_span(
    loc: &addr2line::Location<'_>,
    session: &Session,
    config: &crate::WasmTranslationConfig,
) -> SourceSpan {
    let Some(file) = loc.file else {
        return SourceSpan::UNKNOWN;
    };

    let path = Path::new(file);
    let Some(absolute_path) = resolve_source_path(path, session, config) else {
        log::debug!(target: "module-parser", "failed to resolve source path '{file}'");
        return SourceSpan::UNKNOWN;
    };

    debug_assert!(
        absolute_path.is_absolute(),
        "resolved path should be absolute: {}",
        absolute_path.display()
    );
    log::debug!(target: "module-parser",
        "resolved source path '{}' -> '{}'",
        file,
        absolute_path.display()
    );

    let source_file = match session.source_manager.load_file(&absolute_path) {
        Ok(source) => source,
        Err(error) => {
            log::debug!(target: "module-parser", "failed to load source '{file}': {error}");
            return SourceSpan::UNKNOWN;
        }
    };

    let path = remap_source_path(path, config);
    let remapped_uri = Uri::from(path.as_path());
    let register_remapped_source = source_file.uri() != &remapped_uri
        && session
            .source_manager
            .get_by_uri(&remapped_uri)
            .is_none_or(|existing| existing.as_str() != source_file.as_str());
    if register_remapped_source {
        let mut content = SourceContent::new(
            source_file.content().language(),
            remapped_uri.clone(),
            source_file.as_str(),
        );
        content.set_version(source_file.content().version());
        session.source_manager.load_from_raw_parts(remapped_uri, content);
    }

    // Register available source even when this particular location has no line.
    // Other frames may refer to valid positions in the same file.
    let Some(line) = LineNumber::new(loc.line.unwrap_or_default()) else {
        return SourceSpan::UNKNOWN;
    };
    let column = ColumnNumber::new(loc.column.unwrap_or_default()).unwrap_or_default();
    source_file.line_column_to_span(line, column).unwrap_or(SourceSpan::UNKNOWN)
}

/// Remap metadata paths without requiring the source file to be installed locally.
fn remap_source_path(path: &Path, config: &crate::WasmTranslationConfig) -> PathBuf {
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    config
        .remap_path_prefixes
        .iter()
        .filter_map(|prefix| {
            path.strip_prefix(prefix.source_prefix()).ok().map(|suffix| {
                match prefix.to.as_deref() {
                    Some(parent) => parent.join(suffix),
                    None => suffix.to_path_buf(),
                }
            })
        })
        .max_by_key(|path| path.components().count())
        .unwrap_or_else(|| path.to_path_buf())
}

fn resolve_source_path(
    path: &Path,
    session: &Session,
    config: &crate::WasmTranslationConfig,
) -> Option<PathBuf> {
    if path.is_relative() {
        // Strategy 1: Try remap_path_prefixes.
        if let Some(resolved) = config.remap_path_prefixes.iter().find_map(|prefix| {
            let candidate = prefix.source_prefix().join(path).canonicalize().ok();
            if candidate.as_ref().is_some_and(|candidate| candidate.exists()) {
                candidate
            } else {
                None
            }
        }) {
            return Some(resolved);
        }

        // Strategy 2: Try session.options.current_dir as fallback.
        let current_dir_candidate = session.options.current_dir.join(path).canonicalize().ok();
        if current_dir_candidate.as_ref().is_some_and(|candidate| candidate.exists()) {
            current_dir_candidate
        } else {
            None
        }
    } else if path.exists() {
        path.canonicalize().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
