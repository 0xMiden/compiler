// TODO: remove when completed
#![allow(unused)]

use core::mem;
use std::rc::Rc;

use midenc_dialect_hir::FunctionBuilder;
use midenc_hir::{
    diagnostics::{DiagnosticsHandler, IntoDiagnostic, Severity, SourceSpan},
    CallConv, ConstantData, Linkage, MidenAbiImport, Symbol,
};
use midenc_hir2::{
    dialects::builtin::{Component, ComponentBuilder, Function, Module, ModuleBuilder, ModuleRef},
    version::Version,
    BuilderExt, Context, Ident,
};
use midenc_session::Session;
use wasmparser::Validator;

use super::{module_translation_state::ModuleTranslationState, MemoryIndex};
use crate::{
    error::WasmResult,
    intrinsics::is_miden_intrinsics_module,
    miden_abi::miden_abi_function_type,
    module::{
        func_translator::FuncTranslator,
        module_env::{FunctionBodyData, ModuleEnvironment, ParsedModule},
        types::{ir_func_sig, ir_func_type, ir_type, ModuleTypes},
    },
    WasmTranslationConfig,
};

/// Translate a valid Wasm core module binary into Miden IR component building
/// component imports for well-known Miden ABI functions
///
/// This is a temporary solution until we compile an account code as Wasm
/// component. To be able to do it we need wit-bindgen type re-mapping implemented first (see
/// https://github.com/0xPolygonMiden/compiler/issues/116)
pub fn translate_module_as_component(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<midenc_hir2::dialects::builtin::ComponentRef> {
    let mut validator = Validator::new_with_features(crate::supported_features());
    let parser = wasmparser::Parser::new(0);
    let mut module_types_builder = Default::default();
    let mut parsed_module = ModuleEnvironment::new(
        config,
        &mut validator,
        &mut module_types_builder,
    )
    .parse(parser, wasm, &context.session.diagnostics)?;
    parsed_module.module.set_name_fallback(config.source_name.clone());
    if let Some(name_override) = config.override_name.as_ref() {
        parsed_module.module.set_name_override(name_override.clone());
    }
    let module_types = module_types_builder.finish();

    let mut module_state = ModuleTranslationState::new(
        &parsed_module.module,
        &module_types,
        vec![],
        &context.session.diagnostics,
    );

    // TODO: get proper ns and name (from exported interfaces?)
    let ns = Ident::from("root_ns");
    let name = Ident::from("root");
    let ver = Version::parse("1.0.0").unwrap();
    let mut component_ref =
        context
            .clone()
            .builder()
            .create::<Component, (Ident, Ident, Version)>(Default::default())(ns, name, ver)
        .unwrap();
    let mut cb = ComponentBuilder::new(component_ref);
    let mut module_ref = cb
        .define_module(Ident::from(parsed_module.module.name().as_str()))
        .unwrap()
        .borrow_mut();
    let module = module_ref.as_mut().downcast_mut::<Module>().unwrap();

    build_ir_module(&mut parsed_module, &module_types, &mut module_state, config, context, module)?;

    // TODO: translate core module imports (create empty Components?)
    //
    // let mut cb = midenc_hir::ComponentBuilder::new(&context.session.diagnostics);
    // let module_imports = module.imports();
    // for import_module_id in module_imports.iter_module_names() {
    //     if let Some(imports) = module_imports.imported(import_module_id) {
    //         for ext_func in imports {
    //             if is_miden_intrinsics_module(ext_func.module.as_symbol()) {
    //                 // ignore intrinsics imports
    //                 continue;
    //             }
    //             let function_ty = miden_abi_function_type(
    //                 ext_func.module.as_symbol(),
    //                 ext_func.function.as_symbol(),
    //             );
    //             let component_import =
    //                 midenc_hir::ComponentImport::MidenAbiImport(MidenAbiImport::new(function_ty));
    //             cb.add_import(*ext_func, component_import);
    //         }
    //     }
    // }
    // cb.add_module(module.into()).expect("module is already added");

    Ok(component_ref)
}

pub fn build_ir_module(
    parsed_module: &mut ParsedModule,
    module_types: &ModuleTypes,
    module_state: &mut ModuleTranslationState,
    _config: &WasmTranslationConfig,
    context: Rc<Context>,
    module_ref: &mut Module,
) -> WasmResult<()> {
    let memory_size = parsed_module
        .module
        .memories
        .get(MemoryIndex::from_u32(0))
        .map(|mem| mem.minimum as u32);
    let mut module_builder = ModuleBuilder::new(module_ref);
    // if let Some(memory_size) = memory_size {
    // module_builder.with_reserved_memory_pages(memory_size);
    // }

    // TODO: globals and data segments
    //
    // build_globals(&parsed_module.module, &mut module_builder, &context.session.diagnostics)?;
    // build_data_segments(parsed_module, &mut module_builder, &context.session.diagnostics)?;
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
    let mut func_translator = FuncTranslator::new();
    // Although this renders this parsed module invalid(without functiong
    // bodies), we don't support multiple module instances. Thus, this
    // ParseModule will not be used again to make another module instance.
    let func_body_inputs = mem::take(&mut parsed_module.function_body_inputs);
    for (defined_func_idx, body_data) in func_body_inputs {
        let func_index = &parsed_module.module.func_index(defined_func_idx);
        let func_type = &parsed_module.module.functions[*func_index];
        let func_name = &parsed_module.module.func_name(*func_index);
        let wasm_func_type = module_types[func_type.signature].clone();
        let ir_func_type = ir_func_type(&wasm_func_type, &context.session.diagnostics)?;
        let linkage = if parsed_module.module.is_exported_function(func_index) {
            Linkage::External
        } else {
            Linkage::Internal
        };
        let sig = ir_func_sig(&ir_func_type, CallConv::SystemV, linkage);
        let mut function_ref = module_builder
            .define_function(Ident::from(func_name.as_str()), sig)
            .unwrap()
            .borrow_mut();
        let func = function_ref.as_mut().downcast_mut::<Function>().unwrap();
        // let mut module_func_builder = module_builder.function(func_name.as_str(), sig.clone())?;
        let FunctionBodyData { validator, body } = body_data;
        let mut func_validator = validator.into_validator(Default::default());
        func_translator.translate_body(
            &body,
            func,
            module_state,
            parsed_module,
            module_types,
            &addr2line,
            &context.session,
            &mut func_validator,
        )?;
        // module_func_builder.build(&context.session.diagnostics)?;
    }
    // let module = module_builder.build();
    Ok(())
}

// fn build_globals(
//     wasm_module: &Module,
//     module_builder: &mut ModuleBuilder,
//     diagnostics: &DiagnosticsHandler,
// ) -> WasmResult<()> {
//     for (global_idx, global) in &wasm_module.globals {
//         let global_name = wasm_module
//             .name_section
//             .globals_names
//             .get(&global_idx)
//             .cloned()
//             .unwrap_or(Symbol::intern(format!("gv{}", global_idx.as_u32())));
//         let global_init = wasm_module.try_global_initializer(global_idx, diagnostics)?;
//         let init = ConstantData::from(global_init.to_le_bytes(wasm_module, diagnostics)?);
//         if let Err(e) = module_builder.declare_global_variable(
//             global_name.as_str(),
//             ir_type(global.ty, diagnostics)?,
//             Linkage::External,
//             Some(init.clone()),
//             SourceSpan::default(),
//         ) {
//             let message = format!(
//                 "Failed to declare global variable '{global_name}' with initializer '{init}' with \
//                  error: {:?}",
//                 e
//             );
//             return Err(diagnostics
//                 .diagnostic(Severity::Error)
//                 .with_message(message.clone())
//                 .into_report());
//         }
//     }
//     Ok(())
// }
//
// fn build_data_segments(
//     translation: &ParsedModule,
//     module_builder: &mut ModuleBuilder,
//     diagnostics: &DiagnosticsHandler,
// ) -> WasmResult<()> {
//     for (data_segment_idx, data_segment) in &translation.data_segments {
//         let data_segment_name =
//             translation.module.name_section.data_segment_names[&data_segment_idx];
//         let readonly = data_segment_name.as_str().contains(".rodata");
//         let init = ConstantData::from(data_segment.data);
//         let offset = data_segment.offset.as_i32(&translation.module, diagnostics)? as u32;
//         let size = init.len() as u32;
//         if let Err(e) = module_builder.declare_data_segment(offset, size, init, readonly) {
//             let message = format!(
//                 "Failed to declare data segment '{data_segment_name}' with size '{size}' at \
//                  '{offset}' with error: {:?}",
//                 e
//             );
//             return Err(diagnostics
//                 .diagnostic(Severity::Error)
//                 .with_message(message.clone())
//                 .into_report());
//         }
//     }
//     Ok(())
// }
