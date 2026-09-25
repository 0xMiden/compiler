//! Names of the core-module functions behind component exports and imports.
//!
//! A core function that backs a lifted component export, or that lowers a component import, is
//! named after the Miden path of that export or import, so no component-model name (the core
//! export `<interface id>#<function>`, the core import field) reaches the core module.

use alloc::{format, string::String, vec::Vec};

use midenc_hir::{FxHashMap, FxHashSet, SmallVec, SymbolName, SymbolNameComponent, SymbolPath};
use midenc_session::diagnostics::Report;

use crate::{
    error::WasmResult,
    module::{Module, types::FuncIndex},
};

/// Returns the HIR names of the functions of `module` that back the lifted `exports` or lower
/// the component `imports`, each given as the core function and the Miden path it is named after.
///
/// Exports are named first, in the given order, then imports in function index order. A core
/// function named once keeps that name (two exports lifting one function, or an export of an
/// imported function). Otherwise the name is the first free candidate among the path's leaf and
/// the leaf prefixed by progressively more of its parent segments (`read`, `api_read`,
/// `second_api_read`, ...). A name is free when no function outside the named set, no global and
/// no already named function holds it. Fails when every candidate is taken.
pub(crate) fn assign<'p>(
    module: &Module,
    exports: impl IntoIterator<Item = (FuncIndex, &'p SymbolPath)>,
    imports: impl IntoIterator<Item = (FuncIndex, &'p SymbolPath)>,
) -> WasmResult<FxHashMap<FuncIndex, SymbolName>> {
    let mut imports: Vec<_> = imports.into_iter().collect();
    imports.sort_by_key(|(index, _)| *index);
    let requests: Vec<_> = exports.into_iter().chain(imports).collect();

    // Every function in `requests` is renamed below, so only its assigned name can block others.
    let named: FxHashSet<FuncIndex> = requests.iter().map(|(index, _)| *index).collect();
    let mut taken: FxHashSet<SymbolName> = module
        .functions
        .keys()
        .filter(|index| !module.is_imported_function(*index) && !named.contains(index))
        .map(|index| module.func_name(index))
        .chain(module.globals.keys().map(|index| module.global_name(index)))
        .collect();
    let mut names = FxHashMap::default();

    for (index, path) in requests {
        if names.contains_key(&index) {
            continue;
        }
        let candidates = candidates(path);
        let name =
            candidates.iter().copied().find(|name| !taken.contains(name)).ok_or_else(|| {
                let quoted: Vec<String> =
                    candidates.iter().map(|name| format!("`{name}`")).collect();
                let listed = match quoted.split_last() {
                    Some((last, [])) => format!("{last} is"),
                    Some((last, rest)) => format!("{} and {last} are all", rest.join(", ")),
                    None => unreachable!("the leaf is always a candidate"),
                };
                Report::msg(format!(
                    "cannot name the core function of `{path}` in module `{}`: {listed} taken",
                    module.name().as_str()
                ))
            })?;
        taken.insert(name);
        names.insert(index, name);
    }
    Ok(names)
}

/// Returns the candidate names for a core function named after `path`, in order of preference:
/// the leaf, then the leaf prefixed by one more parent segment at a time, from the innermost.
fn candidates(path: &SymbolPath) -> SmallVec<[SymbolName; 4]> {
    let mut name = String::from(path.name().as_str());
    let mut candidates = SmallVec::from_iter([path.name()]);
    let parent = path.without_leaf();
    let segments: SmallVec<[SymbolName; 4]> = parent
        .components()
        .filter_map(|component| match component {
            SymbolNameComponent::Component(segment) => Some(segment),
            _ => None,
        })
        .collect();
    for segment in segments.iter().rev() {
        name = format!("{segment}_{name}");
        candidates.push(SymbolName::intern(&name));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use wasmparser::Validator;

    use super::*;
    use crate::{
        WasmTranslationConfig,
        component::naming::external_id_path,
        module::{module_env::ModuleEnvironment, types::ModuleTypesBuilder},
    };

    /// Parses the core module `wat`.
    fn module_of(wat: &str) -> Module {
        let wasm: &'static [u8] =
            Box::leak(wat::parse_str(wat).expect("module WAT should compile").into_boxed_slice());
        let config = WasmTranslationConfig::default();
        let mut validator = Validator::new_with_features(crate::supported_features());
        let mut types = ModuleTypesBuilder::default();
        ModuleEnvironment::new(&config, &mut validator, &mut types)
            .parse(wasmparser::Parser::new(0), wasm, &Default::default())
            .expect("module should parse")
            .module
    }

    fn path(path: &str) -> SymbolPath {
        external_id_path("iface", "func", Some(path)).expect("valid Miden path")
    }

    fn func(index: u32) -> FuncIndex {
        FuncIndex::from_u32(index)
    }

    /// A module importing `read` (function 0) and defining `sum` (1), `api_read` (2) and the
    /// global `get`.
    const MODULE: &str = r#"
        (module $m
            (import "acme:first/api" "read" (func $read (result i32)))
            (global $get (mut i32) (i32.const 0))
            (func $sum (result i32) call $read)
            (func $api_read (result i32) i32.const 0)
            (export "acme:app/app#sum" (func $sum))
        )
    "#;

    #[test]
    fn candidates_add_one_parent_segment_at_a_time() {
        let names: Vec<_> = candidates(&path("acme::second::api::read"))
            .into_iter()
            .map(|name| name.as_str().to_owned())
            .collect();
        assert_eq!(names, ["read", "api_read", "second_api_read", "acme_second_api_read"]);
    }

    #[test]
    fn a_function_may_keep_its_own_name_and_skips_taken_ones() {
        let module = module_of(MODULE);
        let sum = path("acme::app::app::sum");
        let get = path("acme::app::app::get");
        let names = assign(&module, [(func(1), &sum), (func(2), &get)], []).unwrap();
        assert_eq!(names[&func(1)].as_str(), "sum");
        // `get` names the global, so the export takes the qualified name.
        assert_eq!(names[&func(2)].as_str(), "app_get");
    }

    #[test]
    fn renamed_functions_free_their_own_names() {
        let module = module_of(
            r#"
            (module $m
                (func $a)
                (func $b)
            )
        "#,
        );
        let a = path("acme::app::app::a");
        let b = path("acme::app::app::b");
        // `$b` is exported as `a` and `$a` as `b`.
        let names = assign(&module, [(func(1), &a), (func(0), &b)], []).unwrap();
        assert_eq!(names[&func(1)].as_str(), "a");
        assert_eq!(names[&func(0)].as_str(), "b");
    }

    #[test]
    fn the_first_export_of_a_function_names_it() {
        let module = module_of(MODULE);
        let first = path("acme::app::app::first");
        let second = path("acme::app::app::second");
        let names = assign(&module, [(func(1), &first), (func(1), &second)], []).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[&func(1)].as_str(), "first");
    }

    #[test]
    fn exports_take_names_before_imports() {
        let module = module_of(MODULE);
        let export = path("acme::app::app::read");
        let import = path("acme::first::api::read");
        let names = assign(&module, [(func(1), &export)], [(func(0), &import)]).unwrap();
        assert_eq!(names[&func(1)].as_str(), "read");
        // `read` names the export and `api_read` another defined function.
        assert_eq!(names[&func(0)].as_str(), "first_api_read");

        // A re-exported import keeps the export's name.
        let names = assign(&module, [(func(0), &export)], [(func(0), &import)]).unwrap();
        assert_eq!(names[&func(0)].as_str(), "read");
    }

    #[test]
    fn every_candidate_taken_is_an_error() {
        let module = module_of(MODULE);
        let read = path("api::read");
        // The export takes `read`; `api_read` names another defined function.
        let err = assign(&module, [(func(1), &read)], [(func(0), &read)])
            .expect_err("the import has no free name")
            .to_string();
        assert_eq!(
            err,
            "cannot name the core function of `::api::read` in module `m`: `read` and `api_read` \
             are all taken"
        );
    }
}
