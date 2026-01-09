//! miden-debugdump - A tool to dump debug information from MASP packages
//!
//! Similar to llvm-dwarfdump, this tool parses the `.debug_info` section
//! from compiled MASP packages and displays the debug metadata in a
//! human-readable format.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
};

use clap::{Parser, ValueEnum};
use miden_core::{
    Decorator,
    utils::{Deserializable, SliceReader},
};
use miden_mast_package::{
    MastForest, Package, SectionId,
    debug_info::{
        DebugFileInfo, DebugFunctionInfo, DebugInfoSection, DebugPrimitiveType, DebugTypeInfo,
        DebugVariableInfo,
    },
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse package: {0}")]
    Parse(String),
    #[error("no debug_info section found in package")]
    NoDebugInfo,
}

/// A tool to dump debug information from MASP packages
#[derive(Parser, Debug)]
#[command(
    name = "miden-debugdump",
    about = "Dump debug information from MASP packages (similar to llvm-dwarfdump)",
    version,
    rename_all = "kebab-case"
)]
struct Cli {
    /// Input MASP file to analyze
    #[arg(required = true)]
    input: PathBuf,

    /// Filter output to specific section
    #[arg(short, long, value_enum)]
    section: Option<DumpSection>,

    /// Show all available information (verbose)
    #[arg(short, long)]
    verbose: bool,

    /// Show raw indices instead of resolved names
    #[arg(long)]
    raw: bool,

