//! A command to dump debug information from MASP packages
//!
//! Similar to llvm-dwarfdump, this tool parses the `.debug_info` section from compiled MASP
//! packages and displays the debug metadata in a human-readable format.
use std::{collections::BTreeMap, path::PathBuf};

use clap::Args;
use miden_core::{
    mast::MastForest,
    operations::{DebugVarInfo, DebugVarLocation},
    serde::{Deserializable, SliceReader},
};
use miden_mast_package::{
    Package, SectionId,
    debug_info::{
        DebugFileInfo, DebugFunctionInfo, DebugFunctionsSection, DebugPrimitiveType,
        DebugSourcesSection, DebugTypeIdx, DebugTypeInfo, DebugTypesSection, DebugVariableInfo,
    },
};

use super::{DumpError, Section};

/// Dump debug information encoded in a .masp file
#[derive(Debug, Args)]
pub struct Config {
    /// The input package to dump info from
    #[arg(required = true)]
    input: PathBuf,

    /// Filter output to a specific section
    #[arg(short, long, value_enum)]
    section: Option<Section>,

    /// Show all available information
    #[arg(short, long)]
    verbose: bool,

    /// Show raw indices instead of resolved names
    #[arg(long)]
    raw: bool,

    /// Only show summary statistics
    #[arg(long)]
    summary: bool,
}

pub fn dump(config: &Config) -> Result<(), DumpError> {
    // Read the MASP file
    let bytes = std::fs::read_to_string(&config.input)?.into_bytes();

    // Parse the package
    let package: Package = Package::read_from(&mut SliceReader::new(&bytes))
        .map_err(|e| DumpError::Parse(e.to_string()))?;

    // Get the MAST forest for location decorators
    let mast_forest = package.mast.mast_forest();

    // Find the three debug sections
    let types_section = extract_section::<DebugTypesSection>(&package, SectionId::DEBUG_TYPES)?;
    let sources_section =
        extract_section::<DebugSourcesSection>(&package, SectionId::DEBUG_SOURCES)?;
    let functions_section =
        extract_section::<DebugFunctionsSection>(&package, SectionId::DEBUG_FUNCTIONS)?;

    // We need at least one section to proceed
    if types_section.is_none() && sources_section.is_none() && functions_section.is_none() {
        return Err(DumpError::NoDebugInfo);
    }

    // Parse each section (use empty defaults if missing)
    let debug_sections = DebugSections {
        types: types_section.unwrap_or_default(),
        sources: sources_section.unwrap_or_default(),
        functions: functions_section.unwrap_or_default(),
    };

    // Print header
    println!("{}", "=".repeat(80));
    println!("Package Info:");
    println!("  | Name:    {}", &package.name);
    println!("  | Version: {}", &package.version);
    println!("  | Kind:    {}", &package.kind);
    println!("Section Versioning:");
    println!("  | Types:     {}", debug_sections.types.version);
    println!("  | Sources:   {}", debug_sections.sources.version);
    println!("  | Functions: {}", debug_sections.functions.version);
    println!("{}", "=".repeat(80));
    println!();

    if config.summary {
        print_summary(&debug_sections, mast_forest);
        return Ok(());
    }

    match config.section {
        Some(Section::Strings) => print_strings(&debug_sections),
        Some(Section::Types) => print_types(&debug_sections, config.raw),
        Some(Section::Files) => print_files(&debug_sections, config.raw),
        Some(Section::Functions) => print_functions(&debug_sections, config.raw, config.verbose),
        Some(Section::Variables) => print_variables(&debug_sections, config.raw),
        Some(Section::Locations) => print_locations(mast_forest, &debug_sections, config.verbose),
        None => {
            // Print everything
            print_summary(&debug_sections, mast_forest);
            println!();
            print_strings(&debug_sections);
            println!();
            print_types(&debug_sections, config.raw);
            println!();
            print_files(&debug_sections, config.raw);
            println!();
            print_functions(&debug_sections, config.raw, config.verbose);
            println!();
            print_locations(mast_forest, &debug_sections, config.verbose);
        }
    }

    Ok(())
}

fn extract_section<T>(package: &Package, id: SectionId) -> Result<Option<T>, DumpError>
where
    T: Deserializable,
{
    let Some(section) = package.sections.iter().find(|s| s.id == id) else {
        return Ok(None);
    };

    T::read_from_bytes(&section.data).map(Some).map_err(DumpError::from)
}

