use midenc_hir::{SymbolName, SymbolPath};

use crate::component::ComponentFunctionType;

/// Represents module argument that is used to instantiate a module.
#[derive(Debug, Clone)]
pub enum ModuleArgument {
    /// Represents function that is exported from another module.
    Function(SymbolPath),
    /// Represents component import (component level type signature) that is lowered to a module import.
    ComponentImport {
        /// The component-level type signature of the import.
        signature: ComponentFunctionType,
        /// The Miden path of the imported function, from its `external-id`.
        path: SymbolPath,
        /// The core-import path of the first import of the component that lowers to `path`.
        first_cm_path: SymbolPath,
        /// The name (the `::`-joined namespace) of the component being translated, which `path`
        /// must lie outside of.
        namespace: SymbolName,
    },
    /// Represents table exported from another module.
    Table,
}

/// Canonical ABI options associated with a lifted or lowered function.
#[derive(Debug, Clone)]
pub struct CanonicalOptions {
    /// The realloc function used by these options, if specified.
    pub realloc: Option<SymbolPath>,
    /// The post-return function used by these options, if specified.
    pub post_return: Option<SymbolPath>,
}
