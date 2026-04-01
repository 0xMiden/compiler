//! Debug info section builder for MASP packages.
//!
//! This module provides utilities for collecting debug information from the HIR
//! and building debug sections that can be serialized into the MASP package.

use alloc::{collections::BTreeMap, format, string::ToString, sync::Arc};

use miden_debug_types::{ColumnNumber, LineNumber};
use miden_mast_package::debug_info::{
    DebugFileInfo, DebugFunctionInfo, DebugFunctionsSection, DebugPrimitiveType,
    DebugSourcesSection, DebugTypeIdx, DebugTypeInfo, DebugTypesSection, DebugVariableInfo,
};
use midenc_dialect_debuginfo as debuginfo;
use midenc_hir::{DILocalVariable, DISubprogramAttr, OpExt, Type, dialects::builtin};

/// The output of the debug info collection pass: three separate sections.
pub struct DebugInfoSections {
    pub types: DebugTypesSection,
    pub sources: DebugSourcesSection,
    pub functions: DebugFunctionsSection,
}

/// Builder for constructing debug info sections from HIR components.
pub struct DebugInfoBuilder {
    types: DebugTypesSection,
    sources: DebugSourcesSection,
    functions: DebugFunctionsSection,
    /// Maps source file paths to their indices in the file table
    file_indices: BTreeMap<alloc::string::String, u32>,
    /// Maps type keys to their indices in the type table
    type_indices: BTreeMap<TypeKey, DebugTypeIdx>,
}

/// A key for deduplicating types (uses u32 since DebugTypeIdx lacks Ord)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TypeKey {
    Primitive(u8), // Use discriminant instead of the enum directly
    Pointer(u32),
    Array(u32, Option<u32>),
    Unknown,
}

impl Default for DebugInfoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugInfoBuilder {
    /// Creates a new debug info builder.
    pub fn new() -> Self {
        Self {
            types: DebugTypesSection::new(),
            sources: DebugSourcesSection::new(),
            functions: DebugFunctionsSection::new(),
            file_indices: BTreeMap::new(),
            type_indices: BTreeMap::new(),
        }
    }

    /// Adds a file to the file table and returns its index.
    ///
    /// The `directory` parameter, if provided, is joined with the path to create
    /// a full path. The debug info section stores full paths only.
    pub fn add_file(&mut self, path: &str, directory: Option<&str>) -> u32 {
        // Build the full path
        let full_path = if let Some(dir) = directory {
            if path.starts_with('/') || path.starts_with("\\\\") {
                // Already absolute
                path.to_string()
            } else {
                format!("{}/{}", dir.trim_end_matches('/'), path)
            }
        } else {
            path.to_string()
        };

        if let Some(&idx) = self.file_indices.get(&full_path) {
            return idx;
        }

        let path_idx = self.sources.add_string(Arc::from(full_path.as_str()));
        let file = DebugFileInfo::new(path_idx);

        let idx = self.sources.add_file(file);
        self.file_indices.insert(full_path, idx);
        idx
    }

    /// Adds a type to the type table and returns its index.
    pub fn add_type(&mut self, ty: &Type) -> DebugTypeIdx {
        let debug_type = hir_type_to_debug_type(ty, self);
        let key = type_to_key(&debug_type);

        if let Some(&idx) = self.type_indices.get(&key) {
            return idx;
        }

        let idx = self.types.add_type(debug_type);
        self.type_indices.insert(key, idx);
        idx
    }

    /// Adds a primitive type and returns its index.
    pub fn add_primitive_type(&mut self, prim: DebugPrimitiveType) -> DebugTypeIdx {
        let key = TypeKey::Primitive(prim as u8);
        if let Some(&idx) = self.type_indices.get(&key) {
            return idx;
        }

        let idx = self.types.add_type(DebugTypeInfo::Primitive(prim));
        self.type_indices.insert(key, idx);
        idx
    }

