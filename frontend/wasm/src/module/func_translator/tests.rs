use gimli::write::{
    Address, AttributeValue, DebugInfoRef, Dwarf, EndianVec, LineProgram, LineString, Sections,
    Unit,
};

use super::*;

/// Two expansions of one abstract function, each spanning two instruction locations.
/// Only the instruction/call-site source exists locally; the declaration source does not.
fn inline_dwarf() -> Dwarf {
    inline_dwarf_version(4)
}

fn inline_dwarf_version(version: u16) -> Dwarf {
    let encoding = gimli::Encoding {
        format: gimli::Format::Dwarf32,
        version,
        address_size: 4,
    };
    let directory = env!("CARGO_MANIFEST_DIR");
    let mut program = LineProgram::new(
        encoding,
        gimli::LineEncoding::default(),
        LineString::String(directory.as_bytes().to_vec()),
        None,
        LineString::String(b"src/module/func_translator.rs".to_vec()),
        None,
    );
    let source = program.add_file(
        LineString::String(b"src/module/func_translator.rs".to_vec()),
        program.default_directory(),
        None,
    );
    let declaration = program.add_file(
        LineString::String(b"unavailable/inline.rs".to_vec()),
        program.default_directory(),
        None,
    );
    program.begin_sequence(Some(Address::Constant(0x1000)));
    for (offset, line) in [(0, 10), (1, 11), (16, 12), (17, 13)] {
        program.row().address_offset = offset;
        program.row().file = source;
        program.row().line = line;
        program.row().column = 1;
        program.generate_row();
    }
    program.end_sequence(32);

    let mut unit = Unit::new(encoding, program);
    let root = unit.root();
    unit.get_mut(root)
        .set(gimli::DW_AT_comp_dir, AttributeValue::String(directory.into()));
    unit.get_mut(root).set(gimli::DW_AT_stmt_list, AttributeValue::LineProgramRef);
    unit.get_mut(root)
        .set(gimli::DW_AT_low_pc, AttributeValue::Address(Address::Constant(0x1000)));
    unit.get_mut(root).set(gimli::DW_AT_high_pc, AttributeValue::Udata(32));

    let origin = unit.add(root, gimli::DW_TAG_subprogram);
    unit.get_mut(origin)
        .set(gimli::DW_AT_name, AttributeValue::String(b"inlined".to_vec()));
    unit.get_mut(origin)
        .set(gimli::DW_AT_decl_file, AttributeValue::FileIndex(Some(declaration)));
    unit.get_mut(origin).set(gimli::DW_AT_decl_line, AttributeValue::Udata(7));
    unit.get_mut(origin).set(gimli::DW_AT_decl_column, AttributeValue::Udata(3));

    let physical = unit.add(root, gimli::DW_TAG_subprogram);
    unit.get_mut(physical)
        .set(gimli::DW_AT_name, AttributeValue::String(b"physical".to_vec()));
    unit.get_mut(physical)
        .set(gimli::DW_AT_low_pc, AttributeValue::Address(Address::Constant(0x1000)));
    unit.get_mut(physical).set(gimli::DW_AT_high_pc, AttributeValue::Udata(32));
    for (address, call_line) in [(0x1000, 20), (0x1010, 30)] {
        let inline = unit.add(physical, gimli::DW_TAG_inlined_subroutine);
        let entry = unit.get_mut(inline);
        entry.set(gimli::DW_AT_abstract_origin, AttributeValue::UnitRef(origin));
        entry.set(gimli::DW_AT_low_pc, AttributeValue::Address(Address::Constant(address)));
        entry.set(gimli::DW_AT_high_pc, AttributeValue::Udata(2));
        entry.set(gimli::DW_AT_call_file, AttributeValue::FileIndex(Some(source)));
        entry.set(gimli::DW_AT_call_line, AttributeValue::Udata(call_line));
        entry.set(gimli::DW_AT_call_column, AttributeValue::Udata(1));
    }
    let mut dwarf = Dwarf::new();
    dwarf.units.add(unit);
    dwarf
}

