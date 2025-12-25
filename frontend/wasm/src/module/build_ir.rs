use core::{mem, str::FromStr};
use std::rc::Rc;

use midenc_hir::{
    Builder, BuilderExt, Context, FunctionIdent, FxHashMap, Ident, Op, OpBuilder, SymbolPath,
    Visibility,
    constants::ConstantData,
    dialects::builtin::{
        self, BuiltinOpBuilder, ComponentBuilder, ModuleBuilder, World, WorldBuilder,
    },
    interner::Symbol,
    version::Version,
};
use midenc_session::diagnostics::{DiagnosticsHandler, IntoDiagnostic, Severity, SourceSpan};
use wasmparser::Validator;

use super::{
    MemoryIndex, module_translation_state::ModuleTranslationState, types::ModuleTypesBuilder,
};
use crate::{
    WasmTranslationConfig,
    error::WasmResult,
    intrinsics::Intrinsic,
    module::{
        DefinedFuncIndex,
        func_translator::FuncTranslator,
        linker_stubs::is_unreachable_stub,
        module_env::{FunctionBodyData, ModuleEnvironment, ParsedModule},
        types::ir_type,
    },
};

/// Translate a valid Wasm core module binary into Miden IR component building
/// component imports for well-known Miden ABI functions
///
/// This is a temporary solution until we compile an account code as Wasm
/// component. To be able to do it we need wit-bindgen type re-mapping implemented first (see
/// https://github.com/0xMiden/compiler/issues/116)
pub fn translate_module_as_component(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<builtin::ComponentRef> {
    let mut validator = Validator::new_with_features(crate::supported_features());
    let parser = wasmparser::Parser::new(0);
    let mut module_types_builder = Default::default();
    let mut parsed_module = ModuleEnvironment::new(
        config,
        &mut validator,
        &mut module_types_builder,
    )
    .parse(parser, wasm, context.diagnostics())?;
    parsed_module.module.set_name_fallback(config.source_name.clone());
    if let Some(name_override) = config.override_name.as_ref() {
        parsed_module.module.set_name_override(name_override.clone());
    }
    let module_types = module_types_builder;

    // If a world wasn't provided to us, create one
    let world_ref = match config.world {
        Some(world) => world,
        None => context.clone().builder().create::<World, ()>(Default::default())()?,
    };
    let mut world_builder = WorldBuilder::new(world_ref);

    let ns = Ident::from("root_ns");
    let name = Ident::from("root");
    let ver = Version::parse("1.0.0").unwrap();
    let component_ref = world_builder.define_component(ns, name, ver)?;
    let mut cb = ComponentBuilder::new(component_ref);
    let module_name = parsed_module.module.name().as_str();
    let module_ref = cb.define_module(Ident::from(module_name)).unwrap();

    let mut module_builder = ModuleBuilder::new(module_ref);
    let mut module_state = ModuleTranslationState::new(
        &parsed_module.module,
        &mut module_builder,
        &mut world_builder,
        &module_types,
        FxHashMap::default(),
        context.diagnostics(),
    )?;
    build_ir_module(&mut parsed_module, &module_types, &mut module_state, config, context)?;

    Ok(component_ref)
}

pub fn build_ir_module(
    parsed_module: &mut ParsedModule,
    module_types: &ModuleTypesBuilder,
    module_state: &mut ModuleTranslationState,
    _config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<()> {
    let _memory_size = parsed_module
        .module
        .memories
        .get(MemoryIndex::from_u32(0))
        .map(|mem| mem.minimum as u32);

    build_globals(&parsed_module.module, module_state.module_builder, context.diagnostics())?;
    build_data_segments(parsed_module, module_state.module_builder, context.diagnostics())?;
    let addr2line = addr2line::Context::from_dwarf(gimli::Dwarf {
        debug_abbrev: parsed_module.debuginfo.dwarf.debug_abbrev,
        debug_addr: parsed_module.debuginfo.dwarf.debug_addr,
        debug_aranges: parsed_module.debuginfo.dwarf.debug_aranges,
        debug_info: parsed_module.debuginfo.dwarf.debug_info,
        debug_line: parsed_module.debuginfo.dwarf.debug_line,
        debug_line_str: parsed_module.debuginfo.dwarf.debug_line_str,
        debug_str: parsed_module.debuginfo.dwarf.debug_str,
        debug_str_offsets: parsed_module.debuginfo.dwarf.debug_str_offsets,
        debug_types: parsed_module.debuginfo.dwarf.debug_types,
        locations: parsed_module.debuginfo.dwarf.locations,
        ranges: parsed_module.debuginfo.dwarf.ranges,
        file_type: parsed_module.debuginfo.dwarf.file_type,
        sup: parsed_module.debuginfo.dwarf.sup.clone(),
        ..Default::default()
    })
    .into_diagnostic()?;
    let mut func_translator = FuncTranslator::new(context.clone());
    // Although this renders this parsed module invalid(without function
    // bodies), we don't support multiple module instances. Thus, this
    // ParseModule will not be used again to make another module instance.
    let func_body_inputs = mem::take(&mut parsed_module.function_body_inputs);

    // Two-pass approach for linker stub inlining:
    // Pass 1: Detect and register ALL linker stubs first.
    // This ensures that when we translate function bodies in pass 2,
    // all stubs are known and calls to them will be inlined regardless
    // of function ordering in the WASM file.
    let mut stub_indices: Vec<DefinedFuncIndex> = Vec::new();
    for (defined_func_idx, body_data) in &func_body_inputs {
        if !is_unreachable_stub(&body_data.body) {
            continue;
        }

        let func_index = parsed_module.module.func_index(defined_func_idx);
        let func_name = parsed_module.module.func_name(func_index).as_str();

        // Try to parse the function name as a MASM function ident to get the symbol path
        let Ok(func_ident) = FunctionIdent::from_str(func_name) else {
            continue;
        };
        let import_path: SymbolPath = SymbolPath::from_masm_function_id(func_ident);

        // Try to recognize as an intrinsic
        let Ok(intrinsic) = Intrinsic::try_from(&import_path) else {
            continue;
        };

        // Register the stub so calls to it will be inlined
        if module_state.register_linker_stub(func_index, intrinsic)?.is_some() {
            stub_indices.push(defined_func_idx);
        }
    }

    // Pass 2: Translate non-stub function bodies.
    // At this point, all stubs are registered and calls to them will be inlined.
    for (defined_func_idx, body_data) in func_body_inputs {
        // Skip stubs - they were already registered in pass 1
        if stub_indices.contains(&defined_func_idx) {
            continue;
        }

        let func_index = parsed_module.module.func_index(defined_func_idx);
        let func_name = parsed_module.module.func_name(func_index).as_str();

        let function_ref =
            module_state.module_builder.get_function(func_name).unwrap_or_else(|| {
                panic!("cannot build {func_name} function, since it is not defined in the module.")
            });

        let FunctionBodyData { validator, body } = body_data;
        let mut func_validator = validator.into_validator(Default::default());
        func_translator.translate_body(
            &body,
            function_ref,
            module_state,
            parsed_module,
            module_types,
            &addr2line,
            context.session(),
            &mut func_validator,
            _config,
        )?;
    }
    Ok(())
}

fn build_globals(
    wasm_module: &crate::module::Module,
    module_builder: &mut ModuleBuilder,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    let span = SourceSpan::default();
    for (global_idx, global) in &wasm_module.globals {
        let global_name = wasm_module
            .name_section
            .globals_names
            .get(&global_idx)
            .cloned()
            .unwrap_or(Symbol::intern(format!("gv{}", global_idx.as_u32())));
        let global_init = wasm_module.try_global_initializer(global_idx, diagnostics)?;
        let visibility = if wasm_module.is_exported(global_idx.into()) {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let mut global_var_ref = module_builder
            .define_global_variable(
                global_name.into(),
                visibility,
                ir_type(global.ty, diagnostics)?,
            )
            .map_err(|e| {
                diagnostics
                    .diagnostic(Severity::Error)
                    .with_message(
                        (format!(
                            "Failed to declare global variable '{global_name}' with error: {e:?}"
                        ))
                        .clone(),
                    )
                    .into_report()
            })?;
        let context = global_var_ref.borrow().as_operation().context_rc().clone();
        let init_region_ref = {
            let mut global_var = global_var_ref.borrow_mut();
            global_var.initializer_mut().as_region_ref()
        };
        let mut op_builder = OpBuilder::new(context);
        op_builder.create_block(init_region_ref, None, &[]);
        op_builder.ret_imm(global_init.to_imm(wasm_module, diagnostics)?, span)?;
    }
    Ok(())
}

fn build_data_segments(
    translation: &ParsedModule,
    module_builder: &mut ModuleBuilder,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    for (data_segment_idx, data_segment) in &translation.data_segments {
        let data_segment_name =
            translation.module.name_section.data_segment_names[&data_segment_idx];
        let readonly = data_segment_name.as_str().contains(".rodata");
        let offset = data_segment.offset.as_i32(&translation.module, diagnostics)? as u32;
        let init = ConstantData::from(data_segment.data.to_vec());
        let size = init.len() as u32;
        if let Err(e) =
            module_builder.define_data_segment(offset, init, readonly, SourceSpan::default())
        {
            return Err(e.wrap_err(format!(
                "Failed to declare data segment '{data_segment_name}' with size '{size}' at \
                 '{offset:#x}'"
            )));
        }
    }
    Ok(())
}