    /// Collects debug information from an HIR component.
    pub fn collect_from_component(&mut self, component: &builtin::Component) {
        // Traverse the component and collect debug info from all functions
        let region = component.body();
        let block = region.entry();

        for op in block.body() {
            if let Some(module) = op.downcast_ref::<builtin::Module>() {
                self.collect_from_module(module);
            } else if let Some(interface) = op.downcast_ref::<builtin::Interface>() {
                self.collect_from_interface(interface);
            } else if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.collect_from_function(function);
            }
        }
    }

    fn collect_from_module(&mut self, module: &builtin::Module) {
        let region = module.body();
        let block = region.entry();

        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.collect_from_function(function);
            }
        }
    }

    fn collect_from_interface(&mut self, interface: &builtin::Interface) {
        let region = interface.body();
        let block = region.entry();

        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.collect_from_function(function);
            }
        }
    }

    fn collect_from_function(&mut self, function: &builtin::Function) {
        // Try to get DISubprogram from the function's attributes
        let subprogram_attr =
            function.get_attribute(midenc_hir::interner::Symbol::intern("di.subprogram"));

        let subprogram = subprogram_attr.and_then(|attr| {
            let borrowed = attr.borrow();
            borrowed.downcast_ref::<DISubprogramAttr>().map(|sp| sp.as_value().clone())
        });

        let Some(subprogram) = subprogram else {
            // No debug info for this function, just collect from body
            self.collect_variables_from_function_body(function, None);
            return;
        };

        // Add file
        let file_idx = self.add_file(subprogram.file.as_str(), None);

        // Add function name
        let name_idx = self.functions.add_string(Arc::from(subprogram.name.as_str()));
        let linkage_name_idx = subprogram
            .linkage_name
            .map(|s| self.functions.add_string(Arc::from(s.as_str())));

        // Create function info
        let line = LineNumber::new(subprogram.line).unwrap_or_default();
        let column = ColumnNumber::new(subprogram.column.unwrap_or(1)).unwrap_or_default();

        let mut func_info = DebugFunctionInfo::new(name_idx, file_idx, line, column);
        if let Some(linkage_idx) = linkage_name_idx {
            func_info = func_info.with_linkage_name(linkage_idx);
        }

        // Collect local variables from function body
        self.collect_variables_from_function_body(function, Some(&mut func_info));

        self.functions.add_function(func_info);
    }

    fn collect_variables_from_function_body(
        &mut self,
        function: &builtin::Function,
        func_info: Option<&mut DebugFunctionInfo>,
    ) {
        // Walk through the function body to find DbgValue operations
        let entry = function.entry_block();
        let entry_block = entry.borrow();

        if let Some(func_info) = func_info {
            self.collect_variables_from_block(&entry_block, func_info);
        }
    }

    fn collect_variables_from_block(
        &mut self,
        block: &midenc_hir::Block,
        func_info: &mut DebugFunctionInfo,
    ) {
        for op in block.body() {
            // Check if this is a DbgValue operation
            if let Some(dbg_value) = op.downcast_ref::<debuginfo::DebugValue>()
                && let Some(var_info) = self.extract_variable_info(dbg_value.variable().as_value())
            {
                func_info.add_variable(var_info);
            }

            // Recursively process nested regions
            for region_idx in 0..op.num_regions() {
                let region = op.region(region_idx);
                let entry = region.entry();
                self.collect_variables_from_block(&entry, func_info);
            }
        }
    }

    fn extract_variable_info(&mut self, var: &DILocalVariable) -> Option<DebugVariableInfo> {
        let name_idx = self.functions.add_string(Arc::from(var.name.as_str()));

        // Add type if available
        let type_idx = if let Some(ref ty) = var.ty {
            self.add_type(ty)
        } else {
            self.add_primitive_type(DebugPrimitiveType::Felt) // Default to felt
        };

        let line = LineNumber::new(var.line).unwrap_or_default();
        let column = ColumnNumber::new(var.column.unwrap_or(1)).unwrap_or_default();

        let mut var_info = DebugVariableInfo::new(name_idx, type_idx, line, column);

        if let Some(arg_index) = var.arg_index {
            var_info = var_info.with_arg_index(arg_index);
        }

        Some(var_info)
    }

    /// Builds and returns the final debug info sections.
    pub fn build(self) -> DebugInfoSections {
        DebugInfoSections {
            types: self.types,
            sources: self.sources,
            functions: self.functions,
        }
    }

    /// Returns whether any debug info has been collected.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.types.is_empty() && self.sources.is_empty()
    }
}

