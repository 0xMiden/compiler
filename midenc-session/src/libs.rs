#![deny(warnings)]

use alloc::{borrow::Cow, format, str::FromStr, sync::Arc, vec::Vec};
#[cfg(feature = "std")]
use alloc::{boxed::Box, string::ToString};
use core::fmt;

pub use miden_assembly::{
    Library as CompiledLibrary, LibraryNamespace, LibraryPath, LibraryPathComponent,
};
use miden_base_sys::masl::tx::MidenTxKernelLibrary;
#[cfg(feature = "std")]
use miden_core::utils::Deserializable;
use miden_stdlib::StdLibrary;
use midenc_hir_symbol::sync::LazyLock;

use crate::{diagnostics::Report, PathBuf, Session, TargetEnv};
#[cfg(feature = "std")]
use crate::{
    diagnostics::{IntoDiagnostic, WrapErr},
    Path,
};

pub static STDLIB: LazyLock<Arc<CompiledLibrary>> =
    LazyLock::new(|| Arc::new(StdLibrary::default().into()));
pub static BASE: LazyLock<Arc<CompiledLibrary>> =
    LazyLock::new(|| Arc::new(MidenTxKernelLibrary::default().into()));

/// The types of libraries that can be linked against during compilation
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    /// A compiled MAST library
    #[default]
    Mast,
    /// A source-form MASM library, using the standard project layout
    Masm,
    // A Miden package (MASP)
    Masp,
}
impl fmt::Display for LibraryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mast => f.write_str("mast"),
            Self::Masm => f.write_str("masm"),
            Self::Masp => f.write_str("masp"),
        }
    }
}
impl FromStr for LibraryKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mast" | "masl" => Ok(Self::Mast),
            "masm" => Ok(Self::Masm),
            "masp" => Ok(Self::Masp),
            _ => Err(()),
        }
    }
}

/// A library requested by the user to be linked against during compilation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkLibrary {
    /// The name of the library.
    ///
    /// If requested by name, e.g. `-l std`, the name is used as given.
    ///
    /// If requested by path, e.g. `-l ./target/libs/miden-base.masl`, then the name of the library
    /// will be the basename of the file specified in the path.
    pub name: Cow<'static, str>,
    /// If specified, the path from which this library should be loaded
    pub path: Option<PathBuf>,
    /// The kind of library to load.
    ///
    /// By default this is assumed to be a `.masl` library, but the kind will be detected based on
    /// how it is requested by the user. It may also be specified explicitly by the user.
    pub kind: LibraryKind,
}
impl LinkLibrary {
    /// Construct a LinkLibrary for Miden stdlib
    pub fn std() -> Self {
        LinkLibrary {
            name: "std".into(),
            path: None,
            kind: LibraryKind::Mast,
        }
    }

    /// Construct a LinkLibrary for Miden base(rollup/tx kernel) library
    pub fn base() -> Self {
        LinkLibrary {
            name: "base".into(),
            path: None,
            kind: LibraryKind::Mast,
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn load(&self, _session: &Session) -> Result<CompiledLibrary, Report> {
        // Handle libraries shipped with the compiler, or via Miden crates
        match self.name.as_ref() {
            "std" => Ok((*STDLIB).as_ref().clone()),
            "base" => Ok((*BASE).as_ref().clone()),
            name => Err(Report::msg(format!(
                "link library '{name}' cannot be loaded: compiler was built without standard \
                 library"
            ))),
        }
    }

    #[cfg(feature = "std")]
    pub fn load(&self, session: &Session) -> Result<CompiledLibrary, Report> {
        if let Some(path) = self.path.as_deref() {
            return self.load_from_path(path, session);
        }

        // Handle libraries shipped with the compiler, or via Miden crates
        match self.name.as_ref() {
            "std" => return Ok((*STDLIB).as_ref().clone()),
            "base" => return Ok((*BASE).as_ref().clone()),
            _ => (),
        }

        // Search for library among specified search paths
        let path = self.find(session)?;

        self.load_from_path(&path, session)
    }

    #[cfg(feature = "std")]
    fn load_from_path(&self, path: &Path, session: &Session) -> Result<CompiledLibrary, Report> {
        match self.kind {
            LibraryKind::Masm => {
                let ns = LibraryNamespace::new(&self.name)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("invalid library namespace '{}'", &self.name))?;
                let assembler = miden_assembly::Assembler::new(session.source_manager.clone())
                    .with_debug_mode(true);
                CompiledLibrary::from_dir(path, ns, assembler)
            }
            LibraryKind::Mast => CompiledLibrary::deserialize_from_file(path).map_err(|err| {
                Report::msg(format!(
                    "failed to deserialize library from '{}': {err}",
                    path.display()
                ))
            }),
            LibraryKind::Masp => {
                let bytes = std::fs::read(path).into_diagnostic()?;
                let package =
                    miden_mast_package::Package::read_from_bytes(&bytes).map_err(|e| {
                        Report::msg(format!(
                            "failed to load Miden package from {}: {e}",
                            path.display()
                        ))
                    })?;
                let lib = match package.mast {
                    miden_mast_package::MastArtifact::Executable(_) => {
                        return Err(Report::msg(format!(
                            "Expected Miden package to contain a Library, got Program: '{}'",
                            path.display()
                        )))
                    }
                    miden_mast_package::MastArtifact::Library(lib) => lib.clone(),
                };
                Ok((*lib).clone())
            }
        }
    }

    #[cfg(feature = "std")]
    fn find(&self, session: &Session) -> Result<PathBuf, Report> {
        use std::fs;

        for search_path in session.options.search_paths.iter() {
            let reader = fs::read_dir(search_path).map_err(|err| {
                Report::msg(format!(
                    "invalid library search path '{}': {err}",
                    search_path.display()
                ))
            })?;
            for entry in reader {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                if stem != self.name.as_ref() {
                    continue;
                }

                match self.kind {
                    LibraryKind::Mast => {
                        if !path.is_file() {
                            return Err(Report::msg(format!(
                                "unable to load MAST library from '{}': not a file",
                                path.display()
                            )));
                        }
                    }
                    LibraryKind::Masm => {
                        if !path.is_dir() {
                            return Err(Report::msg(format!(
                                "unable to load Miden Assembly library from '{}': not a directory",
                                path.display()
                            )));
                        }
                    }
                    LibraryKind::Masp => {
                        if !path.is_file() {
                            return Err(Report::msg(format!(
                                "unable to load Miden Assembly package from '{}': not a file",
                                path.display()
                            )));
                        }
                    }
                }
                return Ok(path);
            }
        }

        Err(Report::msg(format!(
            "unable to locate library '{}' using any of the provided search paths",
            &self.name
        )))
    }
}

#[cfg(feature = "std")]
impl clap::builder::ValueParserFactory for LinkLibrary {
    type Parser = LinkLibraryParser;