const FRAME_BASE_LOCAL_MARKER: u32 = 1 << 31;

fn decode_frame_base_local_offset(encoded: u32) -> Option<i16> {
    if encoded & FRAME_BASE_LOCAL_MARKER == 0 {
        return None;
    }

    let low_bits = (encoded & 0xffff) as u16;
    Some(i16::from_le_bytes(low_bits.to_le_bytes()))
}

fn format_debug_var_location(location: &DebugVarLocation) -> String {
    if let DebugVarLocation::FrameBase {
        global_index,
        byte_offset,
    } = location
        && let Some(offset) = decode_frame_base_local_offset(*global_index)
    {
        format!("frame_base(FMP{offset:+}){byte_offset:+}")
    } else {
        location.to_string()
    }
}

/// Holds the three debug info sections with helper accessors.
struct DebugSections {
    types: DebugTypesSection,
    sources: DebugSourcesSection,
    functions: DebugFunctionsSection,
}

impl DebugSections {
    /// Look up a string in the types section's string table.
    fn get_type_string(&self, idx: u32) -> Option<String> {
        self.types.get_string(idx).map(|s| s.to_string())
    }

    /// Look up a string in the sources section's string table.
    fn get_source_string(&self, idx: u32) -> Option<String> {
        self.sources.get_string(idx).map(|s| s.to_string())
    }

    /// Look up a string in the functions section's string table.
    fn get_func_string(&self, idx: u32) -> Option<String> {
        self.functions.get_string(idx).map(|s| s.to_string())
    }

    /// Look up a type by index.
    fn get_type(&self, idx: DebugTypeIdx) -> Option<&DebugTypeInfo> {
        self.types.get_type(idx)
    }

    /// Look up a file by index.
    fn get_file(&self, idx: u32) -> Option<&DebugFileInfo> {
        self.sources.get_file(idx)
    }
}

fn print_summary(debug_sections: &DebugSections, mast_forest: &MastForest) {
    println!("Summary:");
    println!();

    println!("Types:");
    println!("  | records: {}", &debug_sections.types.types.len());
    println!("  | strings: {}", &debug_sections.types.strings.len());
    println!();

    println!("Sources:");
    println!("  | records: {}", &debug_sections.sources.files.len());
    println!("  | strings: {}", &debug_sections.sources.strings.len());
    println!();

    let total_vars: usize =
        debug_sections.functions.functions.iter().map(|f| f.variables.len()).sum();
    let total_inlined: usize =
        debug_sections.functions.functions.iter().map(|f| f.inlined_calls.len()).sum();
    println!("Functions:");
    println!("  | records:   {}", &debug_sections.functions.functions.len());
    println!("  | strings:   {}", &debug_sections.functions.strings.len());
    println!("  | variables: {total_vars} (total across all functions)");
    println!("  | inlined:   {total_inlined} call sites");
    println!();

    // Count debug vars in MAST
    let debug_var_count = mast_forest.debug_info().debug_vars().len();
    println!("Found {debug_var_count} debug variable records");
}

fn print_strings(debug_sections: &DebugSections) {
    println!(".debug_str contents:");
    println!("{:-<80}", "");

    println!("  [types string table]");
    for (idx, s) in debug_sections.types.strings.iter().enumerate() {
        println!("  [{:4}] \"{}\"", idx, s);
    }
    println!();
    println!("  [sources string table]");
    for (idx, s) in debug_sections.sources.strings.iter().enumerate() {
        println!("  [{:4}] \"{}\"", idx, s);
    }
    println!();
    println!("  [functions string table]");
    for (idx, s) in debug_sections.functions.strings.iter().enumerate() {
        println!("  [{:4}] \"{}\"", idx, s);
    }
}

fn print_types(debug_sections: &DebugSections, raw: bool) {
    println!(".debug_types contents:");
    println!("{:-<80}", "");
    for (idx, ty) in debug_sections.types.types.iter().enumerate() {
        print!("  [{:4}] ", idx);
        print_type(ty, debug_sections, raw, 0);
        println!();
    }
}

