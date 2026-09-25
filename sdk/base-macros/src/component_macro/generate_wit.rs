use std::collections::{BTreeSet, HashSet};

use proc_macro::Span;
use semver::Version;
use syn::spanned::Spanned;

use crate::{
    component_macro::{ComponentMethod, MethodReturn, export_path, to_kebab_case},
    namespace::ComponentNamespace,
    types::{ExportedTypeDef, ExportedTypeKind, ensure_custom_type_defined},
    wit_builder::WitBuilder,
    wit_names::explicit_wit_identifier,
};

/// Inputs used to render the WIT interface and world for a component implementation.
pub(super) struct ComponentWitSpec<'a> {
    /// Component namespace naming the WIT package, interface, and every exported function.
    pub(super) namespace: &'a ComponentNamespace,
    /// Component package version.
    pub(super) component_version: &'a Version,
    /// Fully-qualified interfaces imported by the component world.
    pub(super) dependency_imports: &'a [String],
    /// Core type names imported by the exported interface.
    pub(super) type_imports: &'a BTreeSet<String>,
    /// Public component methods exported in the interface.
    pub(super) methods: &'a [ComponentMethod],
    /// Custom types exported alongside the methods.
    pub(super) exported_types: &'a [ExportedTypeDef],
}

/// Renders the WIT source describing the component interface exported by the `impl` block.
pub(super) fn build_component_wit(spec: ComponentWitSpec<'_>) -> Result<String, syn::Error> {
    let exported_type_names: HashSet<String> =
        spec.exported_types.iter().map(|def| def.wit_name.clone()).collect();

    let mut combined_core_imports = spec.type_imports.clone();
    for exported in spec.exported_types {
        match &exported.kind {
            ExportedTypeKind::Record { fields } => {
                for field in fields {
                    ensure_custom_type_defined(
                        &field.ty,
                        &exported_type_names,
                        Span::call_site().into(),
                    )?;
                    field.ty.add_required_core_type_imports(&mut combined_core_imports);
                }
            }
            ExportedTypeKind::Variant { variants } => {
                for variant in variants {
                    if let Some(payload) = &variant.payload {
                        ensure_custom_type_defined(
                            payload,
                            &exported_type_names,
                            Span::call_site().into(),
                        )?;
                        payload.add_required_core_type_imports(&mut combined_core_imports);
                    }
                }
            }
        }
    }

    let (wit, result) = WitBuilder::exported_interface(
        "#[component]",
        spec.namespace,
        spec.component_version,
        spec.dependency_imports,
        |interface| {
            if !combined_core_imports.is_empty() {
                let imports = combined_core_imports.iter().cloned().collect::<Vec<_>>().join(", ");
                interface.line(&format!("use core-types.{{{imports}}};"));
                interface.blank_line();
            }

            for (index, exported) in spec.exported_types.iter().enumerate() {
                if index > 0 {
                    interface.blank_line();
                }

                match &exported.kind {
                    ExportedTypeKind::Record { fields } => {
                        interface.block(&format!("record {} {{", exported.wit_name), |record| {
                            for field in fields {
                                let field_name = to_kebab_case(&field.name);
                                record.line(&format!("{field_name}: {},", field.ty.wit_name));
                            }
                        });
                    }
                    ExportedTypeKind::Variant { variants } => {
                        interface.block(
                            &format!("variant {} {{", exported.wit_name),
                            |variant_block| {
                                for variant in variants {
                                    if let Some(payload) = &variant.payload {
                                        variant_block.line(&format!(
                                            "{}({}),",
                                            variant.wit_name, payload.wit_name
                                        ));
                                    } else {
                                        variant_block.line(&format!("{},", variant.wit_name));
                                    }
                                }
                            },
                        );
                    }
                }
            }

            if !spec.exported_types.is_empty() && !spec.methods.is_empty() {
                interface.blank_line();
            }

            for method in spec.methods {
                let signature = component_method_signature(method, &exported_type_names)?;
                interface.function(&export_path(spec.namespace, &method.fn_ident), &signature);
            }

            Ok::<(), syn::Error>(())
        },
    );
    result?;
    Ok(wit)
}

/// Renders the WIT function signature for a component method.
fn component_method_signature(
    method: &ComponentMethod,
    exported_type_names: &HashSet<String>,
) -> Result<String, syn::Error> {
    for param in &method.params {
        ensure_custom_type_defined(&param.type_ref, exported_type_names, param.user_ty.span())?;
    }
    if let MethodReturn::Type { type_ref, user_ty } = &method.return_info {
        ensure_custom_type_defined(type_ref, exported_type_names, user_ty.span())?;
    }

    let wit_name = explicit_wit_identifier(&method.wit_name);
    let params = method
        .params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                explicit_wit_identifier(&param.wit_param_name),
                param.type_ref.wit_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let signature = match &method.return_info {
        MethodReturn::Unit => format!("{wit_name}: func({params});"),
        MethodReturn::Type { type_ref, .. } => {
            format!("{wit_name}: func({params}) -> {};", type_ref.wit_name)
        }
    };

    Ok(signature)
}
