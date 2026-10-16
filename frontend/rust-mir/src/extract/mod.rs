//! Reads the MIR of the analyzed crate through `rustc_public` and converts it into the plain
//! data of [crate::extracted].
//!
//! This is the only part of the frontend that reads `rustc_public` types, and it runs inside the
//! `rustc_public` driver callback. It also gives all errors about MIR constructs that the frontend
//! does not support, each of which names the construct.
//!
//! The errors are plain strings, because they cross from the rustc thread to the caller thread.

mod body;
mod operand;
mod rvalue;
mod statement;
mod terminator;
mod types;

use rustc_public::{CrateDef, ItemKind, mir::mono::Instance};

use crate::extracted::ExtractedCrate;

/// The result of an extraction step. The error describes the problem.
type ExtractResult<T> = Result<T, String>;

/// Extracts every local, non-generic function of the analyzed crate.
///
/// The caller must call this function inside a `rustc_public` driver callback, because the MIR
/// is only reachable there.
pub(crate) fn extract_crate() -> ExtractResult<ExtractedCrate> {
    let crate_name = rustc_public::local_crate().name;
    let mut functions = Vec::new();

    for item in rustc_public::all_local_items() {
        if item.kind() != ItemKind::Fn {
            continue;
        }
        // A generic item has no single body, thus it is skipped. Monomorphization is a later
        // milestone.
        let Ok(instance) = Instance::try_from(item) else {
            continue;
        };
        let Some(mir_body) = instance.body() else {
            continue;
        };
        let symbol = function_symbol(&item.name(), &crate_name);
        functions.push(body::extract_function(symbol, &mir_body)?);
    }

    Ok(ExtractedCrate { functions })
}

/// Returns the Miden IR symbol of a Rust item path.
///
/// The symbol keeps the module path, thus functions with the same name in different modules
/// get different symbols. The leading crate name is removed, because the Miden IR module of the
/// crate already has that name.
fn function_symbol(path: &str, crate_name: &str) -> String {
    match path.strip_prefix(crate_name).and_then(|rest| rest.strip_prefix("::")) {
        Some(rest) => rest.to_string(),
        None => path.to_string(),
    }
}
