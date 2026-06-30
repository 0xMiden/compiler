use midenc_hir::SymbolPath;

use crate::component::ComponentFunctionType;

/// Represents module argument that is used to instantiate a module.
#[derive(Debug, Clone)]
pub enum ModuleArgument {
    /// Represents function that is exported from another module.
    Function(SymbolPath),
    /// Represents component import (component level type signature) that is lowered to a module import.
    ComponentImport(ComponentFunctionType),
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