fn write_dwarf(mut dwarf: Dwarf) -> Sections<EndianVec<gimli::LittleEndian>> {
    let mut sections = Sections::new(EndianVec::new(gimli::LittleEndian));
    dwarf.write(&mut sections).unwrap();
    sections
}

fn read_dwarf(
    sections: &Sections<EndianVec<gimli::LittleEndian>>,
) -> gimli::Dwarf<DwarfReader<'_>> {
    gimli::Dwarf::load(|id| {
        Ok::<_, gimli::Error>(gimli::EndianSlice::new(
            sections.get(id).map(|section| section.slice()).unwrap_or_default(),
            gimli::LittleEndian,
        ))
    })
    .unwrap()
}

fn resolve_inline(dwarf: Dwarf, config: &crate::WasmTranslationConfig) -> InlineCallFrame {
    let sections = write_dwarf(dwarf);
    let addr2line = addr2line::Context::from_dwarf(read_dwarf(&sections)).unwrap();
    let context = Context::default();
    let mut resolved =
        resolve_instruction_debug_context(&addr2line, 0x1000, context.session(), config).unwrap();
    assert_eq!(resolved.inline_calls.len(), 1);
    resolved.inline_calls.remove(0)
}

#[test]
fn inline_declaration_inherits_missing_fields_from_another_unit() {
    let mut dwarf = inline_dwarf();
    let (caller_unit, unit) = dwarf.units.iter().next().unwrap();
    let origin = *unit.get(unit.root()).children().next().unwrap();
    let encoding = unit.encoding();
    let mut program = LineProgram::new(
        encoding,
        gimli::LineEncoding::default(),
        LineString::String(b"/not-installed".to_vec()),
        None,
        LineString::String(b"declarations.rs".to_vec()),
        None,
    );
    let file = program.add_file(
        LineString::String(b"declarations.rs".to_vec()),
        program.default_directory(),
        None,
    );
    let mut declarations = Unit::new(encoding, program);
    let root = declarations.root();
    declarations
        .get_mut(root)
        .set(gimli::DW_AT_comp_dir, AttributeValue::String(b"/not-installed".to_vec()));
    let specification = declarations.add(root, gimli::DW_TAG_subprogram);
    let entry = declarations.get_mut(specification);
    entry.set(gimli::DW_AT_decl_file, AttributeValue::FileIndex(Some(file)));
    entry.set(gimli::DW_AT_decl_line, AttributeValue::Udata(42));
    entry.set(gimli::DW_AT_decl_column, AttributeValue::Udata(9));
    let declaration_unit = dwarf.units.add(declarations);

    let origin = dwarf.units.get_mut(caller_unit).get_mut(origin);
    origin.delete(gimli::DW_AT_decl_file);
    origin.delete(gimli::DW_AT_decl_column);
    // The concrete line wins, while file and column come from the specification's unit.
    origin.set(
        gimli::DW_AT_specification,
        AttributeValue::DebugInfoRef(DebugInfoRef::Entry(declaration_unit, specification)),
    );
    let inline = resolve_inline(dwarf, &crate::WasmTranslationConfig::default());
    assert_eq!(inline.file.as_str(), "/not-installed/declarations.rs");
    assert_eq!((inline.line, inline.column), (7, 9));
}

#[test]
fn inline_declaration_keeps_missing_coordinates_unknown() {
    let mut dwarf = inline_dwarf();
    let (_, unit) = dwarf.units.iter_mut().next().unwrap();
    let origin = *unit.get(unit.root()).children().next().unwrap();
    for attr in [gimli::DW_AT_decl_file, gimli::DW_AT_decl_line, gimli::DW_AT_decl_column] {
        unit.get_mut(origin).delete(attr);
    }
    let inline = resolve_inline(dwarf, &crate::WasmTranslationConfig::default());
    assert_eq!(inline.file.as_str(), "");
    assert_eq!((inline.line, inline.column), (0, 0));
    assert_eq!(inline.call_line, 20);
}

