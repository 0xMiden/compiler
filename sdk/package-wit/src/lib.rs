//! WIT interface generation for compiled Miden account-component packages.
//!
//! An account component written in MASM has no WIT of its own, yet `#[account(pkg::Iface)]` in
//! the Miden SDK links a component through its WIT interface. This crate writes that interface
//! from the package's manifest alone: one function per interface procedure, each carrying its
//! export path in `@external-id`, with the manifest types expressed through the SDK's `core-types`
//! where they match exactly or are named by their `core-types` id (as in a Rust-built component),
//! and declared locally otherwise. A procedure the interface cannot offer
//! (an auth procedure, no typed signature, outside the interface module, unsupported or clashing
//! names and types, parameters or results beyond the stack budget, 64-bit integer parameters or
//! results, reserved or invalid names) is left out and reported in [`Generated::skipped`] and in
//! the interface's doc comment. A procedure with several results returns them as one tuple. A package fails as a whole only when
//! it is not an account component ([`Error::NotAComponent`]), has no interface procedures
//! ([`Error::NoInterfaceProcedures`]), has no usable WIT package id or interface name
//! ([`Error::Namespace`]), or has every interface procedure left out
//! ([`Error::EverythingSkipped`]).
//!
//! The interface procedures are the exports marked `@account_procedure` or `@auth_script`: the
//! protocol counts only those as part of an account component's interface
//! (`AccountComponentCode::exports` in `miden-protocol`), so the kernel rejects a call to any
//! other export, `@note_script` and `@transaction_script` entrypoints included. Only the
//! `@account_procedure` ones become functions: an `@auth_script` procedure is invoked by the
//! transaction kernel in the epilogue, which rejects a transaction that called it before, so one
//! without `@account_procedure` is left out.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod emit;
mod naming;
mod types;

use std::collections::BTreeSet;

use miden_mast_package::TargetType;
use midenc_frontend_wasm_metadata::{
    DYNCALL_IMPORT_PREFIX, FPI_IMPORT_PREFIX, namespace::CORE_TYPES_PACKAGE,
    procedure_path::validate_procedure_path,
};
use midenc_hir_type::{FunctionType, StructRef, Type};
use midenc_package_interface::{PackageInterface, ProcedureItem, Role};

use self::{emit::Function, types::TypeSet};

/// The most operand stack elements the parameters or the results of a direct cross-context call
/// may occupy.
const MAX_STACK_ELEMENTS: usize = midenc_package_interface::abi::MAX_STACK_ELEMENTS;

/// Why an `@auth_script` procedure that is not also an `@account_procedure` is left out.
///
/// The kernel's epilogue rejects a transaction in which anything called the auth procedure before
/// the kernel itself (`epilogue.masm` in `miden-protocol`).
const AUTH_PROCEDURE_REASON: &str = "an auth procedure is invoked by the transaction kernel in \
                                     the epilogue, not by notes or scripts";

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
    /// The interface procedures left out, in path order.
    pub skipped: Vec<Skipped>,
}

/// An interface procedure the interface leaves out.
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
    /// The package has no interface procedures (`@account_procedure` or `@auth_script`).
    #[error("package exports no `@account_procedure` or `@auth_script` procedures")]
    NoInterfaceProcedures,
    /// The interface module (the module of the first account procedure) is not inside a module
    /// path that can name the WIT package and interface, or the package name or that module's
    /// path yields no WIT package id or interface name (an invalid name, a WIT or Rust keyword, or
    /// the reserved `core_types` leaf), or the package id is the Miden SDK's own WIT package even
    /// with the package name kept whole.
    #[error("{0}")]
    Namespace(String),
    /// Every interface procedure is left out; the procedures and why, in path order.
    #[error("every interface procedure is left out:{}", list_skipped(.0))]
    EverythingSkipped(Vec<Skipped>),
}

/// One `` `path`: reason`` line per procedure of `skipped`, each after a line break.
fn list_skipped(skipped: &[Skipped]) -> String {
    skipped.iter().map(|s| format!("\n  `{}`: {}", s.path, s.reason)).collect()
}

