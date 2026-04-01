use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use std::path::Path;

use addr2line::Context;
use cranelift_entity::EntityRef;
use gimli::{self, AttributeValue, read::Operation};
use log::debug;
use midenc_hir::{
    DICompileUnit, DIExpression, DIExpressionOp, DILocalVariable, DISubprogram,
    FxHashMap, SourceSpan, interner::Symbol,
};
use midenc_session::diagnostics::{DiagnosticsHandler, IntoDiagnostic};

use super::{
    FuncIndex, Module,
    module_env::{DwarfReader, FunctionBodyData, ParsedModule},
    types::{WasmFuncType, convert_valtype, ir_type},
};
use crate::module::types::ModuleTypesBuilder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationDescriptor {
    /// Inclusive start offset within the function's code, relative to the Wasm code section.
    pub start: u64,
    /// Exclusive end offset. `None` indicates the location is valid until the end of the function.
    pub end: Option<u64>,
    pub storage: VariableStorage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariableStorage {
    Local(u32),
    Global(u32),
    Stack(u32),
    ConstU64(u64),
    /// Frame base (global index) + byte offset — from DW_OP_fbreg
    FrameBase { global_index: u32, byte_offset: i64 },
    Unsupported,
}

impl VariableStorage {
    pub fn as_local(&self) -> Option<u32> {
        match self {
            VariableStorage::Local(index) => Some(*index),
            _ => None,
        }
    }

    pub fn to_expression_op(&self) -> DIExpressionOp {
        match self {
            VariableStorage::Local(idx) => DIExpressionOp::WasmLocal(*idx),
            VariableStorage::Global(idx) => DIExpressionOp::WasmGlobal(*idx),
            VariableStorage::Stack(idx) => DIExpressionOp::WasmStack(*idx),
            VariableStorage::ConstU64(val) => DIExpressionOp::ConstU64(*val),
            VariableStorage::FrameBase { global_index, byte_offset } => {
                DIExpressionOp::FrameBase {
                    global_index: *global_index,
                    byte_offset: *byte_offset,
                }
            }
            VariableStorage::Unsupported => {
                DIExpressionOp::Unsupported(Symbol::intern("unsupported"))
            }
        }
    }
}

#[derive(Clone)]
pub struct LocalDebugInfo {
    pub attr: DILocalVariable,
    pub locations: Vec<LocationDescriptor>,
    pub expression: Option<DIExpression>,
}

#[derive(Clone)]
pub struct FunctionDebugInfo {
    pub compile_unit: DICompileUnit,
    pub subprogram: DISubprogram,
    pub locals: Vec<Option<LocalDebugInfo>>,
    pub function_span: Option<SourceSpan>,
    pub location_schedule: Vec<LocationScheduleEntry>,
    pub next_location_event: usize,
}

#[derive(Default, Clone)]
struct DwarfLocalData {
    name: Option<Symbol>,
    locations: Vec<LocationDescriptor>,
    decl_line: Option<u32>,
    decl_column: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationScheduleEntry {
    pub offset: u64,
    pub var_index: usize,
    pub storage: VariableStorage,
}

impl FunctionDebugInfo {
    pub fn local_attr(&self, index: usize) -> Option<&DILocalVariable> {
        self.locals.get(index).and_then(|info| info.as_ref().map(|data| &data.attr))
    }
}

pub fn collect_function_debug_info(
    parsed_module: &ParsedModule,
    module_types: &ModuleTypesBuilder,
    module: &Module,
    addr2line: &Context<DwarfReader<'_>>,
    diagnostics: &DiagnosticsHandler,
) -> FxHashMap<FuncIndex, Rc<RefCell<FunctionDebugInfo>>> {
    let mut map = FxHashMap::default();

    let collected = collect_dwarf_local_data(parsed_module, module, diagnostics);

    debug!(
        "Collecting function debug info for {} functions",
        parsed_module.function_body_inputs.len()
    );

    for (defined_idx, body) in parsed_module.function_body_inputs.iter() {
        let func_index = module.func_index(defined_idx);
        let func_name = module.func_name(func_index);
        if let Some(info) = build_function_debug_info(
            parsed_module,
            module_types,
            module,
            func_index,
            body,
            addr2line,
            diagnostics,
            collected.by_local.get(&func_index),
            collected.frame_base.get(&func_index),
        ) {
            debug!(
                "Collected debug info for function {}: {} locals",
                func_name.as_str(),
                info.locals.len()
            );
            map.insert(func_index, Rc::new(RefCell::new(info)));
        } else {
            debug!("No debug info collected for function {}", func_name.as_str());
        }
    }

    debug!("Collected debug info for {} functions total", map.len());
    map
}

#[allow(clippy::too_many_arguments)]
fn build_function_debug_info(
    parsed_module: &ParsedModule,
    module_types: &ModuleTypesBuilder,
    module: &Module,
    func_index: FuncIndex,
    body: &FunctionBodyData,
    addr2line: &Context<DwarfReader<'_>>,
    diagnostics: &DiagnosticsHandler,
    dwarf_locals: Option<&FxHashMap<u32, DwarfLocalData>>,
    frame_base_vars: Option<&Vec<DwarfLocalData>>,
) -> Option<FunctionDebugInfo> {
    let func_name = module.func_name(func_index);

    let (file_symbol, directory_symbol) = determine_file_symbols(parsed_module, addr2line, body);
    let (line, column) = determine_location(addr2line, body.body_offset);

    let mut compile_unit = DICompileUnit::new(Symbol::intern("wasm"), file_symbol);
    compile_unit.directory = directory_symbol;
    compile_unit.producer = Some(Symbol::intern("midenc-frontend-wasm"));

    let mut subprogram = DISubprogram::new(func_name, compile_unit.file, line, column);
    subprogram.is_definition = true;

    let wasm_signature = module_types[module.functions[func_index].signature].clone();
    let locals = build_local_debug_info(
        module,
        func_index,
        &wasm_signature,
        body,
        &subprogram,
        diagnostics,
        dwarf_locals,
        frame_base_vars,
    );
    let location_schedule = build_location_schedule(&locals);

    Some(FunctionDebugInfo {
        compile_unit,
        subprogram,
        locals,
        function_span: None,
        location_schedule,
        next_location_event: 0,
    })
}

fn determine_file_symbols(
    parsed_module: &ParsedModule,
    addr2line: &Context<DwarfReader<'_>>,
    body: &FunctionBodyData,
) -> (Symbol, Option<Symbol>) {
    if let Some(location) = addr2line
        .find_location(body.body_offset)
        .ok()
        .flatten()
        .and_then(|loc| loc.file.map(|file| file.to_owned()))
    {
        let path = Path::new(location.as_str());
        let directory_symbol = path.parent().and_then(|parent| parent.to_str()).map(Symbol::intern);
        let file_symbol = Symbol::intern(location.as_str());
        (file_symbol, directory_symbol)
    } else if let Some(path) = parsed_module.wasm_file.path.as_ref() {
        let file_symbol = Symbol::intern(path.to_string_lossy().as_ref());
        let directory_symbol = path.parent().and_then(|parent| parent.to_str()).map(Symbol::intern);
        (file_symbol, directory_symbol)
    } else {
        (Symbol::intern("unknown"), None)
    }
}

fn determine_location(addr2line: &Context<DwarfReader<'_>>, offset: u64) -> (u32, Option<u32>) {
    match addr2line.find_location(offset).ok().flatten() {
        Some(location) => {
            let line = location.line.unwrap_or_default();
            let column = location.column;
            (line, column)
        }
        None => (0, None),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_local_debug_info(
    module: &Module,
    func_index: FuncIndex,
    wasm_signature: &WasmFuncType,
    body: &FunctionBodyData,
    subprogram: &DISubprogram,
    diagnostics: &DiagnosticsHandler,
    dwarf_locals: Option<&FxHashMap<u32, DwarfLocalData>>,
    frame_base_vars: Option<&Vec<DwarfLocalData>>,
) -> Vec<Option<LocalDebugInfo>> {
    let param_count = wasm_signature.params().len();
    let mut local_entries = Vec::new();
    if let Ok(mut locals_reader) = body.body.get_locals_reader().into_diagnostic() {
        let decl_count = locals_reader.get_count();
        for _ in 0..decl_count {
            if let Ok((count, ty)) = locals_reader.read().into_diagnostic() {
                local_entries.push((count, ty));
            }
        }
    }
    let local_count: usize = local_entries.iter().map(|(count, _)| *count as usize).sum();

    let total = param_count + local_count;
    let mut locals = vec![None; total];

    for (param_idx, wasm_ty) in wasm_signature.params().iter().enumerate() {
        let index_u32 = param_idx as u32;
        let dwarf_entry = dwarf_locals.and_then(|map| map.get(&index_u32));
        let mut name_symbol = module
            .local_name(func_index, index_u32)
            .unwrap_or_else(|| Symbol::intern(format!("arg{param_idx}")));
        if let Some(info) = dwarf_entry
            && let Some(symbol) = info.name
        {
            name_symbol = symbol;
        }
        let mut attr = DILocalVariable::new(
            name_symbol,
            subprogram.file,
            subprogram.line,
            subprogram.column,
        );
        attr.arg_index = Some((param_idx + 1) as u32);
        if let Ok(ty) = ir_type(*wasm_ty, diagnostics) {
            attr.ty = Some(ty);
        }
        let dwarf_info = dwarf_entry.cloned();
        if let Some(info) = dwarf_info.as_ref() {
            if let Some(line) = info.decl_line
                && line != 0
            {
                attr.line = line;
            }
            if info.decl_column.is_some() {
                attr.column = info.decl_column;
            }
        }
        let locations = dwarf_info.as_ref().map(|info| info.locations.clone()).unwrap_or_default();

        // Create expression from the first location if available
        let expression = if !locations.is_empty() {
            let ops = vec![locations[0].storage.to_expression_op()];
            Some(DIExpression::with_ops(ops))
        } else {
            None
        };

        locals[param_idx] = Some(LocalDebugInfo {
            attr,
            locations,
            expression,
        });
    }

    let mut next_local_index = param_count;
    for (count, ty) in local_entries {
        for _ in 0..count {
            let index_u32 = next_local_index as u32;
            let dwarf_entry = dwarf_locals.and_then(|map| map.get(&index_u32));
            let mut name_symbol = module
                .local_name(func_index, index_u32)
                .unwrap_or_else(|| Symbol::intern(format!("local{next_local_index}")));
            if let Some(info) = dwarf_entry
                && let Some(symbol) = info.name
            {
                name_symbol = symbol;
            }
            let mut attr = DILocalVariable::new(
                name_symbol,
                subprogram.file,
                subprogram.line,
                subprogram.column,
            );
            let wasm_ty = convert_valtype(ty);
            if let Ok(ir_ty) = ir_type(wasm_ty, diagnostics) {
                attr.ty = Some(ir_ty);
            }
            let dwarf_info = dwarf_entry.cloned();
            if let Some(info) = dwarf_info.as_ref() {
                if let Some(line) = info.decl_line
                    && line != 0
                {
                    attr.line = line;
                }
                if info.decl_column.is_some() {
                    attr.column = info.decl_column;
                }
            }
            let locations =
                dwarf_info.as_ref().map(|info| info.locations.clone()).unwrap_or_default();

            // Create expression from the first location if available
            let expression = if !locations.is_empty() {
                let ops = vec![locations[0].storage.to_expression_op()];
                Some(DIExpression::with_ops(ops))
            } else {
                None
            };

            locals[next_local_index] = Some(LocalDebugInfo {
                attr,
                locations,
                expression,
            });
            next_local_index += 1;
        }
    }

    // Append FrameBase-only variables beyond normal WASM locals.
    // These are variables like local `sum` in debug builds that live in
    // linear memory via __stack_pointer and have no WASM local index.
    if let Some(fb_vars) = frame_base_vars {
        for fb_var in fb_vars {
            let name = fb_var.name.unwrap_or_else(|| Symbol::intern("?"));
            let mut attr = DILocalVariable::new(
                name,
                subprogram.file,
                subprogram.line,
                subprogram.column,
            );
            if let Some(line) = fb_var.decl_line.filter(|l| *l != 0) {
                attr.line = line;
            }
            attr.column = fb_var.decl_column;
            let expression = if !fb_var.locations.is_empty() {
                Some(DIExpression::with_ops(vec![fb_var.locations[0].storage.to_expression_op()]))
            } else {
                None
            };
            locals.push(Some(LocalDebugInfo {
                attr,
                locations: fb_var.locations.clone(),
                expression,
            }));
        }
    }

    locals
}

fn build_location_schedule(locals: &[Option<LocalDebugInfo>]) -> Vec<LocationScheduleEntry> {
    let mut schedule = Vec::new();
    for (var_index, info_opt) in locals.iter().enumerate() {
        let Some(info) = info_opt else {
            continue;
        };
        for descriptor in &info.locations {
            if descriptor.storage.as_local().is_none()
                && !matches!(descriptor.storage, VariableStorage::FrameBase { .. })
            {
                continue;
            }
            schedule.push(LocationScheduleEntry {
                offset: descriptor.start,
                var_index,
                storage: descriptor.storage.clone(),
            });
        }
    }
    schedule.sort_by(|a, b| a.offset.cmp(&b.offset));
    schedule
}

/// Collected DWARF local data for all functions.
struct CollectedDwarfLocals {
    /// Variables keyed by WASM local index (existing behavior).
    by_local: FxHashMap<FuncIndex, FxHashMap<u32, DwarfLocalData>>,
    /// FrameBase-only variables that have no WASM local index (e.g. `sum` in debug builds).
    frame_base: FxHashMap<FuncIndex, Vec<DwarfLocalData>>,
}

fn collect_dwarf_local_data(
    parsed_module: &ParsedModule,
    module: &Module,
    diagnostics: &DiagnosticsHandler,
) -> CollectedDwarfLocals {
    let _ = diagnostics;
    let dwarf = &parsed_module.debuginfo.dwarf;

    let mut func_by_name = FxHashMap::default();
    for (func_index, _) in module.functions.iter() {
        let name = module.func_name(func_index).as_str().to_owned();
        func_by_name.insert(name, func_index);
    }

    let mut low_pc_map = FxHashMap::default();
    let code_section_offset = parsed_module.wasm_file.code_section_offset;
    for (defined_idx, body) in parsed_module.function_body_inputs.iter() {
        let func_index = module.func_index(defined_idx);
        let adjusted = body.body_offset.saturating_sub(code_section_offset);
        low_pc_map.insert(adjusted, func_index);
    }

    let mut results: FxHashMap<FuncIndex, FxHashMap<u32, DwarfLocalData>> = FxHashMap::default();
    let mut fb_results: FxHashMap<FuncIndex, Vec<DwarfLocalData>> = FxHashMap::default();
    let mut units = dwarf.units();
    loop {
        let header = match units.next() {
            Ok(Some(header)) => header,
            Ok(None) => break,
            Err(err) => {
                debug!("failed to iterate DWARF units: {err:?}");
                break;
            }
        };
        let unit = match dwarf.unit(header) {
            Ok(unit) => unit,
            Err(err) => {
                debug!("failed to load DWARF unit: {err:?}");
                continue;
            }
        };

        let mut entries = unit.entries();
        loop {
            let next = match entries.next_dfs() {
                Ok(Some(data)) => data,
                Ok(None) => break,
                Err(err) => {
                    debug!("error while traversing DWARF entries: {err:?}");
                    break;
                }
            };
            let (delta, entry) = next;
            let _ = delta; // we don't need depth deltas explicitly.

            if entry.tag() == gimli::DW_TAG_subprogram {
                let Some(info) =
                    resolve_subprogram_target(dwarf, &unit, &func_by_name, &low_pc_map, entry)
                else {
                    continue;
                };

                if let Err(err) = collect_subprogram_variables(
                    dwarf,
                    &unit,
                    entry.offset(),
                    info.func_index,
                    info.low_pc,
                    info.high_pc,
                    info.frame_base_global,
                    &mut results,
                    &mut fb_results,
                ) {
                    debug!("failed to gather variables for function {:?}: {err:?}", info.func_index);
                }
            }
        }
    }

    CollectedDwarfLocals {
        by_local: results,
        frame_base: fb_results,
    }
}

/// Result of resolving a DWARF subprogram to a WASM function.
struct SubprogramInfo {
    func_index: FuncIndex,
    low_pc: u64,
    high_pc: Option<u64>,
    /// The WASM global index used as the frame base (from DW_AT_frame_base).
    /// Typically global 0 (__stack_pointer).
    frame_base_global: Option<u32>,
}

fn resolve_subprogram_target<R: gimli::Reader<Offset = usize>>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    func_by_name: &FxHashMap<String, FuncIndex>,
    low_pc_map: &FxHashMap<u64, FuncIndex>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<SubprogramInfo> {
    let mut maybe_name: Option<String> = None;
    let mut low_pc = None;
    let mut high_pc = None;
    let mut frame_base_global = None;

    let mut attrs = entry.attrs();
    while let Ok(Some(attr)) = attrs.next() {
        match attr.name() {
            gimli::DW_AT_name => {
                if let Ok(raw) = dwarf.attr_string(unit, attr.value())
                    && let Ok(name) = raw.to_string_lossy()
                {
                    maybe_name = Some(name.into_owned());
                }
            }
            gimli::DW_AT_linkage_name => {
                if maybe_name.is_none()
                    && let Ok(raw) = dwarf.attr_string(unit, attr.value())
                    && let Ok(name) = raw.to_string_lossy()
                {
                    maybe_name = Some(name.into_owned());
                }
            }
            gimli::DW_AT_low_pc => match attr.value() {
                AttributeValue::Addr(addr) => low_pc = Some(addr),
                AttributeValue::Udata(val) => low_pc = Some(val),
                _ => {}
            },
            gimli::DW_AT_high_pc => match attr.value() {
                AttributeValue::Addr(addr) => high_pc = Some(addr),
                AttributeValue::Udata(size) => {
                    if let Some(base) = low_pc {
                        high_pc = Some(base.saturating_add(size));
                    }
                }
                _ => {}
            },
            gimli::DW_AT_frame_base => {
                // Decode the frame base expression to find which WASM global
                // provides the base address (typically __stack_pointer = global 0).
                // Only WASM globals are supported — downstream FrameBase resolution
                // assumes the index refers to a global in the linker's layout.
                if let AttributeValue::Exprloc(expr) = attr.value() {
                    let mut ops = expr.operations(unit.encoding());
                    while let Ok(Some(op)) = ops.next() {
                        if let Operation::WasmLocal { .. } = op {
                            debug!("DW_AT_frame_base uses WASM local; only globals are supported — ignoring");
                        } else if let Operation::WasmGlobal { index } = op {
                            frame_base_global = Some(index);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let make_info = |func_index, lp, hp| SubprogramInfo {
        func_index,
        low_pc: lp,
        high_pc: hp,
        frame_base_global,
    };

    if let Some(ref name) = maybe_name {
        if let Some(&func_index) = func_by_name.get(name) {
            return Some(make_info(func_index, low_pc.unwrap_or_default(), high_pc));
        }
    }

    if let Some(base) = low_pc
        && let Some(&func_index) = low_pc_map.get(&base)
    {
        return Some(make_info(func_index, base, high_pc));
    }
    None
}

fn collect_subprogram_variables<R: gimli::Reader<Offset = usize>>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    offset: gimli::UnitOffset<R::Offset>,
    func_index: FuncIndex,
    low_pc: u64,
    high_pc: Option<u64>,
    frame_base_global: Option<u32>,
    results: &mut FxHashMap<FuncIndex, FxHashMap<u32, DwarfLocalData>>,
    fb_results: &mut FxHashMap<FuncIndex, Vec<DwarfLocalData>>,
) -> gimli::Result<()> {
    let mut tree = unit.entries_tree(Some(offset))?;
    let root = tree.root()?;
    let mut children = root.children();
    let mut param_counter: u32 = 0;
    while let Some(child) = children.next()? {
        walk_variable_nodes(
            dwarf, unit, child, func_index, low_pc, high_pc, frame_base_global, results,
            fb_results, &mut param_counter,
        )?;
    }
    Ok(())
}

fn walk_variable_nodes<R: gimli::Reader<Offset = usize>>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    node: gimli::EntriesTreeNode<R>,
    func_index: FuncIndex,
    low_pc: u64,
    high_pc: Option<u64>,
    frame_base_global: Option<u32>,
    results: &mut FxHashMap<FuncIndex, FxHashMap<u32, DwarfLocalData>>,
    fb_results: &mut FxHashMap<FuncIndex, Vec<DwarfLocalData>>,
    param_counter: &mut u32,
) -> gimli::Result<()> {
    let entry = node.entry();
    let tag = entry.tag();
    match tag {
        gimli::DW_TAG_formal_parameter | gimli::DW_TAG_variable => {
            // For formal parameters, the WASM local index equals the parameter
            // order (params are always the first N WASM locals).
            let fallback_index = if tag == gimli::DW_TAG_formal_parameter {
                let idx = *param_counter;
                *param_counter += 1;
                Some(idx)
            } else {
                None
            };
            let mut fb_vars = Vec::new();
            if let Some((local_index, mut data)) =
                decode_variable_entry(dwarf, unit, entry, low_pc, high_pc, frame_base_global, fallback_index, &mut fb_vars)?
            {
                let local_map = results.entry(func_index).or_default();
                let entry = local_map.entry(local_index).or_insert_with(DwarfLocalData::default);
                entry.name = entry.name.or(data.name);
                entry.decl_line = entry.decl_line.or(data.decl_line);
                entry.decl_column = entry.decl_column.or(data.decl_column);
                if !data.locations.is_empty() {
                    entry.locations.append(&mut data.locations);
                }
            }
            if !fb_vars.is_empty() {
                fb_results.entry(func_index).or_default().extend(fb_vars);
            }
        }
        _ => {}
    }

    let mut children = node.children();
    while let Some(child) = children.next()? {
        walk_variable_nodes(
            dwarf, unit, child, func_index, low_pc, high_pc, frame_base_global, results,
            fb_results, param_counter,
        )?;
    }
    Ok(())
}

fn decode_variable_entry<R: gimli::Reader<Offset = usize>>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<'_, '_, R>,
    low_pc: u64,
    high_pc: Option<u64>,
    frame_base_global: Option<u32>,
    fallback_index: Option<u32>,
    frame_base_vars: &mut Vec<DwarfLocalData>,
) -> gimli::Result<Option<(u32, DwarfLocalData)>> {
    let mut name_symbol = None;
    let mut location_attr = None;
    let mut decl_line = None;
    let mut decl_column = None;

    let mut attrs = entry.attrs();
    while let Some(attr) = attrs.next()? {
        match attr.name() {
            gimli::DW_AT_name => {
                if let Ok(raw) = dwarf.attr_string(unit, attr.value())
                    && let Ok(text) = raw.to_string_lossy()
                {
                    name_symbol = Some(Symbol::intern(text.as_ref()));
                }
            }
            gimli::DW_AT_location => location_attr = Some(attr.value()),
            gimli::DW_AT_decl_line => {
                if let Some(line) = attr.udata_value() {
                    decl_line = Some(line as u32);
                }
            }
            gimli::DW_AT_decl_column => {
                if let Some(column) = attr.udata_value() {
                    decl_column = Some(column as u32);
                }
            }
            _ => {}
        }
    }

    let Some(location_value) = location_attr else {
        return Ok(None);
    };

    let mut locations = Vec::new();

    match location_value {
        AttributeValue::Exprloc(ref expr) => {
            let storage = decode_storage_from_expression(expr, unit, frame_base_global)?;
            if let Some(storage) = storage {
                // Determine the WASM local index for this variable.
                // For WasmLocal storage, use the index directly.
                // For FrameBase (DW_OP_fbreg), use the parameter order as
                // fallback since formal params map to WASM locals 0..N.
                let local_index = storage.as_local().or(fallback_index);
                if let Some(local_index) = local_index {
                    locations.push(LocationDescriptor {
                        start: low_pc,
                        end: high_pc,
                        storage,
                    });
                    let data = DwarfLocalData {
                        name: name_symbol,
                        locations,
                        decl_line,
                        decl_column,
                    };
                    return Ok(Some((local_index, data)));
                } else if matches!(&storage, VariableStorage::FrameBase { .. }) {
                    // FrameBase-only variable (no WASM local index, e.g. local `sum`
                    // in debug builds). Collect separately instead of dropping.
                    locations.push(LocationDescriptor {
                        start: low_pc,
                        end: high_pc,
                        storage,
                    });
                    let data = DwarfLocalData {
                        name: name_symbol,
                        locations,
                        decl_line,
                        decl_column,
                    };
                    frame_base_vars.push(data);
                    return Ok(None);
                }
            }
            return Ok(None);
        }
        AttributeValue::LocationListsRef(offset) => {
            let mut iter = dwarf.locations.locations(
                offset,
                unit.encoding(),
                low_pc,
                &dwarf.debug_addr,
                unit.addr_base,
            )?;
            let mut has_frame_base = false;
            while let Some(entry) = iter.next()? {
                let storage_expr = entry.data;
                if let Some(storage) = decode_storage_from_expression(&storage_expr, unit, frame_base_global)? {
                    if storage.as_local().is_some() || matches!(&storage, VariableStorage::FrameBase { .. }) {
                        if matches!(&storage, VariableStorage::FrameBase { .. }) {
                            has_frame_base = true;
                        }
                        locations.push(LocationDescriptor {
                            start: entry.range.begin,
                            end: Some(entry.range.end),
                            storage,
                        });
                    }
                }
            }
            if locations.is_empty() {
                return Ok(None);
            }
            // Try to find a WASM local index from any location descriptor
            if let Some(local_index) = locations.iter().find_map(|desc| desc.storage.as_local()) {
                let data = DwarfLocalData {
                    name: name_symbol,
                    locations,
                    decl_line,
                    decl_column,
                };
                return Ok(Some((local_index, data)));
            } else if has_frame_base {
                // FrameBase-only location list variable
                let data = DwarfLocalData {
                    name: name_symbol,
                    locations,
                    decl_line,
                    decl_column,
                };
                frame_base_vars.push(data);
                return Ok(None);
            }
            return Ok(None);
        }
        _ => {}
    }

    Ok(None)
}

fn decode_storage_from_expression<R: gimli::Reader<Offset = usize>>(
    expr: &gimli::Expression<R>,
    unit: &gimli::Unit<R>,
    frame_base_global: Option<u32>,
) -> gimli::Result<Option<VariableStorage>> {
    let mut operations = expr.clone().operations(unit.encoding());
    let mut storage = None;
    while let Some(op) = operations.next()? {
        match op {
            Operation::WasmLocal { index } => storage = Some(VariableStorage::Local(index)),
            Operation::WasmGlobal { index } => storage = Some(VariableStorage::Global(index)),
            Operation::WasmStack { index } => storage = Some(VariableStorage::Stack(index)),
            Operation::UnsignedConstant { value } => {
                storage = Some(VariableStorage::ConstU64(value))
            }
            Operation::StackValue => {}
            Operation::FrameOffset { offset } => {
                // DW_OP_fbreg(offset): variable is at frame_base + offset in
                // WASM linear memory. The frame base is a WASM global
                // (typically __stack_pointer = global 0).
                if let Some(global_index) = frame_base_global {
                    storage = Some(VariableStorage::FrameBase {
                        global_index,
                        byte_offset: offset,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(storage)
}

fn func_local_index(func_index: FuncIndex, module: &Module) -> Option<usize> {
    module.defined_func_index(func_index).map(|idx| idx.index())
}
