use core::fmt;

use midenc_hir::{
    formatter::{self, PrettyPrint},
    Abi, FunctionIdent, FunctionType,
};

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

/// Canonical ABI options associated with a lifted or lowered function.
#[derive(Debug, Clone)]
pub struct CanonicalOptions {
    /// The realloc function used by these options, if specified.
    pub realloc: Option<FunctionIdent>,
    /// The post-return function used by these options, if specified.
    pub post_return: Option<FunctionIdent>,
}

impl fmt::Display for CanonicalOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl formatter::PrettyPrint for CanonicalOptions {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        let mut doc = Document::Empty;
        if let Some(realloc) = self.realloc.as_ref() {
            doc += const_text("(realloc ") + display(realloc) + const_text(") ");
        }
        if let Some(post_return) = self.post_return.as_ref() {
            doc += const_text("(post-return ") + display(post_return) + const_text(") ");
        }
        doc
    }
}

/// A component import translated from a Wasm component import that is following
/// the Wasm Component Model Canonical ABI.
#[derive(Debug, Clone)]
pub struct CanonAbiImport {
    /// The interface function name that is being imported (Wasm CM level)
    pub interface_function: FunctionIdent,
    /// The interface function type (Wasm CM level)
    pub interface_function_ty: FunctionType,
    /// Any options associated with this import
    pub options: CanonicalOptions,
}

impl CanonAbiImport {
    pub fn new(
        interface_function: FunctionIdent,
        high_func_ty: FunctionType,
        options: CanonicalOptions,
    ) -> Self {
        assert_eq!(high_func_ty.abi, Abi::Wasm, "expected Abi::Wasm function type ABI");
        Self {
            interface_function,
            interface_function_ty: high_func_ty,
            options,
        }
    }
}

/// A Miden (sdklib, tx kernel) function import that is following the Miden ABI.
#[derive(Debug, Clone)]
pub struct MidenAbiImport {
    /// The Miden function type as it is defined in the MASM
    function_ty: FunctionType,
}

impl MidenAbiImport {
    pub fn new(function_ty: FunctionType) -> Self {
        assert_eq!(function_ty.abi, Abi::Canonical, "expected Abi::Canonical function type ABI");
        Self { function_ty }
    }
}

/// A component import
#[derive(Debug, Clone, derive_more::From)]
pub enum ComponentImport {
    /// A Wasm import that is following the Wasm Component Model Canonical ABI
    CanonAbiImport(CanonAbiImport),
    /// A Miden import that is following the Miden ABI
    MidenAbiImport(MidenAbiImport),
}

impl ComponentImport {
    pub fn unwrap_canon_abi_import(&self) -> &CanonAbiImport {
        match self {
            ComponentImport::CanonAbiImport(import) => import,
            _ => panic!("Expected CanonAbiImport"),
        }
    }
}

impl fmt::Display for ComponentImport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl formatter::PrettyPrint for ComponentImport {
    fn render(&self) -> formatter::Document {
        use formatter::*;
        let function_ty_str = match self {
            ComponentImport::CanonAbiImport(import) => import.interface_function_ty.to_string(),
            ComponentImport::MidenAbiImport(import) => import.function_ty.to_string(),
        };
        let name = match self {
            ComponentImport::CanonAbiImport(import) => {
                format!("{} ", import.interface_function)
            }
            ComponentImport::MidenAbiImport(_import) => "".to_string(),
        };

        const_text("(")
            + text(name)
            + const_text(" ")
            + const_text("(")
            + const_text("type")
            + const_text(" ")
            + text(function_ty_str)
            + const_text(")")
            + const_text(")")
    }
}