fn print_type(ty: &DebugTypeInfo, debug_sections: &DebugSections, raw: bool, indent: usize) {
    let pad = "  ".repeat(indent);
    match ty {
        DebugTypeInfo::Primitive(prim) => {
            print!("{}PRIMITIVE: {}", pad, primitive_name(*prim));
            print!(" (size: {} bytes, {} felts)", prim.size_in_bytes(), prim.size_in_felts());
        }
        DebugTypeInfo::Pointer { pointee_type_idx } => {
            if raw {
                print!("{}POINTER -> type[{}]", pad, pointee_type_idx.as_u32());
            } else {
                print!("{}POINTER -> ", pad);
                if let Some(pointee) = debug_sections.get_type(*pointee_type_idx) {
                    print_type_brief(pointee, debug_sections);
                } else {
                    print!("<invalid type idx {}>", pointee_type_idx.as_u32());
                }
            }
        }
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => {
            if raw {
                print!("{}ARRAY [{}; {:?}]", pad, element_type_idx.as_u32(), count);
            } else {
                print!("{}ARRAY [", pad);
                if let Some(elem) = debug_sections.get_type(*element_type_idx) {
                    print_type_brief(elem, debug_sections);
                } else {
                    print!("<invalid>");
                }
                match count {
                    Some(n) => print!("; {}]", n),
                    None => print!("; ?]"),
                }
            }
        }
        DebugTypeInfo::Struct {
            name_idx,
            size,
            fields,
        } => {
            let name = if raw {
                format!("str[{}]", name_idx)
            } else {
                debug_sections.get_type_string(*name_idx).unwrap_or_else(|| "<unknown>".into())
            };
            print!("{}STRUCT {} (size: {} bytes)", pad, name, size);
            if !fields.is_empty() {
                println!();
                for field in fields {
                    let field_name = if raw {
                        format!("str[{}]", field.name_idx)
                    } else {
                        debug_sections
                            .get_type_string(field.name_idx)
                            .unwrap_or_else(|| "<unknown>".into())
                    };
                    print!("{}    +{:4}: {} : ", pad, field.offset, field_name);
                    if let Some(fty) = debug_sections.get_type(field.type_idx) {
                        print_type_brief(fty, debug_sections);
                    } else {
                        print!("<invalid type>");
                    }
                    println!();
                }
            }
        }
        DebugTypeInfo::Function {
            return_type_idx,
            param_type_indices,
        } => {
            print!("{}FUNCTION (", pad);
            for (i, param_idx) in param_type_indices.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                if raw {
                    print!("type[{}]", param_idx.as_u32());
                } else if let Some(pty) = debug_sections.get_type(*param_idx) {
                    print_type_brief(pty, debug_sections);
                } else {
                    print!("<invalid>");
                }
            }
            print!(") -> ");
            match return_type_idx {
                Some(idx) => {
                    if raw {
                        print!("type[{}]", idx.as_u32());
                    } else if let Some(rty) = debug_sections.get_type(*idx) {
                        print_type_brief(rty, debug_sections);
                    } else {
                        print!("<invalid>");
                    }
                }
                None => print!("void"),
            }
        }
        DebugTypeInfo::Unknown => {
            print!("{}UNKNOWN", pad);
        }
    }
}

fn print_type_brief(ty: &DebugTypeInfo, debug_sections: &DebugSections) {
    match ty {
        DebugTypeInfo::Primitive(prim) => print!("{}", primitive_name(*prim)),
        DebugTypeInfo::Pointer { pointee_type_idx } => {
            print!("*");
            if let Some(p) = debug_sections.get_type(*pointee_type_idx) {
                print_type_brief(p, debug_sections);
            }
        }
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => {
            print!("[");
            if let Some(e) = debug_sections.get_type(*element_type_idx) {
                print_type_brief(e, debug_sections);
            }
            match count {
                Some(n) => print!("; {}]", n),
                None => print!("]"),
            }
        }
        DebugTypeInfo::Struct { name_idx, .. } => {
            print!(
                "struct {}",
                debug_sections.get_type_string(*name_idx).unwrap_or_else(|| "?".into())
            );
        }
        DebugTypeInfo::Function { .. } => print!("fn(...)"),
        DebugTypeInfo::Unknown => print!("?"),
    }
}