#[test]
fn inline_declaration_reference_cycles_preserve_known_fields() {
    let mut dwarf = inline_dwarf();
    let (_, unit) = dwarf.units.iter_mut().next().unwrap();
    let origin = *unit.get(unit.root()).children().next().unwrap();
    unit.get_mut(origin).delete(gimli::DW_AT_decl_column);
    unit.get_mut(origin)
        .set(gimli::DW_AT_specification, AttributeValue::UnitRef(origin));
    let inline = resolve_inline(dwarf, &crate::WasmTranslationConfig::default());
    assert_eq!(inline.line, 7);
    assert_eq!(inline.column, 0);
}

#[test]
fn inline_declaration_remaps_paths_without_loading_source() {
    let mut config = crate::WasmTranslationConfig::default();
    config.remap_path_prefixes.push(midenc_session::RemapPathPrefix {
        from: PathBuf::from(env!("CARGO_MANIFEST_DIR")).into_boxed_path(),
        to: Some(PathBuf::from("source").into_boxed_path()),
    });
    let inline = resolve_inline(inline_dwarf(), &config);
    assert_eq!(inline.file.as_str(), "source/unavailable/inline.rs");
    assert_eq!((inline.line, inline.column), (7, 3));
    assert_eq!(inline.call_file.as_str(), "source/src/module/func_translator.rs");
}

#[test]
fn inline_declaration_resolves_dwarf5_file_indices() {
    let inline = resolve_inline(inline_dwarf_version(5), &crate::WasmTranslationConfig::default());
    assert_eq!(
        inline.file.as_str(),
        concat!(env!("CARGO_MANIFEST_DIR"), "/unavailable/inline.rs")
    );
    assert_eq!((inline.line, inline.column), (7, 3));
}

#[test]
fn inline_declaration_zero_file_index_is_unknown_before_dwarf5() {
    for (version, expected_file) in [
        (4, ""),
        (5, concat!(env!("CARGO_MANIFEST_DIR"), "/src/module/func_translator.rs")),
    ] {
        let mut dwarf = inline_dwarf_version(version);
        let (_, unit) = dwarf.units.iter_mut().next().unwrap();
        let root = unit.root();
        unit.get_mut(root).set(
            gimli::DW_AT_name,
            AttributeValue::String(b"src/module/func_translator.rs".to_vec()),
        );
        let physical = *unit.get(root).children().nth(1).unwrap();
        let inline = *unit.get(physical).children().next().unwrap();
        unit.get_mut(inline)
            .set(gimli::DW_AT_decl_file, AttributeValue::FileIndex(None));
        let inline = resolve_inline(dwarf, &crate::WasmTranslationConfig::default());
        assert_eq!(inline.file.as_str(), expected_file, "DWARF version {version}");
    }
}

