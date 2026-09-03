use crate::module::{
    module_env::ParsedModule,
    types::{EntityIndex, FuncIndex, ModuleTypesBuilder, WasmRefType},
};

/// A core Wasm import consumed while folding a startup adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupAdapterImport {
    pub(super) module: String,
    pub(super) field: String,
}

/// The shim-table fixup combined with a startup adapter by current `wit-component` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupAdapterFixup {
    pub(super) table: StartupAdapterImport,
    pub(super) functions: Vec<StartupAdapterImport>,
}

/// The import resolved and invoked by a supported core Wasm startup adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupAdapter {
    pub(super) start: StartupAdapterImport,
    pub(super) fixup: Option<StartupAdapterFixup>,
}

/// Recognize the exact startup adapter forms emitted by the current component linker.
///
/// Modules which differ from this shape are left to normal translation. In particular, this does
/// not attempt to provide general support for core module start functions or instance-valued
/// instantiation arguments.
pub(super) fn classify_startup_adapter(
    parsed: &ParsedModule<'_>,
    module_types: &ModuleTypesBuilder,
) -> Option<StartupAdapter> {
    let module = &parsed.module;

    if !parsed.function_body_inputs.is_empty()
        || !module.exports.is_empty()
        || !module.passive_elements.is_empty()
        || !module.passive_elements_map.is_empty()
        || parsed.has_declared_elements
        || !module.memories.is_empty()
        || !module.globals.is_empty()
        || !module.global_initializers.is_empty()
        || !parsed.data_segments.is_empty()
        || !module.passive_data_map.is_empty()
    {
        return None;
    }

    let start_func = module.start_func?;
    if !module.is_imported_function(start_func) {
        return None;
    }
    let start_import = module
        .imports
        .iter()
        .find(|import| import.index == EntityIndex::Function(start_func))?;

    let signature = &module_types[module.functions[start_func].signature];
    if !signature.params().is_empty() || !signature.returns().is_empty() {
        return None;
    }

    let start = StartupAdapterImport {
        module: start_import.module.clone(),
        field: start_import.field.clone(),
    };

    if module.imports.len() == 1
        && module.num_imported_funcs == 1
        && module.functions.len() == 1
        && module.num_imported_tables == 0
        && module.tables.is_empty()
        && module.table_initialization.initial_values.is_empty()
        && module.table_initialization.segments.is_empty()
    {
        return Some(StartupAdapter { start, fixup: None });
    }

    classify_combined_startup_fixup(parsed, start_func, start)
}

/// Recognize the exact composition of a startup adapter and a bypassed shim-table fixup.
fn classify_combined_startup_fixup(
    parsed: &ParsedModule<'_>,
    start_func: FuncIndex,
    start: StartupAdapterImport,
) -> Option<StartupAdapter> {
    let module = &parsed.module;
    if module.num_imported_funcs < 2
        || module.functions.len() != module.num_imported_funcs
        || module.num_imported_tables != 1
        || module.tables.len() != 1
        || module.imports.len() != module.num_imported_funcs + 1
        || !module.table_initialization.initial_values.is_empty()
        || module.table_initialization.segments.len() != 1
    {
        return None;
    }

    let mut table_import = None;
    let mut function_imports = Vec::with_capacity(module.num_imported_funcs - 1);
    let mut function_indices = Vec::with_capacity(module.num_imported_funcs - 1);
    for import in &module.imports {
        match import.index {
            EntityIndex::Function(index) if index == start_func => {}
            EntityIndex::Function(index) => {
                function_indices.push(index);
                function_imports.push(StartupAdapterImport {
                    module: import.module.clone(),
                    field: import.field.clone(),
                });
            }
            EntityIndex::Table(index)
                if table_import.is_none()
                    && module.is_imported_table(index)
                    && import.field == "$imports" =>
            {
                table_import = Some((
                    index,
                    StartupAdapterImport {
                        module: import.module.clone(),
                        field: import.field.clone(),
                    },
                ));
            }
            _ => return None,
        }
    }

    let (table_index, table) = table_import?;
    let table_type = &module.tables[table_index];
    let element_count = u32::try_from(function_indices.len()).ok()?;
    if table_type.wasm_ty != WasmRefType::FUNCREF
        || table_type.minimum != element_count
        || table_type.maximum != Some(element_count)
    {
        return None;
    }
    let segment = &module.table_initialization.segments[0];
    if segment.table_index != table_index
        || segment.base.is_some()
        || segment.offset != 0
        || segment.elements.as_ref() != function_indices.as_slice()
    {
        return None;
    }

    Some(StartupAdapter {
        start,
        fixup: Some(StartupAdapterFixup {
            table,
            functions: function_imports,
        }),
    })
}

#[cfg(test)]
mod tests {
    use midenc_hir::Context;
    use wasmparser::Validator;

    use super::*;
    use crate::{
        WasmTranslationConfig,
        component::{ComponentParser, ComponentTypesBuilder},
        supported_component_model_features,
    };

    fn classify(wat: &str) -> Option<StartupAdapter> {
        let wasm = wat::parse_str(wat).expect("component WAT should compile");
        let context = Context::default();
        let config = WasmTranslationConfig::default();
        let mut validator = Validator::new_with_features(supported_component_model_features());
        let mut types = ComponentTypesBuilder::default();
        let parser = ComponentParser::new(&config, context.session(), &mut validator, &mut types);
        let parsed = parser.parse(&wasm).expect("component should parse");
        let module = parsed
            .static_modules
            .values()
            .next()
            .expect("fixture should contain one core module");

        classify_startup_adapter(module, types.module_types_builder())
    }

