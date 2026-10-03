//! Generates Rust FFI bindings and linker stubs for Miden packages, from their manifests.
//!
//! The input is a [`PackageInterface`]: a package's exports, classified once by
//! `midenc-package-interface` under the same Miden ABI rule set the Wasm frontend applies, so a
//! binding and the call the frontend builds for it cannot disagree. The output is two Rust source
//! texts per package ([`Generated`]):
//!
//! - the *bindings*: a module tree mirroring the package's MASM modules under [`Options::root`],
//!   the exported types (`#[repr(C)]` structs, enums, aliases), the exported constants, and for
//!   every `exec`-bindable procedure an `extern "C"` declaration, resolved by name as a linker
//!   stub, plus a wrapper that spreads aggregates into scalars, reads multiple results back
//!   through a word-aligned return area, and passes element-space pointers as `ElementPtr`
//!   element addresses, unconverted;
//! - the *stubs*: the source of a `no_std` crate with a weak, diverging definition of every symbol
//!   the bindings declare, which the build compiles into the archive the linker resolves them
//!   against.
//!
//! Exports that cannot be bound produce no code; each is reported as a [`Skipped`] entry.
//!
//! Both texts are meant to be read by people and, for the SDK's own crates, committed: they are
//! written line by line rather than through a token-stream library, so a regenerated file diffs
//! cleanly against the committed one.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::BTreeMap;

use midenc_package_interface::PackageInterface;

mod items;
mod layout;
mod names;
mod procedures;
mod render;
mod stubs;
mod types;

/// Where the generated bindings of another package live, so the bindings being generated can
/// refer to its types instead of declaring their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    /// The MASM root that package's bindings were generated from, e.g. `::miden::protocol`.
    pub root: String,
    /// The Rust path those bindings are mounted at, e.g. `crate::raw::protocol`.
    pub rust_path: String,
}

/// How to generate the bindings of one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The MASM module path the generated text is mounted at, e.g. `::miden::core`.
    ///
    /// It is stripped from every export path: `::miden::core::mem::pipe_words_to_memory` becomes
    /// `mem::pipe_words_to_memory` relative to wherever the text is mounted. Every export of the
    /// package must lie under it.
    pub root: String,
    /// The absolute Rust path of the support module that provides `Felt`, `Word`, `WordAligned`,
    /// `ElementPtr`, `FeltConstant` and `WordConstant`, e.g. `::miden_intrinsics_sys::support`.
    pub support: String,
    /// The packages whose types the bindings may refer to, by package name.
    ///
    /// A signature that uses a type one of them exports refers to that package's generated type
    /// instead of declaring a copy. The interfaces themselves are passed to [`generate`] as its
    /// `externals`.
    pub with: BTreeMap<String, External>,
}

/// An export that produced no code, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The export's full MASM path.
    pub path: String,
    /// Why it produced no code.
    pub reason: String,
}

/// The generated text for one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The bindings: Rust source to mount at the module chosen for [`Options::root`].
    pub bindings: String,
    /// The stubs: the source of a `no_std` crate that defines every symbol the bindings declare.
    pub stubs: String,
    /// The exports that produced no code, with the reason for each.
    pub skipped: Vec<Skipped>,
}

/// Why a package's bindings could not be generated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An export lies outside [`Options::root`], so it has no place in the generated module tree.
    #[error("export `{path}` lies outside the generated root `{root}`")]
    OutsideRoot {
        /// The export's MASM path.
        path: String,
        /// The root it should lie under.
        root: String,
    },
    /// Two different types would be generated under one name in one module.
    #[error("module `{module}` has two different types named `{name}`")]
    ConflictingType {
        /// The MASM path of the module.
        module: String,
        /// The type name.
        name: String,
    },
    /// Two other items would be generated under one Rust name where they share a namespace: two
    /// modules of one module, a module and a type, two constants of one module, two stubs.
    #[error("`{first}` and `{second}` would both be named `{name}`")]
    ConflictingName {
        /// The MASM path of the first item.
        first: String,
        /// The MASM path of the second item.
        second: String,
        /// The Rust name both would have.
        name: String,
    },
    /// Something the bindings need has no Rust form.
    #[error("`{path}`: {reason}")]
    Unsupported {
        /// The MASM path where the problem was found.
        path: String,
        /// What has no Rust form, and why.
        reason: String,
    },
    /// Any other failure.
    #[error("{0}")]
    Message(String),
}

/// Generate the bindings and stubs of `package`.
///
/// `externals` are the interfaces of the packages [`Options::with`] names; a type one of them
/// exports is referred to rather than declared again. A struct or enum the bindings use that no
/// export they can refer to provides (an anonymous struct, or a type of a package without a
/// `with` entry) is declared in the module that uses it.
///
/// Every procedure the package's interface skips is reported in [`Generated::skipped`], with the
/// reason it was skipped, and so is every export that has no Rust form; none produces code:
///
/// - a type export with no Rust form (`u128`, an enum whose variants carry values or whose
///   discriminant is not an integer, a struct with no Rust layout equal to its HIR layout, a
///   struct holding any of these);
/// - a bindable procedure that uses a type with no Rust form (a pointer to a `u128`, such a
///   struct), whose results are zero-sized, whose Rust name another procedure or a constant of its
///   module has, or whose wrapper cannot name its parts (a type `Ret` in its module, two
///   flattened parameters of one name);
/// - a string constant.
///
/// Two items that would have one Rust name in one namespace are an error otherwise: two type
/// exports or declared types ([`Error::ConflictingType`]), two modules, a module and a type, two
/// constants or two stubs ([`Error::ConflictingName`]).
pub fn generate(
    package: &PackageInterface,
    externals: &[&PackageInterface],
    options: &Options,
) -> Result<Generated, Error> {
    let mut universe = types::TypeUniverse::new(package, externals, options);
    universe.declare_local_types(package)?;
    let mut skipped: Vec<Skipped> = package
        .skipped()
        .map(|(procedure, reason)| Skipped {
            path: procedure.path.to_string(),
            reason: reason.to_string(),
        })
        .collect();
    let bindings = items::bindings(package, &universe, options, &mut skipped)?;
    let stubs = stubs::stubs(package, &skipped)?;
    Ok(Generated {
        bindings,
        stubs,
        skipped,
    })
}