#[test]
fn inline_declaration_inherits_from_supplementary_dwarf() {
    let mut supplementary = inline_dwarf();
    let (_, unit) = supplementary.units.iter_mut().next().unwrap();
    let root = unit.root();
    unit.get_mut(root)
        .set(gimli::DW_AT_comp_dir, AttributeValue::String(b"/supplementary".to_vec()));
    let sections = write_dwarf(supplementary);
    let supplementary = read_dwarf(&sections);
    let unit = supplementary.unit(supplementary.units().next().unwrap().unwrap()).unwrap();
    let mut entries = unit.entries();
    entries.next_dfs().unwrap().unwrap();
    let origin = entries.next_dfs().unwrap().unwrap();
    let reference = origin.offset().to_debug_info_offset(&unit.header).unwrap();

    let mut primary = inline_dwarf();
    let (_, unit) = primary.units.iter_mut().next().unwrap();
    let origin = *unit.get(unit.root()).children().next().unwrap();
    unit.get_mut(origin).delete(gimli::DW_AT_decl_file);
    unit.get_mut(origin).delete(gimli::DW_AT_decl_column);
    unit.get_mut(origin)
        .set(gimli::DW_AT_specification, AttributeValue::DebugInfoRefSup(reference));
    let primary_sections = write_dwarf(primary);
    let mut primary = read_dwarf(&primary_sections);
    primary.sup = Some(std::sync::Arc::new(supplementary));
    let addr2line = addr2line::Context::from_dwarf(primary).unwrap();
    let context = Context::default();
    let resolved = resolve_instruction_debug_context(
        &addr2line,
        0x1000,
        context.session(),
        &crate::WasmTranslationConfig::default(),
    )
    .unwrap();
    assert_eq!(resolved.inline_calls.len(), 1);
    let inline = &resolved.inline_calls[0];
    assert_eq!(inline.file.as_str(), "/supplementary/unavailable/inline.rs");
    assert_eq!((inline.line, inline.column), (7, 3));
}

#[test]
fn inline_declaration_is_independent_of_instruction_and_call_site() {
    let sections = write_dwarf(inline_dwarf());
    let addr2line = addr2line::Context::from_dwarf(read_dwarf(&sections)).unwrap();
    let context = Context::default();
    let config = crate::WasmTranslationConfig::default();
    for (offset, call_line) in [(0x1000, 20), (0x1001, 20), (0x1010, 30), (0x1011, 30)] {
        let resolved =
            resolve_instruction_debug_context(&addr2line, offset, context.session(), &config)
                .unwrap();
        assert_eq!(resolved.inline_calls.len(), 1);
        let inline = &resolved.inline_calls[0];
        assert_eq!(inline.name.as_str(), "inlined");
        assert_eq!(
            inline.file.as_str(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/unavailable/inline.rs")
        );
        assert_eq!((inline.line, inline.column), (7, 3));
        assert_eq!(inline.call_line, call_line);
    }
}

#[test]
fn inline_declaration_interns_one_assembled_function_across_call_sites() {
    check_assembled_inline_functions(inline_dwarf(), 1);
}

#[test]
fn unavailable_inline_call_sources_are_compatible_with_current_assembler() {
    let mut dwarf = inline_dwarf();
    let (id, unit) = dwarf.units.iter().next().unwrap();
    let physical = *unit.get(unit.root()).children().nth(1).unwrap();
    let inlines = unit.get(physical).children().copied().collect::<Vec<_>>();
    let unit = dwarf.units.get_mut(id);
    let missing = unit.line_program.add_file(
        LineString::String(b"unavailable/caller.rs".to_vec()),
        unit.line_program.default_directory(),
        None,
    );
    for inline in inlines {
        unit.get_mut(inline)
            .set(gimli::DW_AT_call_file, AttributeValue::FileIndex(Some(missing)));
    }
    // HIR still contains every frame, but assembler 0.33 discards unresolved call sites.
    check_assembled_inline_functions(dwarf, 0);
}

fn check_assembled_inline_functions(dwarf: Dwarf, expected_functions: usize) {
    use std::sync::Arc;

    use midenc_codegen_masm::{ToMasmComponent, masm};
    use midenc_hir::{
        dialects::{
            builtin,
            debuginfo::attributes::{
                INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr,
            },
        },
        pass::AnalysisManager,
    };

    let context = Rc::new(Context::default());
    let source = r#"
        builtin.component private @"inline:test@1.0.0" {
            builtin.module public @test {
                builtin.function public extern("C") @first() { builtin.ret; };
                builtin.function public extern("C") @second() { builtin.ret; };
                builtin.function public extern("C") @third() { builtin.ret; };
                builtin.function public extern("C") @fourth() { builtin.ret; };
            };
        };
    "#;
    let op = midenc_hir::parse::parse_any(
        midenc_hir::parse::ParserConfig {
            context: context.clone(),
            verify: true,
        },
        Uri::new("inline.hir"),
        source,
    )
    .unwrap();
    let mut returns = Vec::new();
    op.borrow().prewalk_all(|op| {
        if op.is::<builtin::Ret>() {
            returns.push(op.as_operation_ref());
        }
    });
    assert_eq!(returns.len(), 4);

    let sections = write_dwarf(dwarf);
    let addr2line = addr2line::Context::from_dwarf(read_dwarf(&sections)).unwrap();
    for (mut ret, offset) in returns.into_iter().zip([0x1000, 0x1001, 0x1010, 0x1011]) {
        let resolved = resolve_instruction_debug_context(
            &addr2line,
            offset,
            context.session(),
            &crate::WasmTranslationConfig::default(),
        )
        .unwrap();
        assert_eq!(resolved.inline_calls.len(), 1);
        let attr = context
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(resolved.inline_calls))
            .as_attribute_ref();
        ret.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, attr);
    }

    let component = op.try_downcast_op::<builtin::Component>().unwrap();
    let lowered = component.borrow().to_masm_component(AnalysisManager::new(op, None)).unwrap();
    let target = midenc_session::miden_project::Target::library(
        Arc::<masm::Path>::from(
            masm::LibraryPath::new("inline_tests")
                .unwrap()
                .to_absolute()
                .unwrap()
                .into_owned()
                .into_boxed_path(),
        ),
        Uri::new("inline.hir"),
    );
    let sources = lowered.source_inputs(&target, context.session()).unwrap();
    let package = miden_assembly::Assembler::new(context.session().source_manager.clone())
        .assemble_library("inline_tests", sources.root, sources.support)
        .unwrap();
    let debug_info = package.debug_info().unwrap().unwrap();
    let functions = debug_info
        .functions()
        .iter()
        .filter(|function| debug_info.get_string(function.name_idx).as_deref() == Some("inlined"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), expected_functions);
    for function in functions {
        assert_eq!(function.line.to_u32(), 6);
        assert_eq!(function.column.to_u32(), 2);
    }
}