/// Generate the WIT interface of the account-component `package`.
pub fn generate(package: &PackageInterface) -> Result<Generated, Error> {
    if package.kind != TargetType::AccountComponent {
        return Err(Error::NotAComponent(package.kind));
    }
    // Checked on the attributes rather than the first-match `Role`: a procedure carrying both
    // `@note_script` and `@account_procedure` is still part of the interface. `interface_pass`
    // leaves out the `@auth_script` ones that are not also account procedures.
    let procedures: Vec<&ProcedureItem> = package
        .procedures
        .iter()
        .filter(|procedure| {
            [Role::AccountProcedure, Role::AuthScript]
                .iter()
                .any(|role| procedure.attributes.has(role.attribute()))
        })
        .collect();
    if procedures.is_empty() {
        return Err(Error::NoInterfaceProcedures);
    }
    // Only account procedures become functions, so only they decide the interface module and the
    // type names below: an auth procedure elsewhere, even one sorting first, changes neither.
    let Some(first) = procedures.iter().copied().find(|procedure| is_account_procedure(procedure))
    else {
        return Err(Error::EverythingSkipped(
            procedures
                .iter()
                .map(|procedure| Skipped {
                    path: procedure_path(procedure),
                    reason: AUTH_PROCEDURE_REASON.to_owned(),
                })
                .collect(),
        ));
    };
    let (head, leaf) = namespace(first)?;

    let package_name: &str = package.name.as_ref();
    let (id_namespace, id_name) =
        naming::package_id(&head, package_name).map_err(Error::Namespace)?;
    if format!("{id_namespace}:{id_name}") == CORE_TYPES_PACKAGE {
        return Err(Error::Namespace(format!(
            "the package id `{CORE_TYPES_PACKAGE}` is the Miden SDK's own WIT package"
        )));
    }
    let version = &package.version;
    let interface = naming::interface(&leaf).map_err(Error::Namespace)?;
    // Needs no escape: the `-world` suffix keeps it from being a keyword.
    let world = format!("{interface}-world");
    debug_assert!(naming::is_valid(&world));

    // Functions and types share one namespace in a WIT interface. On a clash the function is left
    // out, never a function that uses the type, so the type names are fixed before a function is
    // checked against them. The first pass checks against every name a function could add when
    // offered on its own: a superset of the types any pass keeps, since a function offered next
    // to others adds no other names. Each further pass checks against the types the previous
    // pass kept, and is accepted only while its own types stay within them, so no kept function
    // is shadowed by a type. The checked set shrinks strictly with every accepted pass, so the
    // passes end, in practice after one or two.
    let mut type_names: BTreeSet<String> = BTreeSet::new();
    for procedure in procedures.iter().filter(|procedure| is_account_procedure(procedure)) {
        if let Some(signature) = &procedure.signature
            && let Ok((_, types)) = function(
                procedure,
                &procedure_path(procedure),
                first,
                signature,
                &TypeSet::default(),
            )
        {
            type_names.extend(types.names().map(str::to_owned));
        }
    }
    let mut pass = interface_pass(&procedures, first, &type_names);
    loop {
        let kept_types: BTreeSet<String> = pass.types.names().map(str::to_owned).collect();
        if kept_types == type_names {
            break;
        }
        let next = interface_pass(&procedures, first, &kept_types);
        if !next.types.names().all(|name| kept_types.contains(name)) {
            break;
        }
        type_names = kept_types;
        pass = next;
    }
    let Pass {
        types,
        functions,
        skipped,
    } = pass;
    if functions.is_empty() {
        return Err(Error::EverythingSkipped(skipped));
    }

    let package_id = format!("{id_namespace}:{id_name}@{version}");
    let wit = emit::render(&emit::Document {
        package_name,
        package_version: package.version.to_string(),
        commitment: package.digest.to_string(),
        skipped: &skipped,
        package_id: &package_id,
        interface: &interface,
        world: &world,
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

/// The outcome of offering every interface procedure once: the functions kept, the types they
/// use, and the procedures left out with why.
struct Pass {
    /// The types the kept functions use or declare.
    types: TypeSet,
    /// The kept functions, in procedure order.
    functions: Vec<Function>,
    /// The procedures left out, in procedure order.
    skipped: Vec<Skipped>,
}

/// Offer `procedures` in order, leaving out each auth procedure that is not also an account
/// procedure, each one that has no function form in the interface module of the `first` account
/// procedure, whose function name another kept function already has, or whose function name is
/// one of `type_names`.
fn interface_pass(
    procedures: &[&ProcedureItem],
    first: &ProcedureItem,
    type_names: &BTreeSet<String>,
) -> Pass {
    // `TypeSet::map` may leave partial state behind on failure, so each function is mapped on a
    // clone (see `function`) that replaces the set only on success.
    let mut types = TypeSet::default();
    let mut functions: Vec<Function> = Vec::new();
    let mut skipped = Vec::new();
    for procedure in procedures {
        let path = procedure_path(procedure);
        let result = match &procedure.signature {
            _ if !is_account_procedure(procedure) => Err(AUTH_PROCEDURE_REASON.to_owned()),
            None => Err("no typed signature".to_owned()),
            Some(signature) => function(procedure, &path, first, signature, &types).and_then(
                |(function, extended)| {
                    if type_names.contains(function.name.trim_start_matches('%')) {
                        Err(format!(
                            "function `{}` has the name of a type in the interface",
                            function.name
                        ))
                    } else if functions.iter().any(|f| f.name == function.name) {
                        Err(format!("another function is also named `{}`", function.name))
                    } else {
                        Ok((function, extended))
                    }
                },
            ),
        };
        match result {
            Ok((function, extended)) => {
                types = extended;
                functions.push(function);
            }
            Err(reason) => skipped.push(Skipped { path, reason }),
        }
    }
    Pass {
        types,
        functions,
        skipped,
    }
}

/// The export path of `procedure`, without a leading `::`.
fn procedure_path(procedure: &ProcedureItem) -> String {
    procedure.path.to_relative().to_string()
}

/// Whether `procedure` is marked `@account_procedure`, so that it can become a function.
fn is_account_procedure(procedure: &ProcedureItem) -> bool {
    procedure.attributes.has(Role::AccountProcedure.attribute())
}

/// The head and leaf segments of the module of `first`, the first account procedure (in path
/// order), whose module is the interface module.
///
/// An interface procedure in another module is left out by [`function`].
fn namespace(first: &ProcedureItem) -> Result<(String, String), Error> {
    let module = first.namespace();
    match (module.first(), module.last()) {
        (Some(head), Some(leaf)) => Ok((head.to_owned(), leaf.to_owned())),
        _ => Err(Error::Namespace(format!(
            "interface procedure `{}` is not inside a module",
            first.path.to_relative()
        ))),
    }
}

/// The interface function for `procedure`, with the type set extended by what it needs; or the
/// reason it is left out, in which case `types` stays as it was.
///
/// The function's name is not checked against the other functions and the types;
/// `interface_pass` does that.
fn function(
    procedure: &ProcedureItem,
    path: &str,
    first: &ProcedureItem,
    signature: &FunctionType,
    types: &TypeSet,
) -> Result<(Function, TypeSet), String> {
    // The interface is named after one module, that of the `first` account procedure, so it
    // offers only that module's procedures.
    let module = first.namespace().to_relative();
    if procedure.namespace().to_relative() != module {
        return Err(format!(
            "lives in module `{}`, outside the interface module `{module}`",
            procedure.namespace().to_relative(),
        ));
    }
    // Every `@external-id` consumer applies this rule; it also keeps the `"` and `\` that
    // `@external-id("...")` cannot spell out of the path.
    if let Err(err) = validate_procedure_path(path) {
        return Err(format!("the export path {err}"));
    }
    let name =
        naming::rust_ident(procedure.name()).map_err(|err| format!("the procedure name {err}"))?;
    // The SDK rejects a dependency interface that has a function in a prefix its generated
    // imports use: the foreign procedure call imports and the stored-procedure dispatch imports.
    for (prefix, purpose) in [
        (FPI_IMPORT_PREFIX, "foreign procedure call imports"),
        (DYNCALL_IMPORT_PREFIX, "stored-procedure dispatch imports"),
    ] {
        if name.starts_with(prefix) {
            return Err(format!(
                "the function name `{name}` starts with `{prefix}`, which the SDK reserves for \
                 its {purpose}"
            ));
        }
    }
    let mut types = types.clone();

    let mut param_names = naming::ParamNames::default();
    let mut params = Vec::with_capacity(signature.params.len());
    let mut param_felts = 0;
    for (index, ty) in signature.params.iter().enumerate() {
        let mapped = types.map(ty)?;
        let param_name = param_names.next(index, types::type_name(ty).as_deref());
        // Only a 64-bit integer occupies more stack elements than it flattens to core values.
        // As for results, the stack convention of its two limbs in a call to a MASM callee has
        // not been validated yet: no binding exercises it.
        if mapped.felts > mapped.values {
            return Err(match wide_field(ty) {
                Some(field) => format!(
                    "parameter `{}` contains a 64-bit integer field `{field}`, which is not \
                     supported yet",
                    param_name.trim_start_matches('%')
                ),
                None => "parameters of 64-bit integer type are not supported yet".to_owned(),
            });
        }
        param_felts += mapped.felts;
        params.push((param_name, mapped.wit));
    }
    let mut results = Vec::with_capacity(signature.results.len());
    for ty in &signature.results {
        results.push(types.map(ty)?);
    }

    if param_felts > MAX_STACK_ELEMENTS {
        return Err(format!(
            "parameters flatten to {param_felts} stack elements, more than the \
             {MAX_STACK_ELEMENTS} a direct call can pass"
        ));
    }
    // Results are offered when they contain no 64-bit integer and occupy at most the stack
    // elements a call can return. The callee leaves its results on the stack with the first
    // flattened value on top, which is how the caller reads several values back; the stack
    // convention of a 64-bit integer's two limbs has not been validated yet.
    let result_felts: usize = results.iter().map(|mapped| mapped.felts).sum();
    if let Some((ty, _)) = signature
        .results
        .iter()
        .zip(&results)
        .find(|(_, mapped)| mapped.felts > mapped.values)
    {
        return Err(match wide_field(ty) {
            Some(field) => format!(
                "a result contains a 64-bit integer field `{field}`, which is not supported yet"
            ),
            None => "results of 64-bit integer type are not supported yet".to_owned(),
        });
    }
    if result_felts > MAX_STACK_ELEMENTS {
        return Err(format!(
            "results occupy {result_felts} stack elements, more than the {MAX_STACK_ELEMENTS} a \
             call can return"
        ));
    }
    // Several manifest results become one tuple, whose elements flatten in order, as the results
    // lie on the stack.
    let result = match results.as_slice() {
        [] => None,
        [single] => Some(single.wit.clone()),
        several => Some(format!(
            "tuple<{}>",
            several.iter().map(|mapped| mapped.wit.as_str()).collect::<Vec<_>>().join(", ")
        )),
    };

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

/// The dotted path of the first 64-bit integer field inside the struct `ty`, e.g. `inner.value`;
/// `None` when `ty` is not a struct or has no such field.
fn wide_field(ty: &Type) -> Option<String> {
    let Type::Struct(StructRef::Plain(st)) = ty else {
        return None;
    };
    st.fields().iter().find_map(|field| {
        let name = field.name.as_deref().unwrap_or("_");
        match &field.ty {
            Type::U64 | Type::I64 => Some(name.to_owned()),
            ty => wide_field(ty).map(|inner| format!("{name}.{inner}")),
        }
    })
}

#[cfg(test)]
mod tests;
