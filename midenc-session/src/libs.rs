use alloc::{borrow::Cow, format, sync::Arc, vec::Vec};
#[cfg(feature = "std")]
use alloc::{boxed::Box, string::ToString};

pub use miden_assembly_syntax::{PathBuf as LibraryPath, PathComponent as LibraryPathComponent};
use miden_mast_package::Package;
use miden_project::Linkage;

use crate::{Options, PathBuf, diagnostics::Report};
#[cfg(feature = "std")]
use crate::{Path, diagnostics::IntoDiagnostic};

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
    /// How to link against this library
    pub linkage: Linkage,
}

impl LinkLibrary {
    pub fn is_core(&self) -> bool {
        matches!(self.name.as_ref(), "miden-core" | "core" | "std")
    }

    pub fn is_protocol(&self) -> bool {
        matches!(self.name.as_ref(), "miden-protocol" | "protocol" | "base")
    }

    /// Construct a LinkLibrary for Miden stdlib
    pub fn core() -> Self {
        LinkLibrary {
            name: "miden-core".into(),
            path: None,
            linkage: Linkage::Dynamic,
        }
    }

    /// Construct a LinkLibrary for the Miden transaction kernel library
    pub fn tx_kernel() -> Self {
        LinkLibrary {
            name: "miden-tx-kernel".into(),
            path: None,
            linkage: Linkage::Dynamic,
        }
    }

    /// Construct a LinkLibrary for Miden protocol library (userspace)
    pub fn protocol() -> Self {
        LinkLibrary {
            name: "miden-protocol".into(),
            path: None,
            linkage: Linkage::Dynamic,
        }
    }

    /// The file stem this library has in a toolchain's `lib/` directory.
    ///
    /// The short names `-l std`, `-l core`, `-l base`, `-l protocol` and `-l tx-kernel` are
    /// accepted for the toolchain libraries; everything else is used as given.
    pub fn canonical_name(&self) -> &str {
        match self.name.as_ref() {
            "std" | "core" | "miden-core" => "miden-core",
            "base" | "protocol" | "miden-protocol" => "miden-protocol",
            "tx-kernel" | "miden-tx-kernel" => "miden-tx-kernel",
            other => other,
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn load(&self, _options: &Options) -> Result<Arc<Package>, Report> {
        Err(Report::msg(format!(
            "link library '{}' cannot be loaded: the compiler was built without `std`, and \
             packages are only available from a toolchain on disk",
            self.name
        )))
    }

    #[cfg(feature = "std")]
    pub fn load(&self, options: &Options) -> Result<Arc<Package>, Report> {
        if let Some(path) = self.path.as_deref() {
            return self.load_from_path(path, options);
        }
        let path = self.find(options)?;
        self.load_from_path(&path, options)
    }

    #[cfg(feature = "std")]
    fn load_from_path(&self, path: &Path, _options: &Options) -> Result<Arc<Package>, Report> {
        let package = load_package_from_path(path)?;
        if package.is_program() {
            return Err(Report::msg(format!(
                "Expected Miden package to contain a Library, got Program: '{}'",
                path.display()
            )));
        }

        Ok(package)
    }

    #[cfg(feature = "std")]
    fn find(&self, options: &Options) -> Result<PathBuf, Report> {
        use std::fs;

        for search_path in options.search_paths.iter() {
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
                if stem != self.canonical_name() {
                    continue;
                }

                if !path.is_file() {
                    return Err(Report::msg(format!(
                        "unable to load Miden Assembly package from '{}': not a file",
                        path.display()
                    )));
                }
                return Ok(path);
            }
        }

        Err(Report::msg(format!(
            "unable to locate library '{}' (file stem '{}.masp'): searched {}; install a Miden \
             toolchain with midenup (see miden-toolchain.toml) so that MIDEN_SYSROOT/lib contains \
             it, or pass its directory with -L",
            self.name,
            self.canonical_name(),
            if options.search_paths.is_empty() {
                alloc::string::String::from(
                    "no search paths (MIDEN_SYSROOT is unset and no -L was given)",
                )
            } else {
                options
                    .search_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )))
    }
}

#[cfg(feature = "std")]
pub(crate) fn load_package_from_path(path: &Path) -> Result<Arc<Package>, Report> {
    let bytes = std::fs::read(path).into_diagnostic()?;
    miden_mast_package::Package::read_from_bytes_trusted(&bytes)
        .map_err(|e| {
            Report::msg(format!("failed to load Miden package from {}: {e}", path.display()))
        })
        .map(Arc::new)
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
            [PossibleValue::new("masp").help("A compiled Miden package")].into_iter(),
        ))
    }

    /// Parses the `-l` flag using the following format:
    ///
    /// `-l[KIND[:<LINKAGE>]=]NAME`
    ///
    /// * `KIND` is one of: `masp`; defaults to `masp`
    /// * `LINKAGE` is one of: `static`, `dynamic`; defaults to `dynamic`
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

        let linkage = match kind {
            Some(kind) => match kind.split_once(':') {
                Some(("masp", "static")) => Linkage::Static,
                Some(("masp", "dynamic")) => Linkage::Dynamic,
                Some(("masp", other)) => {
                    return Err(Error::raw(
                        ErrorKind::ValueValidation,
                        format!("unrecognized linkage modifier '{other}'"),
                    ));
                }
                None if kind == "masp" => Linkage::Dynamic,
                Some(_) | None => {
                    return Err(Error::raw(
                        ErrorKind::ValueValidation,
                        "invalid link library kind: supported values are 'masp'",
                    ));
                }
            },
            None => Linkage::Dynamic,
        };

        if name.is_empty() {
            return Err(Error::raw(
                ErrorKind::ValueValidation,
                "invalid link library: must specify a name or path",
            ));
        }

        let maybe_path = Path::new(name);
        let extension = maybe_path.extension().map(|ext| ext.to_str().unwrap());

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

            if !meta.is_file() {
                return Err(Error::raw(
                    ErrorKind::ValueValidation,
                    format!("invalid link library: '{}' is not a file", maybe_path.display()),
                ));
            }

            let name = maybe_path.file_stem().unwrap().to_str().unwrap().to_string();

            Ok(LinkLibrary {
                name: name.into(),
                path: Some(maybe_path.to_path_buf()),
                linkage,
            })
        } else if extension.is_some() {
            let name = name.strip_suffix(unsafe { extension.unwrap_unchecked() }).unwrap();
            let mut name = name.to_string();
            name.pop();

            Ok(LinkLibrary {
                name: name.into(),
                path: None,
                linkage,
            })
        } else {
            Ok(LinkLibrary {
                name: name.to_string().into(),
                path: None,
                linkage,
            })
        }
    }
}

