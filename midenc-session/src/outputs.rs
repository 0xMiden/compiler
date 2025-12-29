use alloc::{
    borrow::{Cow, ToOwned},
    collections::BTreeMap,
    fmt, format,
    str::FromStr,
    string::String,
};

use crate::{Path, PathBuf};

/// Escape `name` for use as a single filesystem path component (e.g. a file stem).
///
/// This is used when emitting artifacts whose names may contain characters that are legal in
/// compiler/session identifiers, but are problematic (or even invalid) as filenames on common
/// filesystems.
fn escape_path_component(name: &str) -> Cow<'_, str> {
    if name.is_empty() {
        return Cow::Borrowed("_");
    }

    let is_safe = name != "."
        && name != ".."
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if is_safe {
        return Cow::Borrowed(name);
    }

    let mut escaped = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            escaped.push(ch);
        } else {
            escaped.push('_');
        }
    }

    match escaped.as_str() {
        "" | "." | ".." => Cow::Borrowed("_"),
        _ => Cow::Owned(escaped),
    }
}

/// The type of output to produce for a given [OutputType], when multiple options are available
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputMode {
    /// Pretty-print the textual form of the current [OutputType]
    Text,
    /// Encode the current [OutputType] in its canonical binary format
    Binary,
}

/// This enum represents the type of outputs the compiler can produce
#[derive(Debug, Copy, Clone, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum OutputType {
    /// The compiler will emit the parse tree of the input, if applicable
    Ast,
    /// The compiler will emit WebAssembly text format (WAT), if applicable
    Wat,
    /// The compiler will emit Miden IR
    Hir,
    /// The compiler will emit Miden Assembly text
    Masm,
    /// The compiler will emit a Merkalized Abstract Syntax Tree in text form
    Mast,
    /// The compiler will emit a MAST library in binary form
    Masl,
    /// The compiler will emit a MAST package in binary form
    #[default]
    Masp,
}
impl OutputType {
    /// Returns true if this output type is an intermediate artifact produced during compilation
    pub fn is_intermediate(&self) -> bool {
        !matches!(self, Self::Mast | Self::Masl | Self::Masp)
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Ast => "ast",
            Self::Wat => "wat",
            Self::Hir => "hir",
            Self::Masm => "masm",
            Self::Mast => "mast",
            Self::Masl => "masl",
            Self::Masp => "masp",
        }
    }

    pub fn shorthand_display() -> String {
        format!(
            "`{}`, `{}`, `{}`, `{}`, `{}`, `{}`, `{}`",
            Self::Ast,
            Self::Wat,
            Self::Hir,
            Self::Masm,
            Self::Mast,
            Self::Masl,
            Self::Masp,
        )
    }

    pub fn all() -> [OutputType; 7] {
        [
            OutputType::Ast,
            OutputType::Wat,
            OutputType::Hir,
            OutputType::Masm,
            OutputType::Mast,
            OutputType::Masl,
            OutputType::Masp,
        ]
    }

    /// Returns the subset of [OutputType] values considered "intermediate" for convenience
    /// emission (WAT, HIR, MASM).
    pub fn ir() -> [OutputType; 3] {
        [OutputType::Wat, OutputType::Hir, OutputType::Masm]
    }
}
impl fmt::Display for OutputType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Ast => f.write_str("ast"),
            Self::Wat => f.write_str("wat"),
            Self::Hir => f.write_str("hir"),
            Self::Masm => f.write_str("masm"),
            Self::Mast => f.write_str("mast"),
            Self::Masl => f.write_str("masl"),
            Self::Masp => f.write_str("masp"),
        }
    }
}
impl FromStr for OutputType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ast" => Ok(Self::Ast),
            "wat" => Ok(Self::Wat),
            "hir" => Ok(Self::Hir),
            "masm" => Ok(Self::Masm),
            "mast" => Ok(Self::Mast),
            "masl" => Ok(Self::Masl),
            "masp" => Ok(Self::Masp),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum OutputFile {
    Real(PathBuf),
    /// A directory in which to place outputs.
    ///
    /// This is distinct from [OutputFile::Real] because callers may want a path to be treated as a
    /// directory even if it does not exist yet.
    Directory(PathBuf),
    Stdout,
}
impl OutputFile {
    pub fn parent(&self) -> Option<&Path> {
        match self {
            Self::Real(path) => path.parent(),
            Self::Directory(path) => Some(path.as_ref()),
            Self::Stdout => None,
        }
    }

