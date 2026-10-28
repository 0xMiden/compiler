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

use std::collections::HashMap;

use rustc_public::{CrateDef, ItemKind, mir::mono::Instance};

use crate::extracted::{ExtractedCrate, ExtractedFunction};

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
    disambiguate_symbols(&mut functions);

    Ok(ExtractedCrate { functions })
}

/// Returns the Miden IR symbol of a Rust item path.
///
/// The symbol keeps the module path, thus functions with the same name in different modules
/// get different symbols. The leading crate name is removed, because the Miden IR module of the
/// crate already has that name. This symbol is not always unique. [disambiguate_symbols] makes
/// it unique.
fn function_symbol(path: &str, crate_name: &str) -> String {
    match path.strip_prefix(crate_name).and_then(|rest| rest.strip_prefix("::")) {
        Some(rest) => rest.to_string(),
        None => path.to_string(),
    }
}

/// Makes the symbols of the extracted functions unique.
///
/// The symbol of a function is its display path. The display path of an item that is nameable
/// from outside its scope is unique, because Rust does not allow two such items with the same
/// path. Items in anonymous scopes, such as function bodies and blocks, can have the same display
/// path. Thus only these items can have the same symbol.
///
/// When two or more functions have the same symbol, each of them gets the suffix `#<ordinal>`.
/// The ordinal starts at 0 and follows the order of the functions. For example, two block-local
/// functions `helper` in `outer` get `outer::helper#0` and `outer::helper#1`. A unique symbol
/// does not change. The same functions in the same order always get the same symbols.
fn disambiguate_symbols(functions: &mut [ExtractedFunction]) {
    let mut group_sizes = HashMap::<String, usize>::new();
    for function in functions.iter() {
        *group_sizes.entry(function.symbol.clone()).or_default() += 1;
    }

    // Rust display paths can contain `#`, but under this driver a function's display path never
    // ends in an unbraced `#<decimal>` suffix, so the generated symbols cannot collide with one.
    let mut next_ordinals = HashMap::<String, usize>::new();
    for function in functions.iter_mut() {
        if group_sizes[&function.symbol] > 1 {
            let ordinal = next_ordinals.entry(function.symbol.clone()).or_default();
            function.symbol = format!("{}#{ordinal}", function.symbol);
            *ordinal += 1;
        }
    }
}