    fn value_parser() -> Self::Parser {
        LinkLibraryParser
    }
}

#[cfg(feature = "std")]
#[doc(hidden)]
#[derive(Clone)]
pub struct LinkLibraryParser;

#[cfg(feature = "std")]
impl clap::builder::TypedValueParser for LinkLibraryParser {
    type Value = LinkLibrary;

    fn possible_values(
        &self,
    ) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        use clap::builder::PossibleValue;

        Some(Box::new(
            [
                PossibleValue::new("masm").help("A Miden Assembly project directory"),
                PossibleValue::new("masl").help("A compiled MAST library file"),
            ]
            .into_iter(),
        ))
    }

    /// Parses the `-l` flag using the following format:
    ///
    /// `-l[KIND=]NAME`
    ///
    /// * `KIND` is one of: `masl`, `masm`; defaults to `masl`
    /// * `NAME` is either an absolute path, or a name (without extension)
    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;
        let (kind, name) = value
            .split_once('=')
            .map(|(kind, name)| (Some(kind), name))
            .unwrap_or((None, value));

        if name.is_empty() {
            return Err(Error::raw(
                ErrorKind::ValueValidation,
                "invalid link library: must specify a name or path",
            ));
        }

        let maybe_path = Path::new(name);
        let extension = maybe_path.extension().map(|ext| ext.to_str().unwrap());
        let kind = match kind {
            Some(kind) if !kind.is_empty() => kind.parse::<LibraryKind>().map_err(|_| {
                Error::raw(ErrorKind::InvalidValue, format!("'{kind}' is not a valid library kind"))
            })?,
            Some(_) | None => match extension {
                Some(kind) => kind.parse::<LibraryKind>().map_err(|_| {
                    Error::raw(
                        ErrorKind::InvalidValue,
                        format!("'{kind}' is not a valid library kind"),
                    )
                })?,
                None => LibraryKind::default(),
            },
        };

        if maybe_path.is_absolute() {
            let meta = maybe_path.metadata().map_err(|err| {
                Error::raw(
                    ErrorKind::ValueValidation,
                    format!(
                        "invalid link library: unable to load '{}': {err}",
                        maybe_path.display()
                    ),
                )
            })?;

            match kind {
                LibraryKind::Mast if !meta.is_file() => {
                    return Err(Error::raw(
                        ErrorKind::ValueValidation,
                        format!("invalid link library: '{}' is not a file", maybe_path.display()),
                    ));
                }
                LibraryKind::Masm if !meta.is_dir() => {
                    return Err(Error::raw(
                        ErrorKind::ValueValidation,
                        format!(
                            "invalid link library: kind 'masm' was specified, but '{}' is not a \
                             directory",
                            maybe_path.display()
                        ),
                    ));
                }
                _ => (),
            }

            let name = maybe_path.file_stem().unwrap().to_str().unwrap().to_string();

            Ok(LinkLibrary {
                name: name.into(),
                path: Some(maybe_path.to_path_buf()),
                kind,
            })
        } else if extension.is_some() {
            let name = name.strip_suffix(unsafe { extension.unwrap_unchecked() }).unwrap();
            let mut name = name.to_string();
            name.pop();

            Ok(LinkLibrary {
                name: name.into(),
                path: None,
                kind,
            })
        } else {
            Ok(LinkLibrary {
                name: name.to_string().into(),
                path: None,
                kind,
            })
        }
    }
}

/// Add libraries required by the target environment to the list of libraries to link against only
/// if they are not already present.
pub fn add_target_link_libraries(
    link_libraries_in: Vec<LinkLibrary>,
    target: &TargetEnv,
) -> Vec<LinkLibrary> {
    let mut link_libraries_out = link_libraries_in;
    match target {
        TargetEnv::Base | TargetEnv::Emu => {
            if !link_libraries_out.iter().any(|ll| ll.name == "std") {
                link_libraries_out.push(LinkLibrary::std());
            }
        }
        TargetEnv::Rollup { .. } => {
            if !link_libraries_out.iter().any(|ll| ll.name == "std") {
                link_libraries_out.push(LinkLibrary::std());
            }

            if !link_libraries_out.iter().any(|ll| ll.name == "base") {
                link_libraries_out.push(LinkLibrary::base());
            }
        }
    }
    link_libraries_out
}