    pub fn filestem(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::Real(path) => path.file_stem().map(|stem| stem.to_string_lossy()),
            Self::Directory(_) => None,
            Self::Stdout => None,
        }
    }

    pub fn is_stdout(&self) -> bool {
        matches!(self, Self::Stdout)
    }

    #[cfg(feature = "std")]
    pub fn is_tty(&self) -> bool {
        use std::io::IsTerminal;
        match self {
            Self::Real(_) => false,
            Self::Directory(_) => false,
            Self::Stdout => std::io::stdout().is_terminal(),
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn is_tty(&self) -> bool {
        false
    }

    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Real(path) => Some(path.as_ref()),
            Self::Directory(path) => Some(path.as_ref()),
            Self::Stdout => None,
        }
    }

    pub fn file_for_writing(
        &self,
        outputs: &OutputFiles,
        ty: OutputType,
        name: Option<&str>,
    ) -> PathBuf {
        match self {
            Self::Real(path) => path.clone(),
            Self::Directory(dir) => {
                let dir = if dir.is_absolute() {
                    dir.clone()
                } else {
                    outputs.cwd.join(dir)
                };
                let stem = escape_path_component(name.unwrap_or(outputs.stem.as_str()));
                dir.join(stem.as_ref()).with_extension(ty.extension())
            }
            Self::Stdout => outputs.temp_path(ty, name),
        }
    }
}
impl fmt::Display for OutputFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Real(path) => write!(f, "{}", path.display()),
            Self::Directory(path) => write!(f, "{}", path.display()),
            Self::Stdout => write!(f, "stdout"),
        }
    }
}

#[derive(Debug)]
pub struct OutputFiles {
    stem: String,
    /// The compiler working directory
    pub cwd: PathBuf,
    /// The directory in which to place temporaries or intermediate artifacts
    pub tmp_dir: PathBuf,
    /// The directory in which to place objects produced by the current compiler operation
    ///
    /// This directory is intended for non-intermediate artifacts, though it may be used
    /// to derive `tmp_dir` elsewhere. You should prefer to use `tmp_dir` for files which
    /// are internal details of the compiler.
    pub out_dir: PathBuf,
    /// If specified, the specific path at which to write the compiler output.
    ///
    /// This _only_ applies to the final output, i.e. the `.masl` library or executable.
    pub out_file: Option<OutputFile>,
    /// The raw output types requested by the user on the command line
    pub outputs: OutputTypes,
}
impl OutputFiles {
    pub fn new(
        stem: String,
        cwd: PathBuf,
        out_dir: PathBuf,
        out_file: Option<OutputFile>,
        tmp_dir: PathBuf,
        outputs: OutputTypes,
    ) -> Self {
        Self {
            stem,
            cwd,
            tmp_dir,
            out_dir,
            out_file,
            outputs,
        }
    }

    /// Return the [OutputFile] representing where an output of `ty` type should be written,
    /// with an optional `name`, which overrides the file stem of the resulting path.
    pub fn output_file(&self, ty: OutputType, name: Option<&str>) -> OutputFile {
        let requested = self.outputs.contains_key(&ty);
        let default_name = escape_path_component(name.unwrap_or(self.stem.as_str()));
        match self.outputs.get(&ty).and_then(|p| p.to_owned()) {
            Some(OutputFile::Real(path)) => OutputFile::Real({
                let path = if path.is_absolute() {
                    path
                } else {
                    self.cwd.join(path)
                };
                if path.is_dir() {
                    path.join(default_name.as_ref()).with_extension(ty.extension())
                } else if let Some(name) = name {
                    let name = escape_path_component(name);
                    path.with_stem_and_extension(name.as_ref(), ty.extension())
                } else {
                    path
                }
            }),
            Some(OutputFile::Directory(dir)) => OutputFile::Real({
                let dir = if dir.is_absolute() {
                    dir
                } else {
                    self.cwd.join(dir)
                };
                dir.join(default_name.as_ref()).with_extension(ty.extension())
            }),
            Some(OutputFile::Stdout) => OutputFile::Stdout,
            None => {
                // If the user requested an output type without specifying a destination, default to
                // the session output directory (i.e. the working directory by default). Only
                // compiler-internal temporaries use `tmp_dir`.
                let out = if ty.is_intermediate() {
                    if requested {
                        self.with_directory_and_extension(&self.out_dir, ty.extension())
                    } else {
                        self.with_directory_and_extension(&self.tmp_dir, ty.extension())
                    }
                } else if let Some(output_file) = self.out_file.as_ref() {
                    return output_file.clone();
                } else {
                    self.with_directory_and_extension(&self.out_dir, ty.extension())
                };
                OutputFile::Real(if let Some(name) = name {
                    let name = escape_path_component(name);
                    out.with_stem(name.as_ref())
                } else {
                    out
                })
            }
        }
    }