    /// Only show summary statistics
    #[arg(long)]
    summary: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DumpSection {
    /// Show string table
    Strings,
    /// Show type information
    Types,
    /// Show source file information
    Files,
    /// Show function debug information
    Functions,
    /// Show variable information within functions
    Variables,
    /// Show variable location decorators from MAST (similar to DWARF .debug_loc)
    Locations,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let cli = Cli::parse();

    // Read the MASP file
    let file = File::open(&cli.input)?;
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;

    // Parse the package
    let package = Package::read_from(&mut SliceReader::new(&bytes))
        .map_err(|e| Error::Parse(e.to_string()))?;

    // Get the MAST forest for location decorators
    let mast_forest = package.mast.mast_forest();

    // Find the debug_info section
    let debug_section = package
        .sections
        .iter()
        .find(|s| s.id == SectionId::DEBUG_INFO)
        .ok_or(Error::NoDebugInfo)?;

    // Parse the debug info
    let debug_info = DebugInfoSection::read_from(&mut SliceReader::new(&debug_section.data))
        .map_err(|e| Error::Parse(e.to_string()))?;

    // Print header
    println!("{}", "=".repeat(80));
    println!("DEBUG INFO DUMP: {}", cli.input.display());
    println!(
        "Package: {} (version: {})",
        package.name,
        package
            .version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
    println!("Debug info version: {}", debug_info.version);
    println!("{}", "=".repeat(80));
    println!();

    if cli.summary {
        print_summary(&debug_info, mast_forest);
        return Ok(());
    }

    match cli.section {
        Some(DumpSection::Strings) => print_strings(&debug_info),
        Some(DumpSection::Types) => print_types(&debug_info, cli.raw),
        Some(DumpSection::Files) => print_files(&debug_info, cli.raw),
        Some(DumpSection::Functions) => print_functions(&debug_info, cli.raw, cli.verbose),
        Some(DumpSection::Variables) => print_variables(&debug_info, cli.raw),
        Some(DumpSection::Locations) => print_locations(mast_forest, &debug_info, cli.verbose),
        None => {
            // Print everything
            print_summary(&debug_info, mast_forest);
            println!();
            print_strings(&debug_info);
            println!();
            print_types(&debug_info, cli.raw);
            println!();
            print_files(&debug_info, cli.raw);
            println!();
            print_functions(&debug_info, cli.raw, cli.verbose);
            println!();
            print_locations(mast_forest, &debug_info, cli.verbose);
        }
    }

    Ok(())
}

fn print_summary(debug_info: &DebugInfoSection, mast_forest: &MastForest) {
    println!(".debug_info summary:");
    println!("  Strings:   {} entries", debug_info.strings.len());
    println!("  Types:     {} entries", debug_info.types.len());
    println!("  Files:     {} entries", debug_info.files.len());
    println!("  Functions: {} entries", debug_info.functions.len());

    let total_vars: usize = debug_info.functions.iter().map(|f| f.variables.len()).sum();
    let total_inlined: usize = debug_info.functions.iter().map(|f| f.inlined_calls.len()).sum();
    println!("  Variables: {} total (across all functions)", total_vars);
    println!("  Inlined:   {} call sites", total_inlined);

    // Count DebugVar decorators in MAST
    let debug_var_count = mast_forest
        .decorators()
        .iter()
        .filter(|d| matches!(d, Decorator::DebugVar(_)))
        .count();
    println!("  DebugVar decorators: {} in MAST", debug_var_count);
}

fn print_strings(debug_info: &DebugInfoSection) {
    println!(".debug_str contents:");
    println!("{:-<80}", "");
    for (idx, s) in debug_info.strings.iter().enumerate() {
        println!("  [{:4}] \"{}\"", idx, s);
    }
}

fn print_types(debug_info: &DebugInfoSection, raw: bool) {
    println!(".debug_types contents:");
    println!("{:-<80}", "");
    for (idx, ty) in debug_info.types.iter().enumerate() {
        print!("  [{:4}] ", idx);
        print_type(ty, debug_info, raw, 0);
        println!();
    }
}

fn print_type(ty: &DebugTypeInfo, debug_info: &DebugInfoSection, raw: bool, indent: usize) {
    let pad = "  ".repeat(indent);
    match ty {
        DebugTypeInfo::Primitive(prim) => {
            print!("{}PRIMITIVE: {}", pad, primitive_name(*prim));
            print!(" (size: {} bytes, {} felts)", prim.size_in_bytes(), prim.size_in_felts());
        }
        DebugTypeInfo::Pointer { pointee_type_idx } => {
            if raw {
                print!("{}POINTER -> type[{}]", pad, pointee_type_idx);
            } else {
                print!("{}POINTER -> ", pad);
                if let Some(pointee) = debug_info.get_type(*pointee_type_idx) {
                    print_type_brief(pointee, debug_info);
                } else {
                    print!("<invalid type idx {}>", pointee_type_idx);
                }
            }
        }
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => {
            if raw {
                print!("{}ARRAY [{}; {:?}]", pad, element_type_idx, count);
            } else {
                print!("{}ARRAY [", pad);
                if let Some(elem) = debug_info.get_type(*element_type_idx) {
                    print_type_brief(elem, debug_info);
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
                debug_info.get_string(*name_idx).unwrap_or("<unknown>").to_string()
            };
            print!("{}STRUCT {} (size: {} bytes)", pad, name, size);
            if !fields.is_empty() {
                println!();
                for field in fields {
                    let field_name = if raw {
                        format!("str[{}]", field.name_idx)
                    } else {
                        debug_info.get_string(field.name_idx).unwrap_or("<unknown>").to_string()
                    };
                    print!("{}    +{:4}: {} : ", pad, field.offset, field_name);
                    if let Some(fty) = debug_info.get_type(field.type_idx) {
                        print_type_brief(fty, debug_info);
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
                    print!("type[{}]", param_idx);
                } else if let Some(pty) = debug_info.get_type(*param_idx) {
                    print_type_brief(pty, debug_info);
                } else {
                    print!("<invalid>");
                }
            }
            print!(") -> ");
            match return_type_idx {
                Some(idx) => {
                    if raw {
                        print!("type[{}]", idx);
                    } else if let Some(rty) = debug_info.get_type(*idx) {
                        print_type_brief(rty, debug_info);
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

fn print_type_brief(ty: &DebugTypeInfo, debug_info: &DebugInfoSection) {
    match ty {
        DebugTypeInfo::Primitive(prim) => print!("{}", primitive_name(*prim)),
        DebugTypeInfo::Pointer { pointee_type_idx } => {
            print!("*");
            if let Some(p) = debug_info.get_type(*pointee_type_idx) {
                print_type_brief(p, debug_info);
            }
        }
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => {
            print!("[");
            if let Some(e) = debug_info.get_type(*element_type_idx) {
                print_type_brief(e, debug_info);
            }
            match count {
                Some(n) => print!("; {}]", n),
                None => print!("]"),
            }
        }
        DebugTypeInfo::Struct { name_idx, .. } => {
            print!("struct {}", debug_info.get_string(*name_idx).unwrap_or("?"));
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

fn print_files(debug_info: &DebugInfoSection, raw: bool) {
    println!(".debug_files contents:");
    println!("{:-<80}", "");
    for (idx, file) in debug_info.files.iter().enumerate() {
        print_file(idx, file, debug_info, raw);
    }
}

fn print_file(idx: usize, file: &DebugFileInfo, debug_info: &DebugInfoSection, raw: bool) {
    let path = if raw {
        format!("str[{}]", file.path_idx)
    } else {
        debug_info.get_string(file.path_idx).unwrap_or("<unknown>").to_string()
    };

    print!("  [{:4}] {}", idx, path);

    if let Some(dir_idx) = file.directory_idx {
        let dir = if raw {
            format!("str[{}]", dir_idx)
        } else {
            debug_info.get_string(dir_idx).unwrap_or("<unknown>").to_string()
        };
        print!(" (dir: {})", dir);
    }

    if let Some(checksum) = &file.checksum {
        print!(" [checksum: ");
        for byte in &checksum[..4] {
            print!("{:02x}", byte);
        }
        print!("...]");
    }

    println!();
}

fn print_functions(debug_info: &DebugInfoSection, raw: bool, verbose: bool) {
    println!(".debug_functions contents:");
    println!("{:-<80}", "");
    for (idx, func) in debug_info.functions.iter().enumerate() {
        print_function(idx, func, debug_info, raw, verbose);
        println!();
    }
}

fn print_function(
    idx: usize,
    func: &DebugFunctionInfo,
    debug_info: &DebugInfoSection,
    raw: bool,
    verbose: bool,
) {
    let name = if raw {
        format!("str[{}]", func.name_idx)
    } else {
        debug_info.get_string(func.name_idx).unwrap_or("<unknown>").to_string()
    };

    println!("  [{:4}] FUNCTION: {}", idx, name);

    // Linkage name
    if let Some(linkage_idx) = func.linkage_name_idx {
        let linkage = if raw {
            format!("str[{}]", linkage_idx)
        } else {
            debug_info.get_string(linkage_idx).unwrap_or("<unknown>").to_string()
        };
        println!("         Linkage name: {}", linkage);
    }

    // Location
    let file_path = if raw {
        format!("file[{}]", func.file_idx)
    } else {
        debug_info
            .get_file(func.file_idx)
            .and_then(|f| debug_info.get_string(f.path_idx))
            .unwrap_or("<unknown>")
            .to_string()
    };
    println!("         Location: {}:{}:{}", file_path, func.line, func.column);

    // Type
    if let Some(type_idx) = func.type_idx {
        print!("         Type: ");
        if raw {
            println!("type[{}]", type_idx);
        } else if let Some(ty) = debug_info.get_type(type_idx) {
            print_type_brief(ty, debug_info);
            println!();
        } else {
            println!("<invalid>");
        }
    }

    // MAST root
    if let Some(root) = &func.mast_root {
        print!("         MAST root: 0x");
        for byte in root {
            print!("{:02x}", byte);
        }
        println!();
    }

    // Variables
    if !func.variables.is_empty() {
        println!("         Variables ({}):", func.variables.len());
        for var in &func.variables {
            print_variable(var, debug_info, raw, verbose);
        }
    }

    // Inlined calls
    if !func.inlined_calls.is_empty() && verbose {
        println!("         Inlined calls ({}):", func.inlined_calls.len());
        for call in &func.inlined_calls {
            let callee = if raw {
                format!("func[{}]", call.callee_idx)
            } else {
                debug_info
                    .functions
                    .get(call.callee_idx as usize)
                    .and_then(|f| debug_info.get_string(f.name_idx))
                    .unwrap_or("<unknown>")
                    .to_string()
            };
            let call_file = if raw {
                format!("file[{}]", call.file_idx)
            } else {
                debug_info
                    .get_file(call.file_idx)
                    .and_then(|f| debug_info.get_string(f.path_idx))
                    .unwrap_or("<unknown>")
                    .to_string()
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
    debug_info: &DebugInfoSection,
    raw: bool,
    _verbose: bool,
) {
    let name = if raw {
        format!("str[{}]", var.name_idx)
    } else {
        debug_info.get_string(var.name_idx).unwrap_or("<unknown>").to_string()
    };

    let kind = if var.is_parameter() {
        format!("param #{}", var.arg_index)
    } else {
        "local".to_string()
    };

    print!("           - {} ({}): ", name, kind);

    if raw {
        print!("type[{}]", var.type_idx);
    } else if let Some(ty) = debug_info.get_type(var.type_idx) {
        print_type_brief(ty, debug_info);
    } else {
        print!("<invalid type>");
    }

    print!(" @ {}:{}", var.line, var.column);

    if var.scope_depth > 0 {
        print!(" [scope depth: {}]", var.scope_depth);
    }

    println!();
}

fn print_variables(debug_info: &DebugInfoSection, raw: bool) {
    println!(".debug_variables contents (all functions):");
    println!("{:-<80}", "");

    for func in &debug_info.functions {
        if func.variables.is_empty() {
            continue;
        }

        let func_name = debug_info.get_string(func.name_idx).unwrap_or("<unknown>");
        println!("  Function: {}", func_name);

        for var in &func.variables {
            print_variable(var, debug_info, raw, false);
        }
        println!();
    }
}

/// Prints the .debug_loc section - variable location decorators from MAST
///
/// This is analogous to DWARF's .debug_loc section which contains location
/// lists describing where a variable's value can be found at runtime.
fn print_locations(mast_forest: &MastForest, debug_info: &DebugInfoSection, verbose: bool) {
    println!(".debug_loc contents (DebugVar decorators from MAST):");
    println!("{:-<80}", "");

    // Collect all DebugVar decorators
    let debug_vars: Vec<_> = mast_forest
        .decorators()
        .iter()
        .enumerate()
        .filter_map(|(idx, dec)| {
            if let Decorator::DebugVar(info) = dec {
                Some((idx, info))
            } else {
                None
            }
        })
        .collect();

    if debug_vars.is_empty() {
        println!("  (no DebugVar decorators found)");
        return;
    }

    // Group by variable name for a cleaner view
    let mut by_name: BTreeMap<&str, Vec<(usize, &miden_core::DebugVarInfo)>> = BTreeMap::new();
    for (idx, info) in &debug_vars {
        by_name.entry(info.name()).or_default().push((*idx, *info));
    }

    println!("  Total DebugVar decorators: {}", debug_vars.len());
    println!("  Unique variable names: {}", by_name.len());
    println!();

    for (name, entries) in &by_name {
        println!("  Variable: \"{}\"", name);
        println!("  {} location entries:", entries.len());

        for (decorator_idx, info) in entries {
            print!("    [dec#{}] ", decorator_idx);

            // Print value location
            print!("{}", info.value_location());

            // Print argument info if present
            if let Some(arg_idx) = info.arg_index() {
                print!(" (param #{})", arg_idx);
            }

            // Print type info if present and we can resolve it
            if let Some(type_id) = info.type_id() {
                if let Some(ty) = debug_info.get_type(type_id) {
                    print!(" : ");
                    print_type_brief(ty, debug_info);
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

    // In verbose mode, also show raw decorator list
    if verbose {
        println!("  Raw decorator list (in order):");
        println!("  {:-<76}", "");
        for (idx, info) in &debug_vars {
            println!("    [{:4}] {}", idx, info);
        }
    }
}