fn primitive_name(prim: DebugPrimitiveType) -> &'static str {
    match prim {
        DebugPrimitiveType::Void => "void",
        DebugPrimitiveType::Bool => "bool",
        DebugPrimitiveType::I8 => "i8",
        DebugPrimitiveType::U8 => "u8",
        DebugPrimitiveType::I16 => "i16",
        DebugPrimitiveType::U16 => "u16",
        DebugPrimitiveType::I32 => "i32",
        DebugPrimitiveType::U32 => "u32",
        DebugPrimitiveType::I64 => "i64",
        DebugPrimitiveType::U64 => "u64",
        DebugPrimitiveType::I128 => "i128",
        DebugPrimitiveType::U128 => "u128",
        DebugPrimitiveType::F32 => "f32",
        DebugPrimitiveType::F64 => "f64",
        DebugPrimitiveType::Felt => "felt",
        DebugPrimitiveType::Word => "word",
    }
}

fn print_files(debug_sections: &DebugSections, raw: bool) {
    println!(".debug_files contents:");
    println!("{:-<80}", "");
    for (idx, file) in debug_sections.sources.files.iter().enumerate() {
        print_file(idx, file, debug_sections, raw);
    }
}

fn print_file(idx: usize, file: &DebugFileInfo, debug_sections: &DebugSections, raw: bool) {
    let path = if raw {
        format!("str[{}]", file.path_idx)
    } else {
        debug_sections
            .get_source_string(file.path_idx)
            .unwrap_or_else(|| "<unknown>".into())
    };

    print!("  [{:4}] {}", idx, path);

    if let Some(checksum) = &file.checksum {
        print!(" [checksum: ");
        for byte in &checksum[..4] {
            print!("{:02x}", byte);
        }
        print!("...]");
    }

    println!();
}

fn print_functions(debug_sections: &DebugSections, raw: bool, verbose: bool) {
    println!(".debug_functions contents:");
    println!("{:-<80}", "");
    for (idx, func) in debug_sections.functions.functions.iter().enumerate() {
        print_function(idx, func, debug_sections, raw, verbose);
        println!();
    }
}

fn print_function(
    idx: usize,
    func: &DebugFunctionInfo,
    debug_sections: &DebugSections,
    raw: bool,
    verbose: bool,
) {
    let name = if raw {
        format!("str[{}]", func.name_idx)
    } else {
        debug_sections
            .get_func_string(func.name_idx)
            .unwrap_or_else(|| "<unknown>".into())
    };

    println!("  [{:4}] FUNCTION: {}", idx, name);

    // Linkage name
    if let Some(linkage_idx) = func.linkage_name_idx {
        let linkage = if raw {
            format!("str[{}]", linkage_idx)
        } else {
            debug_sections
                .get_func_string(linkage_idx)
                .unwrap_or_else(|| "<unknown>".into())
        };
        println!("         Linkage name: {}", linkage);
    }

    // Location
    let file_path = if raw {
        format!("file[{}]", func.file_idx)
    } else {
        debug_sections
            .get_file(func.file_idx)
            .and_then(|f| debug_sections.get_source_string(f.path_idx))
            .unwrap_or_else(|| "<unknown>".into())
    };
    println!("         Location: {}:{}:{}", file_path, func.line, func.column);

    // Type
    if let Some(type_idx) = func.type_idx {
        print!("         Type: ");
        if raw {
            println!("type[{}]", type_idx.as_u32());
        } else if let Some(ty) = debug_sections.get_type(type_idx) {
            print_type_brief(ty, debug_sections);
            println!();
        } else {
            println!("<invalid>");
        }
    }

    // MAST root
    if let Some(root) = &func.mast_root {
        print!("         MAST root: 0x");
        for byte in &root.as_bytes() {
            print!("{:02x}", byte);
        }
        println!();
    }

    // Variables
    if !func.variables.is_empty() {
        println!("         Variables ({}):", func.variables.len());
        for var in &func.variables {
            print_variable(var, debug_sections, raw, verbose);
        }
    }

    // Inlined calls
    if !func.inlined_calls.is_empty() && verbose {
        println!("         Inlined calls ({}):", func.inlined_calls.len());
        for call in &func.inlined_calls {
            let callee = if raw {
                format!("func[{}]", call.callee_idx)
            } else {
                debug_sections
                    .functions
                    .functions
                    .get(call.callee_idx as usize)
                    .and_then(|f| debug_sections.get_func_string(f.name_idx))
                    .unwrap_or_else(|| "<unknown>".into())
            };
            let call_file = if raw {
                format!("file[{}]", call.file_idx)
            } else {
                debug_sections
                    .get_file(call.file_idx)
                    .and_then(|f| debug_sections.get_source_string(f.path_idx))
                    .unwrap_or_else(|| "<unknown>".into())
            };
            println!(
                "           - {} inlined at {}:{}:{}",
                callee, call_file, call.line, call.column
            );
        }
    }
}