    /// Return the most appropriate file path for an output of `ty` type.
    ///
    /// The returned path _may_ be precise, if a specific file path was chosen by the user for
    /// the given output type, but in general the returned path will be derived from the current
    /// `self.stem`, and is thus an appropriate default path for the given output.
    pub fn output_path(&self, ty: OutputType) -> PathBuf {
        match self.output_file(ty, None) {
            OutputFile::Real(path) => path,
            OutputFile::Directory(_) => {
                unreachable!("OutputFiles::output_file never returns OutputFile::Directory")
            }
            OutputFile::Stdout => {
                if ty.is_intermediate() {
                    self.with_directory_and_extension(&self.tmp_dir, ty.extension())
                } else if let Some(output_file) = self.out_file.as_ref().and_then(|of| of.as_path())
                {
                    output_file.to_path_buf()
                } else {
                    self.with_directory_and_extension(&self.out_dir, ty.extension())
                }
            }
        }
    }

    /// Constructs a file path for a temporary file of the given output type, with an optional name,
    /// falling back to `self.stem` if no name is provided.
    ///
    /// The file path is always a child of `self.tmp_dir`
    pub fn temp_path(&self, ty: OutputType, name: Option<&str>) -> PathBuf {
        let name = escape_path_component(name.unwrap_or(self.stem.as_str()));
        self.tmp_dir.join(name.as_ref()).with_extension(ty.extension())
    }

    /// Build a file path which is either:
    ///
    /// * If `self.out_file` is set to a real path, returns it with extension set to `extension`
    /// * Otherwise, calls [with_directory_and_extension] with `self.out_dir` and `extension`
    pub fn with_extension(&self, extension: &str) -> PathBuf {
        match self.out_file.as_ref() {
            Some(OutputFile::Real(path)) => path.with_extension(extension),
            Some(OutputFile::Directory(dir)) => {
                let dir = if dir.is_absolute() {
                    dir.clone()
                } else {
                    self.cwd.join(dir)
                };
                self.with_directory_and_extension(&dir, extension)
            }
            Some(OutputFile::Stdout) | None => {
                self.with_directory_and_extension(&self.out_dir, extension)
            }
        }
    }