fn frame(name: &str, path: &str, line: u32, column: u32) -> ResolvedFrame {
    ResolvedFrame {
        name: name.to_string(),
        linkage_name: Some(format!("_{name}")),
        declaration: FunctionDeclaration {
            file: Some(path.to_owned().into()),
            line: Some(1),
            column: Some(1),
        },
        location: FrameLocation {
            path: PathBuf::from(path),
            line,
            column,
        },
    }
}

#[test]
fn inline_call_chain_uses_caller_locations_as_call_sites() {
    let frames = [
        frame("inner", "src/inner.rs", 30, 7),
        frame("outer", "src/outer.rs", 20, 5),
        frame("physical", "src/lib.rs", 10, 3),
    ];

    let chain = inline_call_chain(&frames);

    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].name.as_str(), "inner");
    assert_eq!(chain[0].file.as_str(), "src/inner.rs");
    assert_eq!(chain[0].call_file.as_str(), "src/outer.rs");
    assert_eq!((chain[0].call_line, chain[0].call_column), (20, 5));
    assert_eq!(chain[1].name.as_str(), "outer");
    assert_eq!(chain[1].call_file.as_str(), "src/lib.rs");
    assert_eq!((chain[1].call_line, chain[1].call_column), (10, 3));
}

#[test]
fn inline_frames_survive_missing_middle_source_and_name() {
    for unknown_line in [false, true] {
        let mut dwarf = inline_dwarf();
        let (id, unit) = dwarf.units.iter().next().unwrap();
        let children = unit.get(unit.root()).children().copied().collect::<Vec<_>>();
        let outer = *unit.get(children[1]).children().next().unwrap();
        let unit = dwarf.units.get_mut(id);
        unit.get_mut(children[0]).delete(gimli::DW_AT_name);
        let missing = unit.line_program.add_file(
            LineString::String(b"unavailable/caller.rs".to_vec()),
            unit.line_program.default_directory(),
            None,
        );
        let inner = unit.add(outer, gimli::DW_TAG_inlined_subroutine);
        let entry = unit.get_mut(inner);
        entry.set(gimli::DW_AT_name, AttributeValue::String(b"inner".to_vec()));
        entry.set(gimli::DW_AT_low_pc, AttributeValue::Address(Address::Constant(0x1000)));
        entry.set(gimli::DW_AT_high_pc, AttributeValue::Udata(1));
        entry.set(gimli::DW_AT_call_file, AttributeValue::FileIndex(Some(missing)));
        entry.set(gimli::DW_AT_call_line, AttributeValue::Udata(if unknown_line { 0 } else { 42 }));
        let sections = write_dwarf(dwarf);
        let addr2line = addr2line::Context::from_dwarf(read_dwarf(&sections)).unwrap();
        let context = Context::default();
        let resolved = resolve_instruction_debug_context(
            &addr2line,
            0x1000,
            context.session(),
            &crate::WasmTranslationConfig::default(),
        )
        .unwrap();
        assert!(!resolved.span.is_unknown());
        assert_eq!(resolved.inline_calls.len(), 2);
        let inner = &resolved.inline_calls[0];
        assert_eq!(inner.name.as_str(), "inner");
        assert!(inner.call_file.as_str().ends_with("unavailable/caller.rs"));
        assert_eq!(inner.call_line, if unknown_line { 0 } else { 42 });
        let outer = &resolved.inline_calls[1];
        assert_eq!(outer.name.as_str(), "<unknown>");
        assert_eq!(outer.call_line, 20);
    }
}

