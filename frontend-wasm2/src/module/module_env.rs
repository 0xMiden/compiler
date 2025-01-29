use std::{ops::Range, path::PathBuf, sync::Arc};

use midenc_hir::{
    cranelift_entity::{packed_option::ReservedValue, PrimaryMap},
    diagnostics::{DiagnosticsHandler, IntoDiagnostic, Report, Severity},
    Ident, Symbol,
};
use wasmparser::{
    types::CoreTypeId, CustomSectionReader, DataKind, ElementItems, ElementKind, Encoding,
    ExternalKind, FuncToValidate, FunctionBody, NameSectionReader, Naming, Operator, Parser,
    Payload, TypeRef, Validator, ValidatorResources,
};

use super::{
    types::{DataSegment, DataSegmentIndex},
    ModuleImport, TableInitialValue,
};
use crate::{
    component::SignatureIndex,
    error::WasmResult,
    module::{
        types::{
            convert_func_type, convert_global_type, convert_table_type, convert_valtype,
            DataSegmentOffset, DefinedFuncIndex, ElemIndex, EntityIndex, EntityType, FuncIndex,
            GlobalIndex, GlobalInit, MemoryIndex, ModuleTypesBuilder, TableIndex, TypeIndex,
            WasmType,
        },
        FuncRefIndex, Module, ModuleType, TableSegment,
    },
    unsupported_diag, WasmTranslationConfig,
};

/// Object containing the standalone environment information.
pub struct ModuleEnvironment<'a, 'data> {
    /// The current module being translated
    result: ParsedModule<'data>,

    /// Intern'd types for this entire translation, shared by all modules.
    types: &'a mut ModuleTypesBuilder,

    /// Wasmparser validator for the current module.
    validator: &'a mut Validator,

    /// Configuration for the translation.
    config: &'a WasmTranslationConfig,
}

/// The result of translating via `ModuleEnvironment`. Function bodies are not
/// yet translated, and data initializers have not yet been copied out of the
/// original buffer.
#[derive(Default)]
pub struct ParsedModule<'data> {
    /// Module information.
    pub module: Module,

    /// References to the function bodies.
    pub function_body_inputs: PrimaryMap<DefinedFuncIndex, FunctionBodyData<'data>>,

    /// A list of type signatures which are considered exported from this
    /// module, or those that can possibly be called.
    pub exported_signatures: Vec<SignatureIndex>,

    /// Metadata about the source Wasm file
    pub wasm_file: WasmFileInfo,

    /// DWARF debug information, if enabled, parsed from the module.
    pub debuginfo: DebugInfoData<'data>,

    /// Set if debuginfo was found but it was not parsed due to `Tunables`
    /// configuration.
    pub has_unparsed_debuginfo: bool,

    /// List of data segments found in this module
    pub data_segments: PrimaryMap<DataSegmentIndex, DataSegment<'data>>,

    /// When we're parsing the code section this will be incremented so we know
    /// which function is currently being defined.
    code_index: u32,
}

/// Contains function data: byte code and its offset in the module.
pub struct FunctionBodyData<'a> {
    /// The body of the function, containing code and locals.
    pub body: FunctionBody<'a>,
    /// Validator for the function body
    pub validator: FuncToValidate<ValidatorResources>,
}

#[derive(Default)]
pub struct DebugInfoData<'a> {
    pub dwarf: Dwarf<'a>,
    debug_loc: gimli::DebugLoc<DwarfReader<'a>>,
    debug_loclists: gimli::DebugLocLists<DwarfReader<'a>>,
    pub debug_ranges: gimli::DebugRanges<DwarfReader<'a>>,
    pub debug_rnglists: gimli::DebugRngLists<DwarfReader<'a>>,
}

pub type Dwarf<'input> = gimli::Dwarf<DwarfReader<'input>>;

pub type DwarfReader<'input> = gimli::EndianSlice<'input, gimli::LittleEndian>;

#[derive(Debug, Default)]
pub struct WasmFileInfo {
    pub path: Option<PathBuf>,
    pub code_section_offset: u64,
    pub imported_func_count: u32,
    pub funcs: Vec<FunctionMetadata>,
}

