//! A command to dump debug information from MASP packages
//!
//! Similar to llvm-dwarfdump, this tool parses the `.debug_info` section from compiled MASP
//! packages and displays the debug metadata in a human-readable format.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use clap::Args;
use miden_core::operations::DebugVarLocation;
use miden_mast_package::{
    MastForest, Package,
    debug_info::{
        DebugFileInfo, DebugFunctionInfo, DebugPrimitiveType, DebugSourceNodeId, DebugSourceVar,
        DebugTypeIdx, DebugTypeInfo, PackageDebugInfo,
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
    let bytes = std::fs::read(&config.input)?;

    // Parse the package
    let package: Package =
        Package::read_from_bytes_trusted(&bytes).map_err(|e| DumpError::Parse(e.to_string()))?;

    // Get the MAST forest for location decorators
    let Some(debug_info) = package.debug_info()? else {
        return Err(DumpError::NoDebugInfo);
    };

    // Print header
    println!("{}", "=".repeat(80));
    println!("Package Info:");
    println!("  | Name:               {}", &package.name);
    println!("  | Version:            {}", &package.version);
    println!("  | Kind:               {}", &package.kind);
    println!("  | Debug Info Version: {}", &debug_info.version());
    println!("{}", "=".repeat(80));
    println!();

    if config.summary {
        print_summary(&debug_info);
        return Ok(());
    }

    match config.section {
        Some(Section::Strings) => print_strings(&debug_info),
        Some(Section::Types) => print_types(&debug_info, config.raw),
        Some(Section::Files) => print_files(&debug_info, config.raw),
        Some(Section::Functions) => {
            print_functions(&debug_info, package.mast_forest(), config.raw, config.verbose)
        }
        Some(Section::Variables) => print_variables(&debug_info, package.mast_forest(), config.raw),
        Some(Section::Locations) => print_locations(&debug_info),
        None => {
            // Print everything
            let mast_forest = package.mast_forest();
            print_summary(&debug_info);
            println!();
            print_strings(&debug_info);
            println!();
            print_types(&debug_info, config.raw);
            println!();
            print_files(&debug_info, config.raw);
            println!();
            print_functions(&debug_info, mast_forest, config.raw, config.verbose);
            println!();
            print_locations(&debug_info);
        }
    }

    Ok(())
}

const FRAME_BASE_LOCAL_MARKER: u32 = 1 << 31;
const DEBUG_VAR_KILL_SENTINEL: &[u8] = b"\0miden.debug.kill";

fn decode_frame_base_local_offset(encoded: u32) -> Option<i16> {
    if encoded & FRAME_BASE_LOCAL_MARKER == 0 {
        return None;
    }

    let low_bits = (encoded & 0xffff) as u16;
    Some(i16::from_le_bytes(low_bits.to_le_bytes()))
}

fn format_debug_var_location(location: &DebugVarLocation) -> String {
    if is_debug_var_kill_location(location) {
        "di.debug_kill".to_string()
    } else if let DebugVarLocation::FrameBase {
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

fn is_debug_var_kill_location(location: &DebugVarLocation) -> bool {
    matches!(
        location,
        DebugVarLocation::Expression(expression) if expression == DEBUG_VAR_KILL_SENTINEL
    )
}

fn print_summary(debug_info: &PackageDebugInfo) {
    println!("Summary:");
    println!();

    println!("Strings:");
    println!("  | records: {}", &debug_info.strings().len());
    println!();

    println!("Types:");
    println!("  | records: {}", &debug_info.types().len());
    println!();

    println!("Functions:");
    println!("  | records:          {}", &debug_info.functions().len());
    println!(
        "  | with source info: {}",
        &debug_info.functions().iter().filter(|f| f.source_node.is_some()).count()
    );
    println!(
        "  | w/o source info:  {}",
        &debug_info.functions().iter().filter(|f| f.source_node.is_none()).count()
    );
    println!();

    println!("Source Files:");
    println!("  | records: {}", &debug_info.files().len());
    println!();

    println!("Locations:");
    println!("  | records: {}", &debug_info.locations().len());
    println!();

    let (total_vars, total_inlined) =
        debug_info
            .nodes()
            .iter()
            .fold((0usize, 0usize), |(acc_vars, acc_inlined), node| {
                (acc_vars + node.debug_vars.len(), acc_inlined + node.inline_calls.len())
            });

    println!("Source Nodes:");
    println!("  | records: {}", &debug_info.nodes().len());
    println!("  | roots:   {}", &debug_info.roots().len());
    println!("  | debug variables (total): {total_vars}");
    println!("  | inline calls (total):    {total_inlined}");
    println!();

    // Count debug vars in MAST
    println!("Found {total_vars} debug variable records");
}

fn print_strings(debug_info: &PackageDebugInfo) {
    println!(".debug_str contents:");
    println!("{:-<80}", "");

    for (idx, s) in debug_info.strings().iter().enumerate() {
        println!("  [{:4}] \"{}\"", idx, s);
    }
}

fn print_types(debug_info: &PackageDebugInfo, raw: bool) {
    println!(".debug_types contents:");
    println!("{:-<80}", "");
    for (idx, ty) in debug_info.types().iter().enumerate() {
        print!("  [{:4}] ", idx);
        print_type(ty, debug_info, raw, 0);
        println!();
    }
}

fn print_type(ty: &DebugTypeInfo, debug_info: &PackageDebugInfo, raw: bool, indent: usize) {
    let pad = "  ".repeat(indent);
    match ty {
        DebugTypeInfo::Primitive(prim) => {
            print!("{}PRIMITIVE: {}", pad, primitive_name(*prim));
            //print!(" (size: {} bytes, {} felts)", prim.size_in_bytes(), prim.size_in_felts());
        }
        DebugTypeInfo::Pointer { pointee_type_idx } => {
            if raw {
                print!("{}POINTER -> type[{pointee_type_idx}]", pad);
            } else {
                print!("{}POINTER -> ", pad);
                if let Some(pointee) = debug_info.get_type(*pointee_type_idx) {
                    print_type_brief(pointee, debug_info);
                } else {
                    print!("<invalid type idx {pointee_type_idx}>");
                }
            }
        }
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => {
            if raw {
                print!("{}ARRAY [{element_type_idx}; {:?}]", pad, count);
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
                format!("str[{}]", name_idx).into_boxed_str().into()
            } else {
                debug_info.get_string(*name_idx).unwrap_or_else(|| "<unknown>".into())
            };
            print!("{}STRUCT {} (size: {} bytes)", pad, name, size);
            if !fields.is_empty() {
                println!();
                for field in fields {
                    let field_name = if raw {
                        format!("str[{}]", field.name_idx).into_boxed_str().into()
                    } else {
                        debug_info.get_string(field.name_idx).unwrap_or_else(|| "<unknown>".into())
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
        DebugTypeInfo::Enum {
            name_idx,
            size,
            discriminant_type_idx,
            variants,
        } => {
            let name = if raw {
                format!("str[{name_idx}]").into_boxed_str().into()
            } else {
                debug_info.get_string(*name_idx).unwrap_or_else(|| "<unknown>".into())
            };
            print!("{}ENUM {} (discriminant: ", pad, name);
            match debug_info.get_type(*discriminant_type_idx) {
                Some(discrim_ty) => print_type_brief(discrim_ty, debug_info),
                None => print!("unknown"),
            };
            print!(", size: {} bytes)", size);
            if !variants.is_empty() {
                println!();
            }
            for variant in variants.iter() {
                let name = if raw {
                    format!("str[{}]", variant.name_idx).into_boxed_str().into()
                } else {
                    debug_info.get_string(variant.name_idx).unwrap_or_else(|| "<unknown>".into())
                };
                print!("{}    +{:4}: {}", pad, variant.payload_offset.unwrap_or(0), name);
                if let Some(vty) = variant.type_idx {
                    print!(" : ");
                    if let Some(vty) = debug_info.get_type(vty) {
                        print_type_brief(vty, debug_info);
                    } else {
                        print!("<invalid type>");
                    }
                }
                println!(" = {}", &variant.discriminant);
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
                    print!("type[{param_idx}]");
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
                        print!("type[{idx}]");
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

fn print_type_brief(ty: &DebugTypeInfo, debug_info: &PackageDebugInfo) {
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
            print!("struct {}", debug_info.get_string(*name_idx).unwrap_or_else(|| "?".into()));
        }
        DebugTypeInfo::Enum { name_idx, .. } => {
            print!("enum {}", debug_info.get_string(*name_idx).unwrap_or_else(|| "?".into()));
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
        DebugPrimitiveType::U256 => "u256",
        DebugPrimitiveType::F32 => "f32",
        DebugPrimitiveType::F64 => "f64",
        DebugPrimitiveType::Felt => "felt",
        DebugPrimitiveType::Word => "word",
    }
}

fn print_files(debug_info: &PackageDebugInfo, raw: bool) {
    println!(".debug_files contents:");
    println!("{:-<80}", "");
    for (idx, file) in debug_info.files().iter().enumerate() {
        print_file(idx, file, debug_info, raw);
    }
}

fn print_file(idx: usize, file: &DebugFileInfo, debug_info: &PackageDebugInfo, raw: bool) {
    let path = if raw {
        format!("str[{}]", file.path_idx).into_boxed_str().into()
    } else {
        debug_info.get_string(file.path_idx).unwrap_or_else(|| "<unknown>".into())
    };

    print!("  [{idx:4}] {path}");

    if let Some(checksum) = file.checksum() {
        print!(" [checksum: ");
        for byte in &checksum[..4] {
            print!("{:02x}", byte);
        }
        print!("...]");
    }

    println!();
}

fn print_functions(
    debug_info: &PackageDebugInfo,
    mast_forest: &MastForest,
    raw: bool,
    verbose: bool,
) {
    println!(".debug_functions contents:");
    println!("{:-<80}", "");
    for (idx, func) in debug_info.functions().iter().enumerate() {
        print_function(idx, func, debug_info, mast_forest, raw, verbose);
        println!();
    }
}

fn print_function(
    idx: usize,
    func: &DebugFunctionInfo,
    debug_info: &PackageDebugInfo,
    mast_forest: &MastForest,
    raw: bool,
    verbose: bool,
) {
    let name = if raw {
        format!("str[{}]", func.name_idx).into_boxed_str().into()
    } else {
        debug_info.get_string(func.name_idx).unwrap_or_else(|| "<unknown>".into())
    };

    println!("  [{idx:4}] FUNCTION: {name}");

    // Linkage name
    if let Some(linkage_idx) = func.linkage_name_idx {
        let linkage = if raw {
            format!("str[{}]", linkage_idx).into_boxed_str().into()
        } else {
            debug_info.get_string(linkage_idx).unwrap_or_else(|| "<unknown>".into())
        };
        println!("         Linkage name: {linkage}");
    }

    // Location
    let file_path = if raw {
        format!("file[{}]", func.file_idx).into_boxed_str().into()
    } else {
        debug_info
            .get_file(func.file_idx)
            .and_then(|f| debug_info.get_string(f.path_idx))
            .unwrap_or_else(|| "<unknown>".into())
    };
    println!("         Location: {file_path}:{}:{}", func.line, func.column);

    // Type
    if let Some(type_idx) = func.type_idx {
        print!("         Type: ");
        if raw {
            println!("type[{type_idx}]");
        } else if let Some(ty) = debug_info.get_type(type_idx) {
            print_type_brief(ty, debug_info);
            println!();
        } else {
            println!("<invalid>");
        }
    }

    // MAST root
    print!("         MAST root: 0x");
    for byte in func.mast_root.as_bytes() {
        print!("{byte:02x}");
    }
    println!();

    // Variables
    let source_node_id = func.source_node.or_else(|| {
        mast_forest.find_procedure_root(func.mast_root).and_then(|exec_node| {
            debug_info.unique_source_root_for_exec_node(exec_node).ok().flatten()
        })
    });
    if let Some(source_node_id) = source_node_id {
        let source_node = &debug_info[source_node_id];
        if !source_node.debug_vars.is_empty() {
            println!("         Variables ({}):", source_node.debug_vars.len());
            for var in source_node.debug_vars.iter() {
                print_variable(var, debug_info, raw, verbose);
            }
        }
        // Inlined calls
        if !source_node.inline_calls.is_empty() && verbose {
            println!("         Inlined calls ({}):", source_node.inline_calls.len());
            for call in source_node.inline_calls.iter() {
                let callee = if raw {
                    format!("func[{}]", call.callee_idx).into_boxed_str().into()
                } else {
                    debug_info
                        .get_function(call.callee_idx)
                        .and_then(|f| debug_info.get_string(f.name_idx))
                        .unwrap_or_else(|| "<unknown>".into())
                };
                let loc = debug_info.get_location(call.loc_idx);
                let call_file = if raw {
                    format!("loc[{}]", call.loc_idx).into_boxed_str().into()
                } else {
                    loc.clone()
                        .map(|loc| Arc::<str>::from(loc.uri))
                        .unwrap_or_else(|| "<unknown>".into())
                };
                if let Some(loc) = loc {
                    println!(
                        "           - {callee} inlined at {call_file}:{}..{}",
                        loc.start, loc.end
                    );
                } else {
                    println!("           - {callee} inlined at {call_file}:?..?",);
                }
            }
        }
    }
}

fn print_variable(var: &DebugSourceVar, debug_info: &PackageDebugInfo, raw: bool, _verbose: bool) {
    let kind = if let Some(index) = var.arg_idx {
        format!("param #{index}")
    } else {
        "local".to_string()
    };

    print!("           - {} ({}): ", debug_info[var.name_idx].as_ref(), kind);

    if let Some(type_id) = var.type_id {
        if raw {
            print!("type[{type_id}]");
        } else if let Some(ty) = debug_info.get_type(DebugTypeIdx::from(type_id)) {
            print_type_brief(ty, debug_info);
        } else {
            print!("<invalid type>");
        }
    } else {
        print!("<invalid type>");
    }

    if let Some(loc) = var.location_idx.and_then(|loc| debug_info.get_location(loc)) {
        print!(" at {}:{}..{}", loc.uri(), loc.start, loc.end);
    }

    // TODO(pauls): Restore once scope info is available again
    /*
    if var.scope_depth > 0 {
        print!(" [scope depth: {}]", var.scope_depth);
    }
    */

    println!();
}

fn print_variables(debug_info: &PackageDebugInfo, mast_forest: &MastForest, raw: bool) {
    println!(".debug_variables contents (all functions):");
    println!("{:-<80}", "");
    for function in debug_info.functions() {
        let Some(source_node_id) = function.source_node.or_else(|| {
            mast_forest.find_procedure_root(function.mast_root).and_then(|exec_node| {
                debug_info.unique_source_root_for_exec_node(exec_node).ok().flatten()
            })
        }) else {
            continue;
        };
        let vars = debug_info.debug_vars_for_source_node(source_node_id).collect::<Vec<_>>();
        if vars.is_empty() {
            continue;
        }
        let func_name =
            debug_info.get_string(function.name_idx).unwrap_or_else(|| "<unknown>".into());
        println!("  Function: {func_name}");
        for var in vars {
            print_variable(var, debug_info, raw, /*verbose=*/ false);
        }
        println!();
    }
}

/// Prints the .debug_loc section - variable location entries from MAST
///
/// This is analogous to DWARF's .debug_loc section which contains location
/// lists describing where a variable's value can be found at runtime.
fn print_locations(debug_info: &PackageDebugInfo) {
    println!(".debug_loc contents (DebugLoc entries from MAST):");
    println!("{:-<80}", "");

    // Collect all debug vars from the MastForest

    // Group by variable name for a cleaner view
    let mut by_name: BTreeMap<Arc<str>, BTreeMap<DebugSourceNodeId, Vec<&DebugSourceVar>>> =
        BTreeMap::new();
    let mut total_count = 0usize;
    for (node_id, node) in debug_info.nodes().iter().enumerate() {
        if node.debug_vars.is_empty() {
            continue;
        }
        let node_id = DebugSourceNodeId::from(node_id as u32);

        total_count += node.debug_vars.len();
        for var in node.debug_vars.iter() {
            by_name
                .entry(debug_info[var.name_idx].clone())
                .or_default()
                .entry(node_id)
                .or_default()
                .push(var);
        }
    }

    println!("  Total DebugVar entries: {total_count}");
    println!("  Unique variable names: {}", by_name.len());
    println!();

    for (name, entries_by_node) in &by_name {
        println!("  Variable: \"{}\"", name);
        println!(
            "  {} location entries:",
            entries_by_node.values().map(|entries| entries.len()).sum::<usize>()
        );

        for (node_id, info) in entries_by_node
            .iter()
            .flat_map(|(nid, entries)| entries.iter().map(|e| (*nid, e)))
        {
            print!("    [node#{node_id}] ");

            // Print value location
            print!("{}", format_debug_var_location(&info.value_location));

            // Print argument info if present
            if let Some(arg_idx) = info.arg_idx {
                print!(" (param #{})", arg_idx);
            }

            // Print type info if present and we can resolve it
            if let Some(type_id) = info.type_id {
                let type_idx = DebugTypeIdx::from(type_id);
                if let Some(ty) = debug_info.get_type(type_idx) {
                    print!(" : ");
                    print_type_brief(ty, debug_info);
                } else {
                    print!(" : type[{type_idx}]");
                }
            }

            // Print source location if present
            if let Some(loc) = info.location_idx.and_then(|idx| debug_info.get_location(idx)) {
                print!(" @ {}:{}..{}", loc.uri, loc.start, loc.end);
            }

            println!();
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_debug_kill_sentinel() {
        let location = DebugVarLocation::Expression(DEBUG_VAR_KILL_SENTINEL.to_vec());

        assert_eq!(format_debug_var_location(&location), "di.debug_kill");
    }
}