#[test]
fn inline_frame_without_outer_location_is_retained() {
    let mut dwarf = inline_dwarf();
    let (id, unit) = dwarf.units.iter().next().unwrap();
    let physical = *unit.get(unit.root()).children().nth(1).unwrap();
    let inline = *unit.get(physical).children().next().unwrap();
    let entry = dwarf.units.get_mut(id).get_mut(inline);
    entry.delete(gimli::DW_AT_call_file);
    entry.delete(gimli::DW_AT_call_line);
    entry.delete(gimli::DW_AT_call_column);
    let frame = resolve_inline(dwarf, &crate::WasmTranslationConfig::default());
    assert_eq!(frame.name.as_str(), "inlined");
    assert_eq!(frame.call_file.as_str(), "");
    assert_eq!((frame.call_line, frame.call_column), (0, 0));
}

#[test]
fn location_only_record_does_not_create_inline_frames() {
    let mut dwarf = inline_dwarf();
    let (id, unit) = dwarf.units.iter().next().unwrap();
    let physical = *unit.get(unit.root()).children().nth(1).unwrap();
    let unit = dwarf.units.get_mut(id);
    unit.get_mut(physical).delete(gimli::DW_AT_low_pc);
    unit.get_mut(physical).delete(gimli::DW_AT_high_pc);
    let sections = write_dwarf(dwarf);
    let addr2line = addr2line::Context::from_dwarf(read_dwarf(&sections)).unwrap();
    let context = Context::default();
    let resolved = resolve_instruction_debug_context(
        &addr2line,
        0x1000,
        context.session(),
        &crate::WasmTranslationConfig::default(),
    )
    .unwrap();
    assert!(!resolved.span.is_unknown());
    assert!(resolved.inline_calls.is_empty());
}

#[test]
fn unreadable_source_has_unknown_span() {
    let context = Context::default();
    // A directory is an existing path which cannot be loaded as source text.
    let location = addr2line::Location {
        file: Some(env!("CARGO_MANIFEST_DIR")),
        line: Some(1),
        column: Some(1),
    };
    assert!(resolve_source_span(
        &location, context.session(), &crate::WasmTranslationConfig::default(),
    ).is_unknown());
}