/// Converts an HIR Type to a DebugTypeInfo.
fn hir_type_to_debug_type(ty: &Type, builder: &mut DebugInfoBuilder) -> DebugTypeInfo {
    match ty {
        Type::Unknown => DebugTypeInfo::Unknown,
        Type::Never => DebugTypeInfo::Primitive(DebugPrimitiveType::Void),
        Type::I1 => DebugTypeInfo::Primitive(DebugPrimitiveType::Bool),
        Type::I8 => DebugTypeInfo::Primitive(DebugPrimitiveType::I8),
        Type::U8 => DebugTypeInfo::Primitive(DebugPrimitiveType::U8),
        Type::I16 => DebugTypeInfo::Primitive(DebugPrimitiveType::I16),
        Type::U16 => DebugTypeInfo::Primitive(DebugPrimitiveType::U16),
        Type::I32 => DebugTypeInfo::Primitive(DebugPrimitiveType::I32),
        Type::U32 => DebugTypeInfo::Primitive(DebugPrimitiveType::U32),
        Type::I64 => DebugTypeInfo::Primitive(DebugPrimitiveType::I64),
        Type::U64 => DebugTypeInfo::Primitive(DebugPrimitiveType::U64),
        Type::I128 => DebugTypeInfo::Primitive(DebugPrimitiveType::I128),
        Type::U128 => DebugTypeInfo::Primitive(DebugPrimitiveType::U128),
        Type::U256 => DebugTypeInfo::Unknown, // No direct mapping for U256
        Type::F64 => DebugTypeInfo::Primitive(DebugPrimitiveType::F64),
        Type::Felt => DebugTypeInfo::Primitive(DebugPrimitiveType::Felt),
        Type::Ptr(ptr_type) => {
            let pointee_idx = builder.add_type(ptr_type.pointee());
            DebugTypeInfo::Pointer {
                pointee_type_idx: pointee_idx,
            }
        }
        Type::Array(array_type) => {
            let element_idx = builder.add_type(array_type.element_type());
            DebugTypeInfo::Array {
                element_type_idx: element_idx,
                count: Some(array_type.len() as u32),
            }
        }
        // For types we don't have direct mappings for, use Unknown
        Type::Struct(_) | Type::List(_) | Type::Function(_) | Type::Enum(_) => {
            DebugTypeInfo::Unknown
        }
    }
}

/// Creates a key for type deduplication.
fn type_to_key(ty: &DebugTypeInfo) -> TypeKey {
    match ty {
        DebugTypeInfo::Primitive(p) => TypeKey::Primitive(*p as u8),
        DebugTypeInfo::Pointer { pointee_type_idx } => TypeKey::Pointer(pointee_type_idx.as_u32()),
        DebugTypeInfo::Array {
            element_type_idx,
            count,
        } => TypeKey::Array(element_type_idx.as_u32(), *count),
        DebugTypeInfo::Unknown => TypeKey::Unknown,
        // For complex types like structs and functions, we don't deduplicate
        _ => TypeKey::Unknown,
    }
}

/// Builds debug info sections from an HIR component if debug info is enabled.
pub fn build_debug_info_sections(
    component: &builtin::Component,
    emit_debug_decorators: bool,
) -> Option<DebugInfoSections> {
    if !emit_debug_decorators {
        return None;
    }

    let mut builder = DebugInfoBuilder::new();
    builder.collect_from_component(component);

    if builder.is_empty() {
        None
    } else {
        Some(builder.build())
    }
}
