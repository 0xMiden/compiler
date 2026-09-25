//! Miden paths of component functions.
//!
//! Every component-level function is named by the Miden path carried in its component-model
//! `external-id` attribute (`@external-id("...")` in WIT). The component-model interface and
//! function names only pair `canon lift`/`canon lower` with core functions and appear in
//! diagnostics.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use midenc_frontend_wasm_metadata::COMPONENT_INIT_PROCEDURE;
use midenc_hir::{FxHashMap, SymbolName, SymbolNameComponent, SymbolPath};
use midenc_session::diagnostics::Report;

use crate::{component::StaticComponentIndex, error::WasmResult};

/// Returns the Miden path carried by the `external_id` of the component function `cm_name` of the
/// component-model interface `cm_iface`.
///
/// Fails when the attribute is missing, or when its value is not an absolute Miden path with a
/// module and a function name whose segments are bare identifiers (a leading `::` is accepted).
pub(crate) fn external_id_path(
    cm_iface: &str,
    cm_name: &str,
    external_id: Option<&str>,
) -> WasmResult<SymbolPath> {
    let Some(external_id) = external_id else {
        return Err(Report::msg(format!(
            "function `{cm_name}` of interface `{cm_iface}` has no `@external-id`; every \
             component function needs its Miden path"
        )));
    };
    let value = external_id.strip_prefix("::").unwrap_or(external_id);
    let is_bare_identifier = |segment: &str| {
        !segment.is_empty() && segment.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    };
    let Some((module, function)) =
        value.rsplit_once("::").filter(|_| value.split("::").all(is_bare_identifier))
    else {
        return Err(Report::msg(format!(
            "the `@external-id` of function `{cm_name}` of interface `{cm_iface}` is \
             `{external_id}`, which is not an absolute Miden path with a module and a function \
             name whose segments are ASCII letters, digits and `_`, e.g. \
             `miden::counter_contract::counter_contract::get_count`"
        )));
    };
    let mut path = SymbolPath::from_masm_module_id(module);
    path.path.push(SymbolNameComponent::Leaf(SymbolName::intern(function)));
    Ok(path)
}

/// The Miden paths of the function exports of the nested components, keyed by the nested
/// component and the component-model name of the export.
pub(crate) type ExportPaths<'a> = FxHashMap<(StaticComponentIndex, &'a str), SymbolPath>;

