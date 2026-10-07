//! WIT interface generation for compiled Miden account-component packages.
//!
//! An account component written in MASM has no WIT of its own, yet `#[account(pkg::Iface)]` in
//! the Miden SDK links a component through its WIT interface. This crate writes that interface
//! from the package's manifest alone: one function per role procedure, each carrying its export
//! path in `@external-id`, with the manifest types expressed through the SDK's `core-types` where
//! they match exactly and declared locally otherwise. A procedure the component model cannot call
//! directly is left out and reported in [`Generated::skipped`], never failing the package.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod emit;
mod naming;
mod types;

use miden_mast_package::TargetType;
use midenc_hir_type::FunctionType;
use midenc_package_interface::{PackageInterface, ProcedureItem};

use self::{emit::Function, types::TypeSet};

/// The most operand stack elements the parameters of a direct cross-context call may occupy.
const MAX_PARAM_FELTS: usize = 16;

/// Where the SDK core types live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTypes {
    /// The WIT package that defines them, with its version: `miden:base@1.0.0`.
    pub package: String,
    /// The interface inside that package: `core-types`.
    pub interface: String,
}

impl Default for CoreTypes {
    fn default() -> Self {
        Self {
            package: "miden:base@1.0.0".to_owned(),
            interface: "core-types".to_owned(),
        }
    }
}

/// Generation options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Where the SDK core types live.
    pub core_types: CoreTypes,
}

/// A generated WIT document and the names in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The WIT text.
    pub wit: String,
    /// The WIT package id, e.g. `miden:standards-wallets-basic-wallet@0.17.0`.
    pub package_id: String,
    /// The interface name, e.g. `basic-wallet`.
    pub interface: String,
    /// The world name, e.g. `basic-wallet-world`.
    pub world: String,
    /// The role procedures left out, in package order.
    pub skipped: Vec<Skipped>,
}

/// A role procedure the interface leaves out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The export path, without a leading `::`.
    pub path: String,
    /// Why it was left out.
    pub reason: String,
}

/// Why a package has no WIT interface.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The package is not an account component.
    #[error("package is a {0}, not an account component")]
    NotAComponent(TargetType),
    /// The package has no role procedures to put in an interface.
    #[error("package exports no role procedures")]
    NoRoleProcedures,
    /// The role procedures do not share one module that can name the interface.
    #[error("{0}")]
    Namespace(String),
}

/// Generate the WIT interface of the account-component `package`.
pub fn generate(package: &PackageInterface, options: &Options) -> Result<Generated, Error> {
    if package.kind != TargetType::AccountComponent {
        return Err(Error::NotAComponent(package.kind));
    }
    let roles: Vec<&ProcedureItem> = package.roles().map(|(procedure, _)| procedure).collect();
    let (head, leaf) = namespace(&roles)?;

    let package_name: &str = package.name.as_ref();
    let package_id = naming::package_id(&head, package_name, &package.version);
    let interface = naming::kebab(&leaf);
    let world = format!("{interface}-world");

    let mut types = TypeSet::default();
    let mut functions: Vec<Function> = Vec::new();
    let mut skipped = Vec::new();
    for procedure in roles {
        let path = procedure.path.to_relative().to_string();
        let result = match &procedure.signature {
            None => Err("no typed signature".to_owned()),
            Some(signature) => function(procedure, &path, signature, &types, &functions),
        };
        match result {
            Ok((function, extended)) => {
                types = extended;
                functions.push(function);
            }
            Err(reason) => skipped.push(Skipped { path, reason }),
        }
    }

    let wit = emit::render(&emit::Document {
        package_name,
        package_version: package.version.to_string(),
        commitment: package.digest.to_string(),
        skipped: &skipped,
        package_id: &package_id,
        interface: &interface,
        world: &world,
        core_types: &options.core_types,
        types: &types,
        functions: &functions,
    });
    Ok(Generated {
        wit,
        package_id,
        interface,
        world,
        skipped,
    })
}

/// The head and the leaf segment of the one module all role procedures live in.
fn namespace(roles: &[&ProcedureItem]) -> Result<(String, String), Error> {
    let Some(first) = roles.first() else {
        return Err(Error::NoRoleProcedures);
    };
    let module = first.namespace();
    if let Some(other) = roles.iter().find(|p| p.namespace().to_relative() != module.to_relative())
    {
        return Err(Error::Namespace(format!(
            "role procedures live in more than one module: `{}` and `{}`",
            module.to_relative(),
            other.namespace().to_relative()
        )));
    }
    match (module.first(), module.last()) {
        (Some(head), Some(leaf)) => Ok((head.to_owned(), leaf.to_owned())),
        _ => Err(Error::Namespace(format!(
            "role procedure `{}` is not inside a module",
            first.path.to_relative()
        ))),
    }
}

/// The interface function for `procedure`, with the type set extended by what it needs; or the
/// reason it is left out, in which case `types` stays as it was.
fn function(
    procedure: &ProcedureItem,
    path: &str,
    signature: &FunctionType,
    types: &TypeSet,
    functions: &[Function],
) -> Result<(Function, TypeSet), String> {
    let mut types = types.clone();
    let name = naming::ident(procedure.name());

    let mut param_names = naming::ParamNames::default();
    let mut params = Vec::with_capacity(signature.params.len());
    let mut param_felts = 0;
    for (index, ty) in signature.params.iter().enumerate() {
        let mapped = types.map(ty)?;
        param_felts += mapped.felts;
        params.push((param_names.next(index, types::type_name(ty).as_deref()), mapped.wit));
    }
    let mut results = Vec::with_capacity(signature.results.len());
    for ty in &signature.results {
        results.push(types.map(ty)?);
    }

    if param_felts > MAX_PARAM_FELTS {
        return Err(format!(
            "parameters flatten to {param_felts} stack values, more than the {MAX_PARAM_FELTS} a \
             direct call can pass"
        ));
    }
    let result_values: usize = results.iter().map(|mapped| mapped.values).sum();
    let result = match results.as_slice() {
        [] => None,
        [single] if single.values == 1 => Some(single.wit.clone()),
        _ => {
            return Err(format!(
                "results flatten to {result_values} values; only a single-value result can cross \
                 a call"
            ));
        }
    };

    // Functions and types share one namespace in a WIT interface.
    let bare = name.trim_start_matches('%');
    if types.contains(bare) {
        return Err(format!("function `{name}` has the name of a type in the interface"));
    }
    if functions.iter().any(|f| f.name == name) {
        return Err(format!("another function is also named `{name}`"));
    }
    if let Some(function) =
        functions.iter().find(|f| types.contains(f.name.trim_start_matches('%')))
    {
        return Err(format!("a type would take the name of function `{}`", function.name));
    }

    Ok((
        Function {
            name,
            external_id: path.to_owned(),
            params,
            result,
        },
        types,
    ))
}

#[cfg(test)]
mod tests;
