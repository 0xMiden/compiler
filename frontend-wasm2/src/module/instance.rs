use midenc_hir::{ComponentImport, FunctionIdent};

/// Represents module argument that is used to instantiate a module.
#[derive(Debug, Clone)]
pub enum ModuleArgument {
    /// Represents function that is exported from another module.
    Function(FunctionIdent),
    /// Represents component import that is lowered to a module import.
    ComponentImport(ComponentImport),
    /// Represents table exported from another module.
    Table,
}
