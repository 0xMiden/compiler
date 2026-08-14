use std::collections::{BTreeSet, HashSet};

use proc_macro::Span;
use semver::Version;
use syn::spanned::Spanned;

use crate::{
    component_macro::{CORE_TYPES_PACKAGE, ComponentMethod, MethodReturn, to_kebab_case},
    types::{ExportedTypeDef, ExportedTypeKind, ensure_custom_type_defined},
    wit_builder::WitBuilder,
    wit_world::write_world_block,
};

/// Inputs used to render the WIT interface and world for a component implementation.
pub(super) struct ComponentWitSpec<'a> {
    /// Fully-qualified WIT package name for the component.
    pub(super) component_package: &'a str,
    /// Component package version.
    pub(super) component_version: &'a Version,
    /// Interface exported by the component world.
    pub(super) interface_name: &'a str,
    /// World generated for the component package.
    pub(super) world_name: &'a str,
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

    let mut wit = WitBuilder::new("#[component]", spec.component_package, spec.component_version);
    wit.use_path(CORE_TYPES_PACKAGE);
    wit.blank_line();
    wit.interface(spec.interface_name, |interface| {
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
            interface.line(&signature);
        }

        Ok::<(), syn::Error>(())
    })?;
    wit.blank_line();
    let exports = [spec.interface_name.to_string()];
    write_world_block(&mut wit, spec.world_name, spec.dependency_imports, &exports);

    Ok(wit.finish())
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

    let signature = if method.params.is_empty() {
        match &method.return_info {
            MethodReturn::Unit => format!("{}: func();", method.wit_name),
            MethodReturn::Type { type_ref, .. } => {
                format!("{}: func() -> {};", method.wit_name, type_ref.wit_name)
            }
        }
    } else {
        let params = method
            .params
            .iter()
            .map(|param| format!("{}: {}", param.wit_param_name, param.type_ref.wit_name))
            .collect::<Vec<_>>()
            .join(", ");
        match &method.return_info {
            MethodReturn::Unit => format!("{}: func({params});", method.wit_name),
            MethodReturn::Type { type_ref, .. } => {
                format!("{}: func({params}) -> {};", method.wit_name, type_ref.wit_name)
            }
        }
    };

    Ok(signature)
}