    /// Build a file path whose parent is `directory`, file stem is `self.stem`, and extension is
    /// `extension`
    #[inline]
    fn with_directory_and_extension(&self, directory: &Path, extension: &str) -> PathBuf {
        let stem = escape_path_component(&self.stem);
        directory.join(stem.as_ref()).with_extension(extension)
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutputTypes(BTreeMap<OutputType, Option<OutputFile>>);
impl OutputTypes {
    #[cfg(feature = "std")]
    pub fn new<I: IntoIterator<Item = OutputTypeSpec>>(entries: I) -> Result<Self, clap::Error> {
        let entries = entries.into_iter();
        let mut map = BTreeMap::default();
        for spec in entries {
            match spec {
                OutputTypeSpec::All { path } => {
                    if !map.is_empty() {
                        return Err(clap::Error::raw(
                            clap::error::ErrorKind::ValueValidation,
                            "--emit=all cannot be combined with other --emit types",
                        ));
                    }
                    let path = match path {
                        None => None,
                        Some(OutputFile::Real(path)) => {
                            if path.extension().is_some() {
                                return Err(clap::Error::raw(
                                    clap::error::ErrorKind::ValueValidation,
                                    "invalid path for --emit=all: must be a directory",
                                ));
                            }
                            Some(OutputFile::Directory(path))
                        }
                        Some(OutputFile::Directory(path)) => {
                            if path.extension().is_some() {
                                return Err(clap::Error::raw(
                                    clap::error::ErrorKind::ValueValidation,
                                    "invalid path for --emit=all: must be a directory",
                                ));
                            }
                            Some(OutputFile::Directory(path))
                        }
                        Some(OutputFile::Stdout) => Some(OutputFile::Stdout),
                    };
                    for ty in OutputType::all() {
                        map.insert(ty, path.clone());
                    }
                }
                OutputTypeSpec::Inter { path } => {
                    // Emit a bundle of intermediate artifacts into the same directory.
                    for output_type in OutputType::ir() {
                        match map.get(&output_type) {
                            // If the user already chose an explicit destination for this type,
                            // don't allow `ir`/`inter` to override it.
                            Some(Some(_)) => {
                                return Err(clap::Error::raw(
                                    clap::error::ErrorKind::ValueValidation,
                                    format!(
                                        "conflicting --emit options given for output type \
                                         '{output_type}'"
                                    ),
                                ));
                            }
                            _ => {
                                // If the user requested the type without a destination, or hasn't
                                // requested it at all yet, route it to the `ir` directory.
                                map.insert(output_type, path.clone());
                            }
                        }
                    }
                }
                OutputTypeSpec::Typed { output_type, path } => {
                    if path.is_some() {
                        if matches!(map.get(&output_type), Some(Some(_))) {
                            return Err(clap::Error::raw(
                                clap::error::ErrorKind::ValueValidation,
                                format!(
                                    "conflicting --emit options given for output type \
                                     '{output_type}'"
                                ),
                            ));
                        }
                    } else if matches!(map.get(&output_type), Some(Some(_))) {
                        continue;
                    }
                    map.insert(output_type, path);
                }
            }
        }
        Ok(Self(map))
    }

    pub fn get(&self, key: &OutputType) -> Option<&Option<OutputFile>> {
        self.0.get(key)
    }

    pub fn insert(&mut self, key: OutputType, value: Option<OutputFile>) {
        self.0.insert(key, value);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains_key(&self, key: &OutputType) -> bool {
        self.0.contains_key(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&OutputType, &Option<OutputFile>)> + '_ {
        self.0.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = OutputType> + '_ {
        self.0.keys().copied()
    }

    pub fn values(&self) -> impl Iterator<Item = Option<&OutputFile>> {
        self.0.values().map(|v| v.as_ref())
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn should_link(&self) -> bool {
        self.0.keys().any(|k| {
            matches!(
                k,
                OutputType::Hir
                    | OutputType::Masm
                    | OutputType::Mast
                    | OutputType::Masl
                    | OutputType::Masp
            )
        })
    }

    pub fn should_codegen(&self) -> bool {
        self.0.keys().any(|k| {
            matches!(k, OutputType::Masm | OutputType::Mast | OutputType::Masl | OutputType::Masp)
        })
    }

    pub fn should_assemble(&self) -> bool {
        self.0
            .keys()
            .any(|k| matches!(k, OutputType::Mast | OutputType::Masl | OutputType::Masp))
    }
}

/// This type describes an output type with optional path specification
#[derive(Debug, Clone)]
pub enum OutputTypeSpec {
    All {
        path: Option<OutputFile>,
    },
    /// Emit a common set of intermediate artifacts (WAT, HIR, MASM) into a directory.
    ///
    /// The directory path is interpreted relative to the session working directory.
    Inter {
        path: Option<OutputFile>,
    },
    Typed {
        output_type: OutputType,
        path: Option<OutputFile>,
    },
}

#[cfg(feature = "std")]
impl clap::builder::ValueParserFactory for OutputTypeSpec {
    type Parser = OutputTypeParser;

    fn value_parser() -> Self::Parser {
        OutputTypeParser
    }
}

#[doc(hidden)]
#[derive(Clone)]
#[cfg(feature = "std")]
pub struct OutputTypeParser;

#[cfg(feature = "std")]
impl clap::builder::TypedValueParser for OutputTypeParser {
    type Value = OutputTypeSpec;

    fn possible_values(
        &self,
    ) -> Option<alloc::boxed::Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        use alloc::boxed::Box;

        use clap::builder::PossibleValue;
        Some(Box::new(
            [
                PossibleValue::new("ast").help("Abstract Syntax Tree (text)"),
                PossibleValue::new("wat").help("WebAssembly text format (text)"),
                PossibleValue::new("hir").help("High-level Intermediate Representation (text)"),
                PossibleValue::new("masm").help("Miden Assembly (text)"),
                PossibleValue::new("mast").help("Merkelized Abstract Syntax Tree (text)"),
                PossibleValue::new("masl").help("Merkelized Abstract Syntax Tree (binary)"),
                PossibleValue::new("masp").help("Miden Assembly Package Format (binary)"),
                PossibleValue::new("ir").help("WAT + HIR + MASM (text, optional directory)"),
                PossibleValue::new("all").help("All of the above"),
            ]
            .into_iter(),
        ))
    }

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let output_type = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;

        let (shorthand, path) = match output_type.split_once('=') {
            None => (output_type, None),
            Some((shorthand, "-")) => (shorthand, Some(OutputFile::Stdout)),
            Some((shorthand, path)) => (shorthand, Some(OutputFile::Real(PathBuf::from(path)))),
        };
        if shorthand == "all" {
            let path = match path {
                None => None,
                Some(OutputFile::Real(path)) => Some(OutputFile::Directory(path)),
                Some(OutputFile::Stdout) => Some(OutputFile::Stdout),
                Some(OutputFile::Directory(_)) => unreachable!("all path is parsed as real"),
            };
            return Ok(OutputTypeSpec::All { path });
        }
        if shorthand == "ir" || shorthand == "inter" {
            let path = match path {
                None => None,
                Some(OutputFile::Real(path)) => Some(OutputFile::Directory(path)),
                Some(OutputFile::Stdout) => {
                    return Err(Error::raw(
                        ErrorKind::InvalidValue,
                        format!("invalid output type: `{shorthand}=-` - expected `ir[=PATH]`"),
                    ));
                }
                Some(OutputFile::Directory(_)) => unreachable!("ir path is parsed as real"),
            };
            return Ok(OutputTypeSpec::Inter { path });
        }
        let output_type = shorthand.parse::<OutputType>().map_err(|_| {
            Error::raw(
                ErrorKind::InvalidValue,
                format!(
                    "invalid output type: `{shorthand}` - expected one of: {display}, `all`, \
                     `ir[=PATH]`",
                    display = OutputType::shorthand_display(),
                ),
            )
        })?;
        Ok(OutputTypeSpec::Typed { output_type, path })
    }
}

#[cfg(feature = "std")]
trait PathMut {
    fn with_stem(self, stem: impl AsRef<std::ffi::OsStr>) -> PathBuf;
    fn with_stem_and_extension(
        self,
        stem: impl AsRef<std::ffi::OsStr>,
        ext: impl AsRef<std::ffi::OsStr>,
    ) -> PathBuf;
}
#[cfg(feature = "std")]
impl PathMut for &std::path::Path {
    fn with_stem(self, stem: impl AsRef<std::ffi::OsStr>) -> std::path::PathBuf {
        let mut path = self.with_file_name(stem);
        if let Some(ext) = self.extension() {
            path.set_extension(ext);
        }
        path
    }

    fn with_stem_and_extension(
        self,
        stem: impl AsRef<std::ffi::OsStr>,
        ext: impl AsRef<std::ffi::OsStr>,
    ) -> std::path::PathBuf {
        let mut path = self.with_file_name(stem);
        path.set_extension(ext);
        path
    }
}
#[cfg(feature = "std")]
impl PathMut for std::path::PathBuf {
    fn with_stem(mut self, stem: impl AsRef<std::ffi::OsStr>) -> std::path::PathBuf {
        if let Some(ext) = self.extension() {
            let ext = ext.to_string_lossy().into_owned();
            self.with_stem_and_extension(stem, ext)
        } else {
            self.set_file_name(stem);
            self
        }
    }

    fn with_stem_and_extension(
        mut self,
        stem: impl AsRef<std::ffi::OsStr>,
        ext: impl AsRef<std::ffi::OsStr>,
    ) -> std::path::PathBuf {
        self.set_file_name(stem);
        self.set_extension(ext);
        self
    }
}