/// Add libraries required by the target environment to the list of libraries to link against only
/// if they are not already present.
pub fn add_target_link_libraries(link_libraries: &mut Vec<LinkLibrary>, requires_protocol: bool) {
    if !link_libraries.iter().any(LinkLibrary::is_core) {
        link_libraries.push(LinkLibrary::core());
    }
    if requires_protocol && !link_libraries.iter().any(LinkLibrary::is_protocol) {
        link_libraries.push(LinkLibrary::protocol());
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn aliases_canonicalise_to_the_toolchain_file_stems() {
        for (alias, canonical) in [
            ("std", "miden-core"),
            ("core", "miden-core"),
            ("miden-core", "miden-core"),
            ("base", "miden-protocol"),
            ("protocol", "miden-protocol"),
            ("tx-kernel", "miden-tx-kernel"),
            ("my-lib", "my-lib"),
        ] {
            let lib = LinkLibrary {
                name: alias.into(),
                path: None,
                linkage: Linkage::Dynamic,
            };
            assert_eq!(lib.canonical_name(), canonical, "{alias}");
        }
    }

    #[test]
    fn a_missing_library_names_the_sysroot_and_search_paths_and_points_at_midenup() {
        let dir = tempfile::tempdir().unwrap();
        let sysroot = dir.path().join("toolchain");
        std::fs::create_dir_all(sysroot.join("lib")).unwrap();
        let options = crate::Options::new(
            None,
            None,
            dir.path().into(),
            dir.path().into(),
            None,
            Some(sysroot.clone()),
        );
        let err = LinkLibrary::core().load(&options).unwrap_err().to_string();
        assert!(err.contains("miden-core"), "{err}");
        assert!(err.contains(&sysroot.join("lib").display().to_string()), "{err}");
        assert!(err.contains("midenup"), "{err}");
    }

    #[test]
    fn a_library_in_the_sysroot_loads_by_alias() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("toolchain").join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let package = crate::registry::tests::fixture_package("miden-core");
        package.write_masp_file(&lib_dir).unwrap();
        let options = crate::Options::new(
            None,
            None,
            dir.path().into(),
            dir.path().into(),
            None,
            Some(dir.path().join("toolchain")),
        );
        let loaded = LinkLibrary {
            name: "std".into(),
            path: None,
            linkage: Linkage::Dynamic,
        }
        .load(&options)
        .unwrap();
        assert_eq!(AsRef::<str>::as_ref(&loaded.name), "miden-core");
    }
}
