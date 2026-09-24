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
use core::str::FromStr;

use midenc_hir::{FunctionIdent, FxHashMap, SymbolPath};
use midenc_session::diagnostics::{IntoDiagnostic, Report};
use wasmparser::{ComponentExternalKind, Encoding, Parser, Payload};

use crate::error::WasmResult;

/// Returns the Miden path carried by the `external_id` of the component function `cm_name` of the
/// component-model interface `cm_iface`.
///
/// Fails when the attribute is missing, or when its value is not an absolute Miden path with a
/// module and a function name (a leading `::` is accepted).
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
    let has_module_and_leaf =
        value.contains("::") && value.split("::").all(|segment| !segment.is_empty());
    let id =
        FunctionIdent::from_str(value)
            .ok()
            .filter(|_| has_module_and_leaf)
            .ok_or_else(|| {
                Report::msg(format!(
                    "the `@external-id` of function `{cm_name}` of interface `{cm_iface}` is \
                     `{external_id}`, which is not an absolute Miden path with a module and a \
                     function name, e.g. `miden::counter_contract::counter_contract::get_count`"
                ))
            })?;
    Ok(SymbolPath::from_masm_function_id(id))
}

/// A component function export, as seen by the namespace pre-scan.
pub(crate) struct ExportedFunction<'a> {
    /// The component-model interface the function is exported through (for diagnostics).
    pub interface: String,
    /// The component-model name of the function.
    pub name: &'a str,
    /// The value of its `external-id` attribute, if any.
    pub external_id: Option<&'a str>,
}

/// Returns the namespace shared by the Miden paths of `exports`, or `None` when there are none.
///
/// Fails when a function has no valid Miden path, when two functions share a path, or when the
/// functions are not all under the same namespace.
pub(crate) fn exports_namespace<'a>(
    exports: impl IntoIterator<Item = ExportedFunction<'a>>,
) -> WasmResult<Option<SymbolPath>> {
    let mut seen: FxHashMap<SymbolPath, &'a str> = FxHashMap::default();
    let mut namespaces: Vec<SymbolPath> = Vec::new();
    for export in exports {
        let path = external_id_path(&export.interface, export.name, export.external_id)?;
        if let Some(previous) = seen.insert(path.clone(), export.name) {
            return Err(Report::msg(format!(
                "component functions `{previous}` and `{}` share the Miden path `{path}`",
                export.name
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

/// Names the interface a nested component's exports go through, for diagnostics.
///
/// The root component exports the instances of its nested components under their interface
/// names; with a single one it is unambiguous, otherwise the nested component index is used.
pub(crate) fn interface_hint(root_instance_exports: &[&str], nested_component: u32) -> String {
    match root_instance_exports {
        [single] => single.to_string(),
        _ => format!("<nested component {nested_component}>"),
    }
}

/// Returns the namespace declared by the function exports of the Wasm component `wasm`, i.e. the
/// namespace the frontend roots the component at when the build does not provide one.
///
/// Returns `Ok(None)` for a core module or a component without function exports.
pub fn declared_namespace(wasm: &[u8]) -> WasmResult<Option<SymbolPath>> {
    if !Parser::is_component(wasm) {
        return Ok(None);
    }
    // The encodings of the (nested) modules and components being parsed, innermost last.
    let mut stack: Vec<Encoding> = Vec::new();
    // `(nested component ordinal, name, external-id)` of every nested component function export.
    let mut functions: Vec<(u32, &str, Option<&str>)> = Vec::new();
    let mut root_instance_exports: Vec<&str> = Vec::new();
    let mut nested_components = 0u32;
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.into_diagnostic()? {
            Payload::Version { encoding, .. } => {
                if encoding == Encoding::Component && !stack.is_empty() {
                    nested_components += 1;
                }
                stack.push(encoding);
            }
            Payload::End(_) => {
                stack.pop();
            }
            Payload::ComponentExportSection(reader) => {
                for export in reader {
                    let export = export.into_diagnostic()?;
                    match (stack.len(), export.kind) {
                        (1, ComponentExternalKind::Instance) => {
                            root_instance_exports.push(export.name.name)
                        }
                        (depth, ComponentExternalKind::Func) if depth > 1 => functions.push((
                            nested_components - 1,
                            export.name.name,
                            export.name.external_id,
                        )),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    exports_namespace(functions.into_iter().map(|(component, name, external_id)| {
        ExportedFunction {
            interface: interface_hint(&root_instance_exports, component),
            name,
            external_id,
        }
    }))
}

#[cfg(test)]
mod tests {
    use alloc::{format, rc::Rc, string::String};

    use midenc_hir::{Context, SymbolName, SymbolTable};

    use super::*;
    use crate::{WasmTranslationConfig, translate};

    /// A component lifting one core function and exporting it as `get-count` through the
    /// interface `miden:counter/counter@0.1.0`, with `attribute` on the nested export.
    fn counter_component(attribute: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"
            (component
                (core module $m
                    (func (export "get-count") (result i32) i32.const 0)
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

    fn translate_with(wasm: &[u8], namespace: Option<&str>) -> WasmResult<crate::FrontendOutput> {
        let config = WasmTranslationConfig {
            namespace: namespace.map(SymbolPath::from_masm_module_id),
            ..Default::default()
        };
        translate(wasm, &config, Rc::new(Context::default()))
    }

    fn error_of(result: WasmResult<crate::FrontendOutput>) -> String {
        match result {
            Ok(_) => panic!("translation should fail"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn an_export_is_named_by_its_external_id() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::get_count")"#);
        let output = translate_with(&wasm, None).expect("component should translate");
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
    fn an_export_without_external_id_is_rejected() {
        let err = error_of(translate_with(&counter_component(""), None));
        assert!(
            err.contains("`get-count`")
                && err.contains("`miden:counter/counter@0.1.0`")
                && err.contains("has no `@external-id`"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn an_external_id_without_a_module_is_rejected() {
        let err = error_of(translate_with(
            &counter_component(r#"(external-id "no-double-colon")"#),
            None,
        ));
        assert!(
            err.contains("`no-double-colon`") && err.contains("not an absolute Miden path"),
            "unexpected diagnostic: {err}"
        );
    }

    #[test]
    fn exports_outside_the_target_namespace_are_rejected() {
        let wasm = counter_component(r#"(external-id "miden::counter::counter::get_count")"#);
        let err = error_of(translate_with(&wasm, Some("miden::other::other")));
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
}