/// Returns the namespace shared by `exports`, the component-model names of the component's
/// function exports paired with their Miden paths, or `None` when there are none.
///
/// Fails when two functions share a path, when a function takes the path of the compiler's
/// component initializer (`<namespace>::init`), or when the functions are not all under the same
/// namespace.
pub(crate) fn exports_namespace<'a>(
    exports: impl IntoIterator<Item = (&'a str, &'a SymbolPath)>,
) -> WasmResult<Option<SymbolPath>> {
    let mut seen: FxHashMap<&'a SymbolPath, &'a str> = FxHashMap::default();
    let mut namespaces: Vec<SymbolPath> = Vec::new();
    for (name, path) in exports {
        if let Some(previous) = seen.insert(path, name) {
            return Err(Report::msg(format!(
                "component functions `{previous}` and `{name}` share the Miden path `{path}`"
            )));
        }
        // Codegen emits the component initializer as the public `init` procedure next to the
        // exports, so an export with that leaf would silently collide with it.
        if path.name().as_str() == COMPONENT_INIT_PROCEDURE {
            return Err(Report::msg(format!(
                "export `{name}` uses the path `{path}`, which is reserved for the compiler's \
                 component initializer"
            )));
        }
        let namespace = path.without_leaf().into_owned();
        if !namespaces.contains(&namespace) {
            namespaces.push(namespace);
        }
    }
    match namespaces.len() {
        0 => Ok(None),
        1 => Ok(namespaces.pop()),
        _ => Err(Report::msg(format!(
            "the component's exports are under different namespaces ({}); every export must be \
             `<namespace>::<name>` for one namespace",
            namespaces.iter().map(|ns| format!("`{ns}`")).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// The error for the function export `name` of the root component, which no interface carries.
pub(crate) fn world_level_function_export(name: &str) -> Report {
    Report::msg(format!(
        "world-level function export `{name}` is not supported; export it from an interface"
    ))
}

/// Names the interface a nested component's exports go through, for diagnostics.
///
/// The root component exports the instances of its nested components under their interface
/// names; with a single one it is unambiguous, otherwise the nested component's
/// `StaticComponentIndex` is used.
pub(crate) fn interface_hint(root_instance_exports: &[&str], nested_component: u32) -> String {
    match root_instance_exports {
        [single] => single.to_string(),
        _ => format!("<nested component {nested_component}>"),
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, rc::Rc, string::String};

    use midenc_hir::{Context, Op, Operation, SymbolName, SymbolTable, dialects::builtin::Module};

    use super::*;
    use crate::{WasmTranslationConfig, translate};

    /// A component lifting one core function, named `miden:counter/counter@0.1.0#get-count` in the
    /// name section, and exporting it as `get-count` through the interface
    /// `miden:counter/counter@0.1.0`, with `attribute` on the nested export.
    fn counter_component(attribute: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"
            (component
                (core module $m
                    (func $miden:counter/counter@0.1.0#get-count (export "get-count")
                        (result i32) i32.const 0)
                )
                (core instance $i (instantiate $m))
                (func $lifted (result u32) (canon lift (core func $i "get-count")))
                (component $shim
                    (import "import-func-get-count" (func $f (result u32)))
                    (export "get-count" {attribute} (func $f))
                )
                (instance $exports
                    (instantiate $shim (with "import-func-get-count" (func $lifted))))
                (export "miden:counter/counter@0.1.0" (instance $exports))
            )
            "#
        ))
        .expect("component WAT should compile")
    }

    /// Translates `wasm` in `context` (which owns the resulting IR), rooted at `namespace` when
    /// given.
    fn translate_with(
        context: &Rc<Context>,
        wasm: &[u8],
        namespace: Option<&str>,
    ) -> WasmResult<crate::FrontendOutput> {
        let config = WasmTranslationConfig {
            namespace: namespace.map(SymbolPath::from_masm_module_id),
            ..Default::default()
        };
        translate(wasm, &config, context.clone())
    }

    /// Returns the namespace the exports of `wasm` declare, as the compiler scans it before
    /// translation.
    fn declared_namespace(wasm: &[u8]) -> WasmResult<Option<SymbolPath>> {
        crate::declared_namespace(wasm, Context::default().session())
    }

    /// Returns the diagnostic of translating `wasm` (rooted at `namespace` when given), which
    /// must fail.
    fn error_of(wasm: &[u8], namespace: Option<&str>) -> String {
        match translate_with(&Rc::default(), wasm, namespace) {
            Ok(_) => panic!("translation should fail"),
            Err(err) => err.to_string(),
        }
    }

    /// Returns the printed core modules of the translated component `output`.
    fn core_modules_of(output: &crate::FrontendOutput) -> String {
        let mut hir = String::new();
        output.component.borrow().as_operation().prewalk_all(|op: &Operation| {
            if op.is::<Module>() {
                hir.push_str(&op.to_string());
            }
        });
        hir
    }

    /// A component importing `read` from both `acme:first/api` and `acme:second/api`, with the
    /// core imports routed through a wit-component style indirection shim and fixup module.
    fn two_interfaces_through_shim_component() -> Vec<u8> {
        wat::parse_str(
            r#"
            (component
                (import "acme:first/api" (instance $first
                    (export "read" (external-id "acme::first::api::read") (func (result u32)))
                ))
                (import "acme:second/api" (instance $second
                    (export "read" (external-id "acme::second::api::read") (func (result u32)))
                ))
                (alias export $first "read" (func $first-read))
                (alias export $second "read" (func $second-read))
                (core module $main
                    (import "acme:first/api" "read" (func $first (result i32)))
                    (import "acme:second/api" "read" (func $second (result i32)))
                    (memory (export "memory") 1)
                    (func (export "sum") (result i32) call $first call $second i32.add)
                )
                (core module $shim
                    (type $read (func (result i32)))
                    (table (export "$imports") 2 2 funcref)
                    (func (export "0") (result i32) i32.const 0 call_indirect (type $read))
                    (func (export "1") (result i32) i32.const 1 call_indirect (type $read))
                )
                (core module $fixup
                    (type $read (func (result i32)))
                    (import "" "0" (func (type $read)))
                    (import "" "1" (func (type $read)))
                    (import "" "$imports" (table 2 2 funcref))
                    (elem (table 0) (i32.const 0) func 0 1)
                )
                (core instance $shim-instance (instantiate $shim))
                (alias core export $shim-instance "0" (core func $shim-0))
                (alias core export $shim-instance "1" (core func $shim-1))
                (core instance $first-args (export "read" (func $shim-0)))
                (core instance $second-args (export "read" (func $shim-1)))
                (core instance $main-instance (instantiate $main
                    (with "acme:first/api" (instance $first-args))
                    (with "acme:second/api" (instance $second-args))
                ))
                (alias core export $main-instance "memory" (core memory $memory))
                (core func $lowered-first (canon lower (func $first-read)))
                (core func $lowered-second (canon lower (func $second-read)))
                (alias core export $shim-instance "$imports" (core table $imports))
                (core instance $fixup-args
                    (export "$imports" (table $imports))
                    (export "0" (func $lowered-first))
                    (export "1" (func $lowered-second))
                )
                (core instance (instantiate $fixup (with "" (instance $fixup-args))))
                (func $sum (result u32) (canon lift (core func $main-instance "sum")))
                (component $exports
                    (import "import-func-sum" (func $f (result u32)))
                    (export "sum" (external-id "acme::app::app::sum") (func $f))
                )
                (instance $app (instantiate $exports (with "import-func-sum" (func $sum))))
                (export "acme:app/app" (instance $app))
            )
            "#,
        )
        .expect("component WAT should compile")
    }

    #[test]
    fn shim_bypassed_imports_call_their_own_interface() {
        let context = Rc::default();
        let output = translate_with(&context, &two_interfaces_through_shim_component(), None)
            .expect("component should translate");
        let hir = core_modules_of(&output);
        assert!(
            hir.contains("hir.call ::@acme::@first::@api::@read()")
                && hir.contains("hir.call ::@acme::@second::@api::@read()"),
            "each import must call the procedure of its own interface:\n{hir}"
        );
        // Two imports with one leaf: the second import stub takes the parent-qualified name.
        assert!(
            hir.contains(r#"extern("C") @read()"#) && hir.contains(r#"extern("C") @api_read()"#),
            "the import stubs are named `read` and `api_read`:\n{hir}"
        );
    }

    #[test]
    fn an_export_whose_leaf_names_another_core_function_takes_a_qualified_name() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func $get_count (result i32) i32.const 1)
                    (func $miden:counter/counter@0.1.0#get-count
                        (export "miden:counter/counter@0.1.0#get-count") (result i32)
                        call $get_count)
                )
                (core instance $i (instantiate $m))
                (func $lifted (result u32)
                    (canon lift (core func $i "miden:counter/counter@0.1.0#get-count")))
                (component $shim
                    (import "import-func-get-count" (func $f (result u32)))
                    (export "get-count" (external-id "miden::counter::counter::get_count")
                        (func $f))
                )
                (instance $exports
                    (instantiate $shim (with "import-func-get-count" (func $lifted))))
                (export "miden:counter/counter@0.1.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @counter_get_count()"#)
                && hir.contains(r#"extern("C") @get_count()"#),
            "the export's core function takes the parent-qualified name:\n{hir}"
        );
        assert!(!hir.contains("get-count"), "no component-model name in the core module:\n{hir}");
    }

    /// One nested component instantiated twice and exported as two interfaces lifts its export
    /// path twice.
    #[test]
    fn one_component_exported_as_two_interfaces_is_rejected() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func (export "get-count") (result i32) i32.const 0)
                )
                (core instance $i (instantiate $m))
                (func $lifted (result u32) (canon lift (core func $i "get-count")))
                (component $shim
                    (import "import-func-get-count" (func $f (result u32)))
                    (export "get-count" (external-id "miden::counter::counter::get_count")
                        (func $f))
                )
                (instance $first
                    (instantiate $shim (with "import-func-get-count" (func $lifted))))
                (instance $second
                    (instantiate $shim (with "import-func-get-count" (func $lifted))))
                (export "miden:counter/first@0.1.0" (instance $first))
                (export "miden:counter/second@0.1.0" (instance $second))
            )
            "#,
        )
        .expect("component WAT should compile");
        let err = error_of(&wasm, None);
        assert!(
            err.contains(
                "component instances `miden:counter/first@0.1.0` and `miden:counter/second@0.1.0` \
                 both export `::miden::counter::counter::get_count`"
            ),
            "unexpected diagnostic: {err}"
        );
    }

    /// A function exported by the root component itself, rather than through an interface, is
    /// rejected by the frontend and by the namespace scan.
    #[test]
    fn a_world_level_function_export_is_rejected() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func (export "get-count") (result i32) i32.const 0)
                )
                (core instance $i (instantiate $m))
                (func $lifted (result u32) (canon lift (core func $i "get-count")))
                (export "get-count" (func $lifted))
            )
            "#,
        )
        .expect("component WAT should compile");
        let expected =
            "world-level function export `get-count` is not supported; export it from an interface";
        let err = error_of(&wasm, Some("miden::counter::counter"));
        assert!(err.contains(expected), "unexpected diagnostic: {err}");
        let err = declared_namespace(&wasm).expect_err("the scan should fail").to_string();
        assert!(err.contains(expected), "unexpected scan diagnostic: {err}");
    }

    /// A linker stub lifted as an export is defined under the export's leaf and still lowered to
    /// its intrinsic, which is recognized by the stub's Wasm name.
    #[test]
    fn a_lifted_linker_stub_is_lowered_to_its_intrinsic() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func $intrinsics::mem::heap_base (export "get-count") (result i32)
                        unreachable)
                )
                (core instance $i (instantiate $m))
                (func $lifted (result u32) (canon lift (core func $i "get-count")))
                (component $shim
                    (import "import-func-get-count" (func $f (result u32)))
                    (export "get-count" (external-id "miden::counter::counter::get_count")
                        (func $f))
                )
                (instance $exports
                    (instantiate $shim (with "import-func-get-count" (func $lifted))))
                (export "miden:counter/counter@0.1.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @get_count()"#),
            "the stub is defined under the export's leaf:\n{hir}"
        );
        assert!(hir.contains("heap_base"), "the stub calls its intrinsic:\n{hir}");
        assert!(!hir.contains("unreachable"), "the stub body is not translated:\n{hir}");
    }

    #[test]
    fn an_export_keeps_its_leaf_against_an_import_of_the_same_leaf() {
        let wasm = import_and_export_component("acme::first::api::read", "sum", "read");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @read()"#)
                && hir.contains(r#"extern("C") @api_read()"#)
                && !hir.contains("@sum("),
            "the export's core function is `read` and the import stub `api_read`:\n{hir}"
        );
    }

    #[test]
    fn exports_of_two_interfaces_lifted_after_each_other_are_all_lifted() {
        // wit-component order: the lift of the second interface follows the export of the first.
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func (export "acme:app/one#a") (result i32) i32.const 1)
                    (func (export "acme:app/two#b") (result i32) i32.const 2)
                )
                (core instance $i (instantiate $m))
                (alias core export $i "acme:app/one#a" (core func $core-a))
                (func $a (result u32) (canon lift (core func $core-a)))
                (component $c1
                    (import "import-func-a" (func $f (result u32)))
                    (export "a" (external-id "miden::app::app::a") (func $f))
                )
                (instance $one (instantiate $c1 (with "import-func-a" (func $a))))
                (export "acme:app/one" (instance $one))
                (alias core export $i "acme:app/two#b" (core func $core-b))
                (func $b (result u32) (canon lift (core func $core-b)))
                (component $c2
                    (import "import-func-b" (func $f (result u32)))
                    (export "b" (external-id "miden::app::app::b") (func $f))
                )
                (instance $two (instantiate $c2 (with "import-func-b" (func $b))))
                (export "acme:app/two" (instance $two))
            )
            "#,
        )
        .expect("component WAT should compile");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        {
            let component = output.component.borrow();
            for leaf in ["a", "b"] {
                assert!(
                    component.get(SymbolName::intern(leaf)).is_some(),
                    "the lifted export `{leaf}` is defined"
                );
            }
        }
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @a()"#)
                && hir.contains(r#"extern("C") @b()"#)
                && !hir.contains("acme:app"),
            "both core functions are named after their exports:\n{hir}"
        );
    }

    #[test]
    fn an_export_is_named_by_its_external_id() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::get_count")"#);
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        let component = output.component.borrow();
        assert_eq!(component.namespace_path().to_string(), "::miden::counter::counter");
        assert!(
            component.get(SymbolName::intern("get_count")).is_some(),
            "the lifted export is defined at the leaf of its Miden path"
        );
        assert_eq!(
            declared_namespace(&wasm).unwrap().map(|ns| ns.to_string()).as_deref(),
            Some("::miden::counter::counter")
        );
    }

    #[test]
    fn debug_info_names_subprograms_after_the_hir_functions() {
        use alloc::{boxed::Box, sync::Arc, vec::Vec};

        use midenc_hir::dialects::{builtin::Function, debuginfo::attributes::SubprogramAttr};
        use midenc_session::{
            DebugInfo, InputFile, Options, Session, diagnostics::DefaultSourceManager,
        };

        let options = Box::new(Options::default())
            .with_output_types(Default::default(), None)
            .with_debug_info(DebugInfo::Full);
        let session = Session::new(
            InputFile::empty(),
            options,
            None,
            Arc::new(DefaultSourceManager::default()),
        )
        .unwrap();
        let context = Rc::new(Context::new(Rc::new(session)));
        let wasm = counter_component(r#"(external-id "miden::counter::counter::get_count")"#);
        let output = translate_with(&context, &wasm, None).expect("component should translate");

        let mut subprograms = Vec::new();
        output.component.borrow().as_operation().prewalk_all(|op: &Operation| {
            let Some(function) = op.downcast_ref::<Function>() else {
                return;
            };
            if let Some(attr) = op.get_attribute("di.subprogram") {
                let subprogram = attr.try_downcast_attr::<SubprogramAttr>().unwrap();
                subprograms
                    .push((function.name().as_str(), subprogram.borrow().name.as_str().to_owned()));
            }
        });
        // The core function carries `miden:counter/counter@0.1.0#get-count` in the name section.
        assert!(
            !subprograms.is_empty()
                && subprograms.iter().all(|(function, subprogram)| {
                    function == subprogram && !subprogram.contains('#')
                }),
            "every subprogram is named after its HIR function: {subprograms:?}"
        );
        assert!(
            subprograms.iter().any(|(_, subprogram)| subprogram == "get_count"),
            "{subprograms:?}"
        );
    }

    #[test]
    fn two_exports_sharing_one_path_are_rejected() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func (export "get-count") (result i32) i32.const 0)
                    (func (export "read-count") (result i32) i32.const 1)
                )
                (core instance $i (instantiate $m))
                (func $get (result u32) (canon lift (core func $i "get-count")))
                (func $read (result u32) (canon lift (core func $i "read-count")))
                (component $shim
                    (import "import-func-get-count" (func $g (result u32)))
                    (import "import-func-read-count" (func $r (result u32)))
                    (export "get-count" (external-id "miden::counter::counter::get_count")
                        (func $g))
                    (export "read-count" (external-id "miden::counter::counter::get_count")
                        (func $r))
                )
                (instance $exports (instantiate $shim
                    (with "import-func-get-count" (func $get))
                    (with "import-func-read-count" (func $read))
                ))
                (export "miden:counter/counter@0.1.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let expected = "component functions `get-count` and `read-count` share the Miden path \
                        `::miden::counter::counter::get_count`";
        let err = error_of(&wasm, None);
        assert!(err.contains(expected), "unexpected diagnostic: {err}");
        let err = declared_namespace(&wasm)
            .expect_err("the pre-scan must reject the shared path")
            .to_string();
        assert!(err.contains(expected), "unexpected diagnostic: {err}");
    }

    #[test]
    fn an_export_without_external_id_is_rejected() {
        let err = error_of(&counter_component(""), None);
        assert!(
            err.contains("`get-count`")
                && err.contains("`miden:counter/counter@0.1.0`")
                && err.contains("has no `@external-id`"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn an_external_id_without_a_module_is_rejected() {
        let err = error_of(&counter_component(r#"(external-id "no-double-colon")"#), None);
        assert!(
            err.contains("`no-double-colon`") && err.contains("not an absolute Miden path"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn exports_outside_the_target_namespace_are_rejected() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::get_count")"#);
        let err = error_of(&wasm, Some("miden::other::other"));
        assert!(
            err.contains("`::miden::other::other`") && err.contains("`::miden::counter::counter`"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn external_id_path_rejects_empty_segments() {
        for value in ["a::::f", "::f", "a::", "f"] {
            assert!(external_id_path("i", "f", Some(value)).is_err(), "`{value}` must be rejected");
        }
        let path = external_id_path("i", "f", Some("::a::b::f")).expect("valid path");
        assert_eq!(path.to_string(), "::a::b::f");
        assert_eq!(path.name().as_str(), "f");
    }

    #[test]
    fn external_id_path_rejects_segments_that_are_not_bare_identifiers() {
        for value in [
            "miden::x::y::get-count",
            "miden::test::api::\"first\"",
            "miden:x::y::f",
            "miden::x/y::f",
            "miden::x::y@1::f",
            "miden::x::y#f",
        ] {
            let err = external_id_path("i", "f", Some(value))
                .expect_err("a segment that is not a bare identifier must be rejected")
                .to_string();
            assert!(
                err.contains(&format!("`{value}`"))
                    && err.contains("ASCII letters, digits and `_`"),
                "unexpected diagnostic: {err}"
            );
        }
        let path = external_id_path("i", "f", Some("::miden::x::y::get_count")).expect("valid");
        assert_eq!(path.to_string(), "::miden::x::y::get_count");
    }

    #[test]
    fn a_component_without_exports_or_namespace_is_rejected() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m (func (export "f")))
                (core instance $i (instantiate $m))
            )
            "#,
        )
        .expect("component WAT should compile");
        let err = error_of(&wasm, None);
        assert!(
            err.contains("exports no functions and no namespace was given")
                && err.contains("[lib].namespace"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn two_exports_can_lift_one_core_function() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $m
                    (func $count (result i32) i32.const 0)
                    (export "get-count" (func $count))
                    (export "read-count" (func $count))
                )
                (core instance $i (instantiate $m))
                (func $get (result u32) (canon lift (core func $i "get-count")))
                (func $read (result u32) (canon lift (core func $i "read-count")))
                (component $shim
                    (import "import-func-get-count" (func $g (result u32)))
                    (import "import-func-read-count" (func $r (result u32)))
                    (export "get-count" (external-id "miden::counter::counter::get_count")
                        (func $g))
                    (export "read-count" (external-id "miden::counter::counter::read_count")
                        (func $r))
                )
                (instance $exports (instantiate $shim
                    (with "import-func-get-count" (func $get))
                    (with "import-func-read-count" (func $read))
                ))
                (export "miden:counter/counter@0.1.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        {
            let component = output.component.borrow();
            for leaf in ["get_count", "read_count"] {
                assert!(
                    component.get(SymbolName::intern(leaf)).is_some(),
                    "the lifted export `{leaf}` is defined"
                );
            }
        }
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @get_count()"#)
                && !hir.contains(r#"extern("C") @read_count()"#),
            "the first export names the shared core function:\n{hir}"
        );
    }

    #[test]
    fn an_import_stub_avoids_the_name_of_a_global() {
        let wasm = wat::parse_str(
            r#"
            (component
                (import "acme:first/api" (instance $first
                    (export "read" (external-id "acme::first::api::read") (func (result u32)))
                ))
                (alias export $first "read" (func $first-read))
                (core func $lowered (canon lower (func $first-read)))
                (core instance $args (export "read" (func $lowered)))
                (core module $main
                    (import "acme:first/api" "read" (func $import (result i32)))
                    (global $read (mut i32) (i32.const 0))
                    (func (export "sum") (result i32) call $import global.get $read i32.add)
                )
                (core instance $main-instance
                    (instantiate $main (with "acme:first/api" (instance $args))))
                (func $sum (result u32) (canon lift (core func $main-instance "sum")))
                (component $exports
                    (import "import-func-sum" (func $f (result u32)))
                    (export "sum" (external-id "acme::app::app::sum") (func $f))
                )
                (instance $app (instantiate $exports (with "import-func-sum" (func $sum))))
                (export "acme:app/app" (instance $app))
            )
            "#,
        )
        .expect("component WAT should compile");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        let hir = core_modules_of(&output);
        assert!(
            hir.contains(r#"extern("C") @api_read()"#) && hir.contains("@read"),
            "the import stub takes the parent-qualified name next to the global `read`:\n{hir}"
        );
    }

    #[test]
    fn an_export_named_like_the_core_module_is_rejected() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::m")"#);
        let err = error_of(&wasm, None);
        assert!(
            err.contains(
                "export `m` of `::miden::counter::counter` clashes with the core module `m` of \
                 the same name"
            ),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn an_export_at_the_initializer_path_is_rejected() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::init")"#);
        let err = error_of(&wasm, None);
        assert!(
            err.contains(
                "export `get-count` uses the path `::miden::counter::counter::init`, which is \
                 reserved for the compiler's component initializer"
            ),
            "unexpected diagnostic: {err}"
        );
    }

    /// A component importing `read` from `acme:first/api` (returning `u32`) and from
    /// `acme:second/api` (returning `second_result`, lowered to the core type `second_core`),
    /// both carrying the external-id `acme::shared::api::read`.
    fn two_imports_of_one_path_component(second_result: &str, second_core: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"
            (component
                (import "acme:first/api" (instance $first
                    (export "read" (external-id "acme::shared::api::read") (func (result u32)))
                ))
                (import "acme:second/api" (instance $second
                    (export "read" (external-id "acme::shared::api::read")
                        (func (result {second_result})))
                ))
                (alias export $first "read" (func $first-read))
                (alias export $second "read" (func $second-read))
                (core func $lowered-first (canon lower (func $first-read)))
                (core func $lowered-second (canon lower (func $second-read)))
                (core instance $first-args (export "read" (func $lowered-first)))
                (core instance $second-args (export "read" (func $lowered-second)))
                (core module $main
                    (import "acme:first/api" "read" (func $first (result i32)))
                    (import "acme:second/api" "read" (func $second (result {second_core})))
                    (func (export "sum") (result i32) call $first call $second drop)
                )
                (core instance $main-instance (instantiate $main
                    (with "acme:first/api" (instance $first-args))
                    (with "acme:second/api" (instance $second-args))
                ))
                (func $sum (result u32) (canon lift (core func $main-instance "sum")))
                (component $exports
                    (import "import-func-sum" (func $f (result u32)))
                    (export "sum" (external-id "acme::app::app::sum") (func $f))
                )
                (instance $app (instantiate $exports (with "import-func-sum" (func $sum))))
                (export "acme:app/app" (instance $app))
            )
            "#
        ))
        .expect("component WAT should compile")
    }

    #[test]
    fn imports_of_one_path_share_a_declaration() {
        let context = Rc::default();
        let output =
            translate_with(&context, &two_imports_of_one_path_component("u32", "i32"), None)
                .expect("component should translate");
        let hir = core_modules_of(&output);
        assert_eq!(
            hir.matches("hir.call ::@acme::@shared::@api::@read()").count(),
            2,
            "both import stubs call the one declaration:\n{hir}"
        );
    }

    #[test]
    fn imports_of_one_path_with_different_signatures_are_rejected() {
        let err = error_of(&two_imports_of_one_path_component("u64", "i64"), None);
        assert!(
            err.contains(
                "imports `::acme:first/api::read` and `::acme:second/api::read` both lower to \
                 `::acme::shared::api::read` with different signatures"
            ),
            "unexpected diagnostic: {err}"
        );
    }

    /// A component whose core module imports `read` with the external-id `read_id` and exports
    /// `export_field`, lifted as `acme::app::app::<export_leaf>`.
    fn import_and_export_component(
        read_id: &str,
        export_field: &str,
        export_leaf: &str,
    ) -> Vec<u8> {
        wat::parse_str(format!(
            r#"
            (component
                (import "acme:first/api" (instance $first
                    (export "read" (external-id "{read_id}") (func (result u32)))
                ))
                (alias export $first "read" (func $first-read))
                (core func $lowered (canon lower (func $first-read)))
                (core instance $args (export "read" (func $lowered)))
                (core module $main
                    (import "acme:first/api" "read" (func $import (result i32)))
                    (func (export "sum") (result i32) call $import)
                    (export "read-export" (func $import))
                )
                (core instance $main-instance
                    (instantiate $main (with "acme:first/api" (instance $args))))
                (func $lifted (result u32) (canon lift (core func $main-instance "{export_field}")))
                (component $exports
                    (import "import-func" (func $f (result u32)))
                    (export "lifted" (external-id "acme::app::app::{export_leaf}") (func $f))
                )
                (instance $app (instantiate $exports (with "import-func" (func $lifted))))
                (export "acme:app/app" (instance $app))
            )
            "#
        ))
        .expect("component WAT should compile")
    }

    /// An import anywhere under the component's namespace is rejected, including under the path
    /// of its own core module `main`.
    #[test]
    fn an_import_inside_the_components_own_namespace_is_rejected() {
        for read_id in [
            "acme::app::app::read",
            "acme::app::app::main::read",
            "acme::app::app::sub::read",
        ] {
            let wasm = import_and_export_component(read_id, "sum", "sum");
            let err = error_of(&wasm, None);
            assert!(
                err.contains(&format!(
                    "import `::{read_id}` lies inside this component's own namespace \
                     `::acme::app::app`"
                )),
                "unexpected diagnostic for `{read_id}`: {err}"
            );
        }
    }

    #[test]
    fn a_core_export_of_an_imported_function_can_be_lifted() {
        let wasm = import_and_export_component("acme::first::api::read", "read-export", "read");
        let context = Rc::default();
        let output = translate_with(&context, &wasm, None).expect("component should translate");
        assert!(
            output.component.borrow().get(SymbolName::intern("read")).is_some(),
            "the re-exported import is lifted"
        );
    }
}