#[derive(Debug)]
pub struct FunctionMetadata {
    pub params: Box<[WasmType]>,
    pub locals: Box<[(u32, WasmType)]>,
}

impl<'a, 'data> ModuleEnvironment<'a, 'data> {
    /// Allocates the environment data structures.
    pub fn new(
        config: &'a WasmTranslationConfig,
        validator: &'a mut Validator,
        types: &'a mut ModuleTypesBuilder,
    ) -> Self {
        Self {
            result: ParsedModule::default(),
            types,
            config,
            validator,
        }
    }

    /// Parse a wasm module using this environment.
    ///
    /// This function will parse the `data` provided with `parser`,
    /// validating everything along the way with this environment's validator.
    ///
    /// The result of parsing, [`ParsedModule`], contains everything
    /// necessary to translate functions afterwards
    pub fn parse(
        mut self,
        parser: Parser,
        data: &'data [u8],
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<ParsedModule<'data>> {
        for payload in parser.parse_all(data) {
            self.parse_payload(payload.into_diagnostic()?, diagnostics)?;
        }
        Ok(self.result)
    }

    /// Parses a single payload from the wasm module.
    fn parse_payload(
        &mut self,
        payload: Payload<'data>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<()> {
        match payload {
            Payload::Version {
                num,
                encoding,
                range,
            } => {
                self.validator.version(num, encoding, &range).into_diagnostic()?;
                match encoding {
                    Encoding::Module => {}
                    Encoding::Component => {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("wasm error: component model is not supported")
                            .into_report());
                    }
                }
            }
            Payload::End(offset) => self.payload_end(offset)?,
            Payload::TypeSection(types) => self.type_section(types)?,
            Payload::ImportSection(imports) => self.import_section(imports)?,
            Payload::FunctionSection(functions) => self.function_section(functions)?,
            Payload::TableSection(tables) => self.table_section(tables, diagnostics)?,
            Payload::MemorySection(memories) => self.memory_section(memories)?,
            Payload::TagSection(tags) => {
                self.validator.tag_section(&tags).into_diagnostic()?;
                // This feature isn't enabled at this time, so we should
                // never get here.
                unreachable!();
            }
            Payload::GlobalSection(globals) => self.global_section(globals, diagnostics)?,
            Payload::ExportSection(exports) => self.export_section(exports)?,
            Payload::StartSection { func, range } => self.start_section(func, range)?,
            Payload::ElementSection(elements) => self.element_section(elements, diagnostics)?,
            Payload::CodeSectionStart { count, range, .. } => {
                self.code_section_start(count, range)?
            }
            Payload::CodeSectionEntry(body) => self.code_section_entry(body)?,
            Payload::DataSection(data) => self.data_section(data, diagnostics)?,
            Payload::DataCountSection { count, range } => {
                self.validator.data_count_section(count, &range).into_diagnostic()?;
                // Note: the count passed in here is the *total* segment count
                // There is no way to reserve for just the passive segments as
                // they are discovered when iterating the data section entries
                // Given that the total segment count might be much larger than
                // the passive count, do not reserve anything here.
            }
            Payload::CustomSection(s) if s.name() == "name" => {
                let reader = wasmparser::BinaryReader::new(
                    s.data(),
                    s.data_offset(),
                    *self.validator.features(),
                );
                let result = self.name_section(NameSectionReader::new(reader));
                if let Err(e) = result {
                    log::warn!("failed to parse name section {:?}", e);
                }
            }
            Payload::CustomSection(s) => self.dwarf_section(&s),
            // It's expected that validation will probably reject other
            // payloads such as `UnknownSection` or those related to the
            // component model.
            other => {
                self.validator.payload(&other).into_diagnostic()?;
                unsupported_diag!(diagnostics, "wasm error: unsupported section {:?}", other);
            }
        }
        Ok(())
    }

    fn payload_end(&mut self, offset: usize) -> Result<(), Report> {
        self.validator.end(offset).into_diagnostic()?;
        self.result.exported_signatures = self
            .result
            .module
            .functions
            .iter()
            .filter_map(|(_, func)| {
                if func.is_escaping() {
                    Some(func.signature)
                } else {
                    None
                }
            })
            .collect();
        self.result.exported_signatures.sort_unstable();
        self.result.exported_signatures.dedup();
        Ok(())
    }

    fn type_section(&mut self, types: wasmparser::TypeSectionReader<'data>) -> Result<(), Report> {
        self.validator.type_section(&types).into_diagnostic()?;
        let num = usize::try_from(types.count()).unwrap();
        self.result.module.types.reserve(num);
        self.types.reserve_wasm_signatures(num);
        for i in 0..types.count() {
            let types = self.validator.types(0).unwrap();
            let ty = types.core_type_at(i);
            self.declare_type(ty.unwrap_sub())?;
        }
        Ok(())
    }

    fn import_section(
        &mut self,
        imports: wasmparser::ImportSectionReader<'data>,
    ) -> Result<(), Report> {
        self.validator.import_section(&imports).into_diagnostic()?;
        let cnt = usize::try_from(imports.count()).unwrap();
        self.result.module.imports.reserve(cnt);
        for entry in imports {
            let import = entry.into_diagnostic()?;
            let ty = match import.ty {
                TypeRef::Func(index) => {
                    let index = TypeIndex::from_u32(index);
                    let sig_index = self.result.module.types[index].unwrap_function();
                    self.result.module.num_imported_funcs += 1;
                    self.result.wasm_file.imported_func_count += 1;
                    EntityType::Function(sig_index)
                }
                TypeRef::Memory(ty) => {
                    self.result.module.memories.push(super::Memory {
                        minimum: ty.initial,
                        maximum: ty.maximum,
                        imported: true,
                    });
                    EntityType::Memory(ty.into())
                }
                TypeRef::Global(ty) => {
                    self.result.module.num_imported_globals += 1;
                    EntityType::Global(convert_global_type(&ty))
                }
                TypeRef::Table(ty) => {
                    self.result.module.num_imported_tables += 1;
                    EntityType::Table(convert_table_type(&ty))
                }

                // doesn't get past validation
                TypeRef::Tag(_) => unreachable!(),
            };
            self.declare_import(import.module, import.name, ty);
        }
        Ok(())
    }

    fn function_section(
        &mut self,
        functions: wasmparser::FunctionSectionReader<'data>,
    ) -> Result<(), Report> {
        self.validator.function_section(&functions).into_diagnostic()?;
        let cnt = usize::try_from(functions.count()).unwrap();
        self.result.module.functions.reserve_exact(cnt);
        for entry in functions {
            let sigindex = entry.into_diagnostic()?;
            let ty = TypeIndex::from_u32(sigindex);
            let sig_index = self.result.module.types[ty].unwrap_function();
            self.result.module.push_function(sig_index);
        }
        Ok(())
    }

    fn table_section(
        &mut self,
        tables: wasmparser::TableSectionReader<'data>,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        self.validator.table_section(&tables).into_diagnostic()?;
        let cnt = usize::try_from(tables.count()).unwrap();
        self.result.module.tables.reserve_exact(cnt);
        for entry in tables {
            let wasmparser::Table { ty, init } = entry.into_diagnostic()?;
            let table = convert_table_type(&ty);
            self.result.module.tables.push(table);
            let init = match init {
                wasmparser::TableInit::RefNull => TableInitialValue::Null {
                    precomputed: Vec::new(),
                },
                wasmparser::TableInit::Expr(cexpr) => {
                    let mut init_expr_reader = cexpr.get_binary_reader();
                    match init_expr_reader.read_operator().into_diagnostic()? {
                        Operator::RefNull { hty: _ } => TableInitialValue::Null {
                            precomputed: Vec::new(),
                        },
                        Operator::RefFunc { function_index } => {
                            let index = FuncIndex::from_u32(function_index);
                            self.flag_func_escaped(index);
                            TableInitialValue::FuncRef(index)
                        }
                        s => {
                            unsupported_diag!(
                                diagnostics,
                                "wasm error: unsupported init expr in table section: {:?}",
                                s
                            );
                        }
                    }
                }
            };
            self.result.module.table_initialization.initial_values.push(init);
        }
        Ok(())
    }

    fn memory_section(
        &mut self,
        memories: wasmparser::MemorySectionReader<'data>,
    ) -> Result<(), Report> {
        self.validator.memory_section(&memories).into_diagnostic()?;
        let cnt = usize::try_from(memories.count()).unwrap();
        assert_eq!(cnt, 1, "only one memory per module is supported");
        for memory in memories {
            let memory = memory.into_diagnostic()?;
            self.result.module.memories.push(memory.into());
        }
        Ok(())
    }

    fn global_section(
        &mut self,
        globals: wasmparser::GlobalSectionReader<'data>,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        self.validator.global_section(&globals).into_diagnostic()?;
        let cnt = usize::try_from(globals.count()).unwrap();
        self.result.module.globals.reserve_exact(cnt);
        for entry in globals {
            let wasmparser::Global { ty, init_expr } = entry.into_diagnostic()?;
            let mut init_expr_reader = init_expr.get_binary_reader();
            let initializer = match init_expr_reader.read_operator().into_diagnostic()? {
                Operator::I32Const { value } => GlobalInit::I32Const(value),
                Operator::I64Const { value } => GlobalInit::I64Const(value),
                Operator::F32Const { value } => GlobalInit::F32Const(value.bits()),
                Operator::F64Const { value } => GlobalInit::F64Const(value.bits()),
                Operator::V128Const { value } => {
                    GlobalInit::V128Const(u128::from_le_bytes(*value.bytes()))
                }
                Operator::GlobalGet { global_index } => {
                    GlobalInit::GetGlobal(GlobalIndex::from_u32(global_index))
                }
                s => {
                    unsupported_diag!(
                        diagnostics,
                        "wasm error: unsupported init expr in global section: {:?}",
                        s
                    );
                }
            };
            let ty = convert_global_type(&ty);
            self.result.module.globals.push(ty);
            self.result.module.global_initializers.push(initializer);
        }
        Ok(())
    }

    fn export_section(
        &mut self,
        exports: wasmparser::ExportSectionReader<'data>,
    ) -> Result<(), Report> {
        self.validator.export_section(&exports).into_diagnostic()?;
        let cnt = usize::try_from(exports.count()).unwrap();
        self.result.module.exports.reserve(cnt);
        for entry in exports {
            let wasmparser::Export { name, kind, index } = entry.into_diagnostic()?;
            let entity = match kind {
                ExternalKind::Func => {
                    let index = FuncIndex::from_u32(index);
                    self.flag_func_escaped(index);
                    EntityIndex::Function(index)
                }
                ExternalKind::Table => EntityIndex::Table(TableIndex::from_u32(index)),
                ExternalKind::Memory => EntityIndex::Memory(MemoryIndex::from_u32(index)),
                ExternalKind::Global => EntityIndex::Global(GlobalIndex::from_u32(index)),

                // this never gets past validation
                ExternalKind::Tag => unreachable!(),
            };
            self.result.module.exports.insert(String::from(name), entity);
        }
        Ok(())
    }

    fn start_section(&mut self, func: u32, range: Range<usize>) -> Result<(), Report> {
        self.validator.start_section(func, &range).into_diagnostic()?;
        let func_index = FuncIndex::from_u32(func);
        self.flag_func_escaped(func_index);
        debug_assert!(self.result.module.start_func.is_none());
        self.result.module.start_func = Some(func_index);
        Ok(())
    }

    fn element_section(
        &mut self,
        elements: wasmparser::ElementSectionReader<'data>,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        self.validator.element_section(&elements).into_diagnostic()?;
        for (index, entry) in elements.into_iter().enumerate() {
            let wasmparser::Element {
                kind,
                items,
                range: _,
            } = entry.into_diagnostic()?;

            // Build up a list of `FuncIndex` corresponding to all the
            // entries listed in this segment. Note that it's not
            // possible to create anything other than a `ref.null
            // extern` for externref segments, so those just get
            // translated to the reserved value of `FuncIndex`.
            let mut elements = Vec::new();
            match items {
                ElementItems::Functions(funcs) => {
                    elements.reserve(usize::try_from(funcs.count()).unwrap());
                    for func in funcs {
                        let func = FuncIndex::from_u32(func.into_diagnostic()?);
                        self.flag_func_escaped(func);
                        elements.push(func);
                    }
                }
                ElementItems::Expressions(_ty, funcs) => {
                    elements.reserve(usize::try_from(funcs.count()).unwrap());
                    for func in funcs {
                        let func = match func
                            .into_diagnostic()?
                            .get_binary_reader()
                            .read_operator()
                            .into_diagnostic()?
                        {
                            Operator::RefNull { .. } => FuncIndex::reserved_value(),
                            Operator::RefFunc { function_index } => {
                                let func = FuncIndex::from_u32(function_index);
                                self.flag_func_escaped(func);
                                func
                            }
                            s => {
                                unsupported_diag!(
                                    diagnostics,
                                    "wasm error: unsupported init expr in element section: {:?}",
                                    s
                                );
                            }
                        };
                        elements.push(func);
                    }
                }
            }

            match kind {
                ElementKind::Active {
                    table_index,
                    offset_expr,
                } => {
                    let table_index = TableIndex::from_u32(table_index.unwrap_or(0));
                    let mut offset_expr_reader = offset_expr.get_binary_reader();
                    let (base, offset) =
                        match offset_expr_reader.read_operator().into_diagnostic()? {
                            Operator::I32Const { value } => (None, value as u32),
                            Operator::GlobalGet { global_index } => {
                                (Some(GlobalIndex::from_u32(global_index)), 0)
                            }
                            ref s => {
                                unsupported_diag!(
                                    diagnostics,
                                    "wasm error: unsupported init expr in element section: {:?}",
                                    s
                                );
                            }
                        };

                    self.result.module.table_initialization.segments.push(TableSegment {
                        table_index,
                        base,
                        offset,
                        elements: elements.into(),
                    });
                }

                ElementKind::Passive => {
                    let elem_index = ElemIndex::from_u32(index as u32);
                    let index = self.result.module.passive_elements.len();
                    self.result.module.passive_elements.push(elements.into());
                    self.result.module.passive_elements_map.insert(elem_index, index);
                }

                ElementKind::Declared => {}
            }
        }
        Ok(())
    }

    fn code_section_start(&mut self, count: u32, range: Range<usize>) -> Result<(), Report> {
        self.validator.code_section_start(count, &range).into_diagnostic()?;
        let cnt = usize::try_from(count).unwrap();
        self.result.function_body_inputs.reserve_exact(cnt);
        self.result.wasm_file.code_section_offset = range.start as u64;
        Ok(())
    }

    fn code_section_entry(&mut self, body: FunctionBody<'data>) -> Result<(), Report> {
        let validator = self.validator.code_section_entry(&body).into_diagnostic()?;
        let func_index = self.result.code_index + self.result.module.num_imported_funcs as u32;
        let func_index = FuncIndex::from_u32(func_index);
        if self.config.generate_native_debuginfo {
            let sig_index = self.result.module.functions[func_index].signature;
            let sig = &self.types[sig_index];
            let mut locals = Vec::new();
            for pair in body.get_locals_reader().into_diagnostic()? {
                let (cnt, ty) = pair.into_diagnostic()?;
                let ty = convert_valtype(ty);
                locals.push((cnt, ty));
            }
            self.result.wasm_file.funcs.push(FunctionMetadata {
                locals: locals.into_boxed_slice(),
                params: sig.params().into(),
            });
        }
        self.result.function_body_inputs.push(FunctionBodyData { validator, body });
        self.result.code_index += 1;
        Ok(())
    }

    fn data_section(
        &mut self,
        data_section: wasmparser::DataSectionReader<'data>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<()> {
        self.validator.data_section(&data_section).into_diagnostic()?;
        let cnt = usize::try_from(data_section.count()).unwrap();
        self.result.data_segments.reserve_exact(cnt);
        for entry in data_section.into_iter() {
            let wasmparser::Data {
                kind,
                data,
                range: _,
            } = entry.into_diagnostic()?;
            match kind {
                DataKind::Active {
                    memory_index,
                    offset_expr,
                } => {
                    assert_eq!(
                        memory_index, 0,
                        "data section memory index must be 0 (only one memory per module is \
                         supported)"
                    );
                    let mut offset_expr_reader = offset_expr.get_binary_reader();
                    let offset = match offset_expr_reader.read_operator().into_diagnostic()? {
                        Operator::I32Const { value } => DataSegmentOffset::I32Const(value),
                        Operator::GlobalGet { global_index } => {
                            DataSegmentOffset::GetGlobal(GlobalIndex::from_u32(global_index))
                        }
                        ref s => {
                            unsupported_diag!(
                                diagnostics,
                                "wasm error: unsupported init expr in data section offset: {:?}",
                                s
                            );
                        }
                    };
                    let segment = DataSegment { offset, data };
                    self.result.data_segments.push(segment);
                }
                DataKind::Passive => {
                    unsupported_diag!(
                        diagnostics,
                        "wasm error: unsupported passive data segment in data section"
                    );
                }
            }
        }
        Ok(())
    }

    /// Parses the Name section of the wasm module.
    fn name_section(&mut self, names: NameSectionReader<'data>) -> WasmResult<()> {
        for subsection in names {
            match subsection.into_diagnostic()? {
                wasmparser::Name::Function(names) => {
                    for name in names {
                        let Naming { index, name } = name.into_diagnostic()?;
                        // Skip this naming if it's naming a function that
                        // doesn't actually exist.
                        if (index as usize) >= self.result.module.functions.len() {
                            continue;
                        }

                        // Store the name unconditionally, regardless of
                        // whether we're parsing debuginfo, since function
                        // names are almost always present in the
                        // final compilation artifact.
                        let index = FuncIndex::from_u32(index);
                        self.result
                            .module
                            .name_section
                            .func_names
                            .insert(index, Symbol::intern(name));
                    }
                }
                wasmparser::Name::Module { name, .. } => {
                    // Module name ends with `.wasm`,
                    // remove it if its there since MASM doesn't yet allow dots in the module name
                    let name = name.trim_end_matches(".wasm");
                    self.result.module.name_section.module_name = Some(Ident::from(name));
                }
                wasmparser::Name::Local(reader) => {
                    if !self.config.generate_native_debuginfo {
                        continue;
                    }
                    for f in reader {
                        let f = f.into_diagnostic()?;
                        // Skip this naming if it's naming a function that
                        // doesn't actually exist.
                        if (f.index as usize) >= self.result.module.functions.len() {
                            continue;
                        }
                        for name in f.names {
                            let Naming { index, name } = name.into_diagnostic()?;

                            self.result
                                .module
                                .name_section
                                .locals_names
                                .entry(FuncIndex::from_u32(f.index))
                                .or_default()
                                .insert(index, Symbol::intern(name));
                        }
                    }
                }
                wasmparser::Name::Global(names) => {
                    for name in names {
                        let Naming { index, name } = name.into_diagnostic()?;
                        if index != u32::MAX {
                            self.result
                                .module
                                .name_section
                                .globals_names
                                .insert(GlobalIndex::from_u32(index), Symbol::intern(name));
                        }
                    }
                }
                wasmparser::Name::Data(names) => {
                    for name in names {
                        let Naming { index, name } = name.into_diagnostic()?;
                        if index != u32::MAX {
                            self.result
                                .module
                                .name_section
                                .data_segment_names
                                .insert(DataSegmentIndex::from_u32(index), Symbol::intern(name));
                        }
                    }
                }
                wasmparser::Name::Label(_)
                | wasmparser::Name::Type(_)
                | wasmparser::Name::Table(_)
                | wasmparser::Name::Memory(_)
                | wasmparser::Name::Element(_)
                | wasmparser::Name::Field(_)
                | wasmparser::Name::Tag(_)
                | wasmparser::Name::Unknown { .. } => {}
            }
        }
        Ok(())
    }

    fn dwarf_section(&mut self, section: &CustomSectionReader<'data>) {
        let name = section.name();
        if !name.starts_with(".debug_") {
            return;
        }
        if !self.config.generate_native_debuginfo && !self.config.parse_wasm_debuginfo {
            self.result.has_unparsed_debuginfo = true;
            return;
        }
        let info = &mut self.result.debuginfo;
        let dwarf = &mut info.dwarf;
        let endian = gimli::LittleEndian;
        let data = section.data();
        let slice = gimli::EndianSlice::new(data, endian);

        match name {
            // `gimli::Dwarf` fields.
            ".debug_abbrev" => dwarf.debug_abbrev = gimli::DebugAbbrev::new(data, endian),
            ".debug_addr" => dwarf.debug_addr = gimli::DebugAddr::from(slice),
            ".debug_info" => dwarf.debug_info = gimli::DebugInfo::new(data, endian),
            ".debug_line" => dwarf.debug_line = gimli::DebugLine::new(data, endian),
            ".debug_line_str" => dwarf.debug_line_str = gimli::DebugLineStr::from(slice),
            ".debug_str" => dwarf.debug_str = gimli::DebugStr::new(data, endian),
            ".debug_str_offsets" => dwarf.debug_str_offsets = gimli::DebugStrOffsets::from(slice),
            ".debug_str_sup" => {
                let dwarf_sup: Dwarf<'data> = Dwarf {
                    debug_str: gimli::DebugStr::from(slice),
                    ..Default::default()
                };
                dwarf.sup = Some(Arc::new(dwarf_sup));
            }
            ".debug_types" => dwarf.debug_types = gimli::DebugTypes::from(slice),

            // Additional fields.
            ".debug_loc" => info.debug_loc = gimli::DebugLoc::from(slice),
            ".debug_loclists" => info.debug_loclists = gimli::DebugLocLists::from(slice),
            ".debug_ranges" => info.debug_ranges = gimli::DebugRanges::new(data, endian),
            ".debug_rnglists" => info.debug_rnglists = gimli::DebugRngLists::new(data, endian),

            // We don't use these at the moment.
            ".debug_aranges" | ".debug_pubnames" | ".debug_pubtypes" => return,

            other => {
                log::warn!("unknown debug section `{}`", other);
                return;
            }
        }

        dwarf.ranges = gimli::RangeLists::new(info.debug_ranges, info.debug_rnglists);
        dwarf.locations = gimli::LocationLists::new(info.debug_loc, info.debug_loclists);
    }

    /// Declares a new import with the `module` and `field` names, importing the
    /// `ty` specified.
    fn declare_import(&mut self, module: &'data str, field: &'data str, ty: EntityType) {
        let index = self.push_type(ty);
        self.result.module.imports.push(ModuleImport {
            module: module.to_owned(),
            field: field.to_owned(),
            index,
        });
    }

    fn push_type(&mut self, ty: EntityType) -> EntityIndex {
        match ty {
            EntityType::Function(ty) => EntityIndex::Function(self.result.module.push_function(ty)),
            EntityType::Table(ty) => EntityIndex::Table(self.result.module.tables.push(ty)),
            EntityType::Memory(ty) => EntityIndex::Memory(self.result.module.memories.push(ty)),
            EntityType::Global(ty) => EntityIndex::Global(self.result.module.globals.push(ty)),
        }
    }

    fn flag_func_escaped(&mut self, func: FuncIndex) {
        let ty = &mut self.result.module.functions[func];
        // If this was already assigned a funcref index no need to re-assign it.
        if ty.is_escaping() {
            return;
        }
        let index = self.result.module.num_escaped_funcs as u32;
        ty.func_ref = FuncRefIndex::from_u32(index);
        self.result.module.num_escaped_funcs += 1;
    }

    fn declare_type(&mut self, id: CoreTypeId) -> WasmResult<()> {
        use wasmparser::CompositeInnerType;

        let types = self.validator.types(0).unwrap();
        let ty = &types[id];
        assert!(ty.is_final);
        assert!(ty.supertype_idx.is_none());
        match &ty.composite_type.inner {
            CompositeInnerType::Func(ty) => {
                let wasm = convert_func_type(ty);
                let sig_index = self.types.wasm_func_type(id, wasm);
                self.result.module.types.push(ModuleType::Function(sig_index));
            }
            CompositeInnerType::Array(_) | CompositeInnerType::Struct(_) => unimplemented!(),
        }
        Ok(())
    }
}