    #[test]
    fn recognizes_name_agnostic_state_free_adapter() {
        let adapter = classify(
            r#"
            (component
                (core module $adapter
                    (@custom "ignored" "metadata")
                    (type $start-type (func))
                    (import "not-main" "not-initialize" (func $start (type $start-type)))
                    (start $start)
                )
            )
            "#,
        )
        .expect("state-free imported-start module should be recognized");

        assert_eq!(adapter.start.module, "not-main");
        assert_eq!(adapter.start.field, "not-initialize");
        assert!(adapter.fixup.is_none());
    }

    #[test]
    fn recognizes_combined_startup_and_shim_fixup() {
        let adapter = classify(
            r#"
            (component
                (core module $adapter
                    (type $actual-type (func (param i32)))
                    (type $start-type (func))
                    (import "actual" "0" (func $actual-zero (type $actual-type)))
                    (import "target" "start" (func $start (type $start-type)))
                    (import "actual" "1" (func $actual-one (type $actual-type)))
                    (import "shim" "$imports" (table 2 2 funcref))
                    (start $start)
                    (elem (i32.const 0) func $actual-zero $actual-one)
                )
            )
            "#,
        )
        .expect("combined startup/fixup module should be recognized");

        assert_eq!(adapter.start.module, "target");
        assert_eq!(adapter.start.field, "start");
        let fixup = adapter.fixup.expect("combined adapter should describe its fixup");
        assert_eq!(fixup.table.module, "shim");
        assert_eq!(fixup.table.field, "$imports");
        assert_eq!(
            fixup.functions,
            vec![
                StartupAdapterImport {
                    module: "actual".to_owned(),
                    field: "0".to_owned(),
                },
                StartupAdapterImport {
                    module: "actual".to_owned(),
                    field: "1".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn rejects_malformed_combined_startup_fixups() {
        let malformed = [
            // The imported table must use the established shim export field.
            r#"
            (component (core module
                (import "actual" "0" (func $actual))
                (import "target" "start" (func $start))
                (import "shim" "not-imports" (table 1 1 funcref))
                (start $start)
                (elem (i32.const 0) func $actual)
            ))
            "#,
            // The element segment must begin at zero.
            r#"
            (component (core module
                (import "actual" "0" (func $actual))
                (import "target" "start" (func $start))
                (import "shim" "$imports" (table 2 2 funcref))
                (start $start)
                (elem (i32.const 1) func $actual)
            ))
            "#,
            // The element entries must match all non-start functions in import order.
            r#"
            (component (core module
                (import "actual" "0" (func $zero))
                (import "target" "start" (func $start))
                (import "actual" "1" (func $one))
                (import "shim" "$imports" (table 2 2 funcref))
                (start $start)
                (elem (i32.const 0) func $one $zero)
            ))
            "#,
            r#"
            (component (core module
                (import "actual" "0" (func $zero))
                (import "target" "start" (func $start))
                (import "actual" "1" (func $one))
                (import "shim" "$imports" (table 2 2 funcref))
                (start $start)
                (elem (i32.const 0) func $zero)
            ))
            "#,
            // Multiple element segments are not the generated fixup form.
            r#"
            (component (core module
                (import "actual" "0" (func $actual))
                (import "target" "start" (func $start))
                (import "shim" "$imports" (table 2 2 funcref))
                (start $start)
                (elem (i32.const 0) func $actual)
                (elem (i32.const 1) func $actual)
            ))
            "#,
            // Locally defined functions are never folded with the generated module.
            r#"
            (component (core module
                (import "actual" "0" (func $actual))
                (import "target" "start" (func $start))
                (import "shim" "$imports" (table 1 1 funcref))
                (func $local)
                (start $start)
                (elem (i32.const 0) func $actual)
            ))
            "#,
        ];

        for module in malformed {
            assert!(classify(module).is_none(), "malformed fixup must not be folded:\n{module}");
        }
    }

    #[test]
    fn rejects_adapter_lookalikes_with_observable_structure() {
        let lookalikes = [
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (export "start" (func $start))
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (func)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (memory 1)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (memory 1)
                (data (i32.const 0) "observable")
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (table 1 funcref)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (global i32 (i32.const 0))
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (elem func $start)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (elem declare func $start)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func $start))
                (import "m" "other" (func))
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "state" (global i32))
                (func $start)
                (start $start)
            ))
            "#,
            r#"
            (component (core module
                (import "m" "start" (func))
            ))
            "#,
        ];

        for lookalike in lookalikes {
            assert!(classify(lookalike).is_none(), "lookalike must not be folded:\n{lookalike}");
        }
    }

    #[test]
    fn core_start_validation_rejects_non_void_signature() {
        let wasm = wat::parse_str(
            r#"
            (component (core module
                (func $start (param i32))
                (start $start)
            ))
            "#,
        )
        .expect("component WAT should encode before validation");
        let context = Context::default();
        let config = WasmTranslationConfig::default();
        let mut validator = Validator::new_with_features(supported_component_model_features());
        let mut types = ComponentTypesBuilder::default();
        let parser = ComponentParser::new(&config, context.session(), &mut validator, &mut types);
        let err = match parser.parse(&wasm) {
            Ok(_) => panic!("a non-void core start signature must fail Wasm validation"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("start function") || err.to_string().contains("type mismatch"),
            "unexpected validator diagnostic: {err:?}"
        );
    }
}