fn print_variable(
    var: &DebugVariableInfo,
    debug_sections: &DebugSections,
    raw: bool,
    _verbose: bool,
) {
    let name = if raw {
        format!("str[{}]", var.name_idx)
    } else {
        debug_sections
            .get_func_string(var.name_idx)
            .unwrap_or_else(|| "<unknown>".into())
    };

    let kind = if var.is_parameter() {
        format!("param #{}", var.arg_index)
    } else {
        "local".to_string()
    };

    print!("           - {} ({}): ", name, kind);

    if raw {
        print!("type[{}]", var.type_idx.as_u32());
    } else if let Some(ty) = debug_sections.get_type(var.type_idx) {
        print_type_brief(ty, debug_sections);
    } else {
        print!("<invalid type>");
    }

    print!(" @ {}:{}", var.line, var.column);

    if var.scope_depth > 0 {
        print!(" [scope depth: {}]", var.scope_depth);
    }

    println!();
}

fn print_variables(debug_sections: &DebugSections, raw: bool) {
    println!(".debug_variables contents (all functions):");
    println!("{:-<80}", "");

    for func in &debug_sections.functions.functions {
        if func.variables.is_empty() {
            continue;
        }

        let func_name = debug_sections
            .get_func_string(func.name_idx)
            .unwrap_or_else(|| "<unknown>".into());
        println!("  Function: {}", func_name);

        for var in &func.variables {
            print_variable(var, debug_sections, raw, false);
        }
        println!();
    }
}

/// Prints the .debug_loc section - variable location entries from MAST
///
/// This is analogous to DWARF's .debug_loc section which contains location
/// lists describing where a variable's value can be found at runtime.
fn print_locations(mast_forest: &MastForest, debug_sections: &DebugSections, verbose: bool) {
    println!(".debug_loc contents (DebugVar entries from MAST):");
    println!("{:-<80}", "");

    // Collect all debug vars from the MastForest
    let debug_vars = mast_forest.debug_info().debug_vars();

    if debug_vars.is_empty() {
        println!("  (no DebugVar entries found)");
        return;
    }

    // Group by variable name for a cleaner view
    let mut by_name: BTreeMap<&str, Vec<(usize, &DebugVarInfo)>> = BTreeMap::new();
    for (idx, info) in debug_vars.iter().enumerate() {
        by_name.entry(info.name()).or_default().push((idx, info));
    }

    println!("  Total DebugVar entries: {}", debug_vars.len());
    println!("  Unique variable names: {}", by_name.len());
    println!();

    for (name, entries) in &by_name {
        println!("  Variable: \"{}\"", name);
        println!("  {} location entries:", entries.len());

        for (var_idx, info) in entries {
            print!("    [var#{}] ", var_idx);

            // Print value location
            print!("{}", format_debug_var_location(info.value_location()));

            // Print argument info if present
            if let Some(arg_idx) = info.arg_index() {
                print!(" (param #{})", arg_idx);
            }

            // Print type info if present and we can resolve it
            if let Some(type_id) = info.type_id() {
                let type_idx = DebugTypeIdx::from(type_id);
                if let Some(ty) = debug_sections.get_type(type_idx) {
                    print!(" : ");
                    print_type_brief(ty, debug_sections);
                } else {
                    print!(" : type[{}]", type_id);
                }
            }

            // Print source location if present
            if let Some(loc) = info.location() {
                print!(" @ {}:{}:{}", loc.uri, loc.line, loc.column);
            }

            println!();
        }
        println!();
    }

    // In verbose mode, also show raw list
    if verbose {
        println!("  Raw debug var list (in order):");
        println!("  {:-<76}", "");
        for (idx, info) in debug_vars.iter().enumerate() {
            println!("    [{:4}] {}", idx, info);
        }
    }
}
