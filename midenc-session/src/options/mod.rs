mod printing;

use alloc::{
    boxed::Box,
    fmt,
    str::FromStr,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};

use miden_debug_types::SourceManager;
use miden_project::TargetType;

pub use self::printing::IrFilter;
use crate::{
    ColorChoice, CompileFlags, InputFile, LinkLibrary, OutputFile, OutputTypes, PathBuf,
    diagnostics::{DiagnosticsConfig, Emitter, Report},
};

/// This struct contains all of the configuration options for the compiler
#[derive(Debug, Clone)]
pub struct Options {
    /// The path to the current project manifest, if present
    pub manifest_path: Option<PathBuf>,
    /// The name of the program being compiled
    pub name: Option<String>,
    /// The name of the function to call as the entrypoint
    pub entrypoint: Option<String>,
    /// The name of the build profile to use
    pub profile: String,
    /// Build all packages in the current workspace
    ///
    /// A manifest-backed Rust build selects on this — it builds every member of the workspace
    /// rather than one package — and forwards it to its nested `cargo build`.
    pub workspace: bool,
    /// Build the specified packages in the current workspace
    ///
    /// A manifest-backed Rust build selects on this — these are the workspace members it builds
    /// — and forwards them to its nested `cargo build`.
    pub packages: Vec<String>,
    /// The name of the current project target being compiled
    pub target: Option<String>,
    /// The type of target that was requested
    pub target_type: Option<TargetType>,
    /// The optimization level for the current program
    pub optimize: OptLevel,
    /// The level of debugging info for the current program
    pub debug: DebugInfo,
    /// The type of outputs to emit
    pub output_types: OutputTypes,
    /// The paths in which to search for Miden Assembly libraries to link against
    pub search_paths: Vec<PathBuf>,
    /// The set of Miden libraries to link against
    pub link_libraries: Vec<LinkLibrary>,
    /// A set of Miden Assembly modules to link against
    pub link_modules: Vec<(miden_assembly_syntax::PathBuf, String)>,
    /// The path to the current toolchain directory, which contains libraries and other tools that
    /// the compiler may use.
    ///
    /// This is expected to be set by `midenup` when the compiler is invoked via `miden` CLI
    pub sysroot: Option<PathBuf>,
    /// The path to `midenup`'s home directory
    ///
    /// This is expected to be set by `midenup` when the compiler is invoked via `miden` CLI
    pub midenup_home: Option<PathBuf>,
    /// The name of the current `midenup` toolchain
    ///
    /// This is expected to be set by `midenup` when the compiler is invoked via `miden` CLI
    pub toolchain: Option<String>,
    /// Whether, and how, to color terminal output
    pub color: ColorChoice,
    /// The current diagnostics configuration
    pub diagnostics: DiagnosticsConfig,
    /// The current working directory of the compiler
    pub current_dir: PathBuf,
    /// The target directory of the compiler
    pub target_dir: PathBuf,
    /// The artifact output directory of the compiler
    pub output_dir: Option<PathBuf>,
    /// The output file requested by the user, if requested
    pub output_file: Option<OutputFile>,
    /// Path prefixes to remap for any file paths encoded in debug info
    pub remap_path_prefixes: Vec<RemapPathPrefix>,
    /// Print source location information in HIR output
    pub print_hir_source_locations: bool,
    /// Stop compilation after the named checkpoint, as `--stop-after` asked.
    ///
    /// An alias declared by the route being compiled — `parse`, `analyze`, `transform`,
    /// `lower`, `assemble` — or a fully-qualified checkpoint id such as `hir.initial`. Which
    /// names are valid depends on the frontend the input selects, so the value is carried
    /// uninterpreted and resolved against that route once it is known; an unrecognized one is
    /// reported there, listing the names that route does accept.
    ///
    /// This is the general form of the `-C` stop flags below, and naming both is a usage error
    /// rather than a precedence rule.
    pub stop_after: Option<String>,
    /// Only parse inputs
    pub parse_only: bool,
    /// Only perform semantic analysis on the input
    pub analyze_only: bool,
    /// Run the linker on the inputs, but do not generate Miden Assembly
    pub link_only: bool,
    /// Generate Miden Assembly from the inputs without the linker
    pub no_link: bool,
    /// Run the experimental Miden Assembly linter prior to codegen
    ///
    /// This linter uses the HIR dataflow analysis framework to check for issues such as
    /// unconstrained advice usage.
    pub lint: bool,
    /// Print CFG to stdout after each pass
    pub print_cfg_after_all: bool,
    /// Print CFG to stdout each time the named passes are applied
    pub print_cfg_after_pass: Vec<String>,
    /// Print IR to stdout at the start of each stage
    pub print_ir_before_stage: Vec<String>,
    /// Print IR to stdout after each pass
    pub print_ir_after_all: bool,
    /// Print IR to stdout each time the named passes are applied
    pub print_ir_after_pass: Vec<String>,
    /// Only print the IR if the pass modified the IR structure.
    pub print_ir_after_modified: bool,
    /// Apply filters to what IR is printed, when printing is enabled
    pub print_ir_filters: Vec<IrFilter>,
    /// Save intermediate artifacts in memory during compilation
    pub save_temps: bool,
    /// Custom RUSTFLAGS to set when building Rust
    pub rustflags: Option<String>,
    /// Look for `cargo -Zscript`-style frontmatter when compiling standalone Rust sources
    pub cargo_frontmatter: bool,
    /// We store any leftover argument matches in the session options for use
    /// by any downstream crates that register custom flags
    pub flags: CompileFlags,
}

impl Default for Options {
    fn default() -> Self {
        let current_dir = current_dir();
        let target_dir = current_dir.join("target");
        Self::new(None, None, current_dir, target_dir, None, None)
    }
}

impl Options {
    pub fn new(
        name: Option<String>,
        target: Option<TargetType>,
        current_dir: PathBuf,
        target_dir: PathBuf,
        output_dir: Option<PathBuf>,
        sysroot: Option<PathBuf>,
    ) -> Self {
        let search_paths = if let Some(sysroot) = sysroot.as_deref() {
            let lib_dir = sysroot.join("lib");
            if lib_dir.try_exists().is_ok_and(|exists| exists) {
                vec![lib_dir]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Self {
            manifest_path: None,
            name,
            profile: "dev".to_string(),
            workspace: false,
            packages: vec![],
            target: None,
            target_type: target,
            entrypoint: None,
            optimize: OptLevel::None,
            debug: DebugInfo::None,
            output_types: Default::default(),
            search_paths,
            link_libraries: vec![],
            link_modules: vec![],
            sysroot,
            midenup_home: None,
            toolchain: None,
            color: Default::default(),
            diagnostics: Default::default(),
            current_dir,
            target_dir,
            output_dir,
            output_file: None,
            print_hir_source_locations: false,
            stop_after: None,
            parse_only: false,
            analyze_only: false,
            link_only: false,
            no_link: false,
            save_temps: false,
            lint: false,
            cargo_frontmatter: false,
            print_cfg_after_all: false,
            print_cfg_after_pass: vec![],
            print_ir_before_stage: vec![],
            print_ir_after_all: false,
            print_ir_after_pass: vec![],
            print_ir_after_modified: false,
            print_ir_filters: vec![],
            rustflags: None,
            remap_path_prefixes: vec![],
            flags: CompileFlags::default(),
        }
    }

    #[inline(always)]
    pub fn with_color(mut self: Box<Self>, color: ColorChoice) -> Box<Self> {
        self.color = color;
        self
    }

    #[inline(always)]
    pub fn with_verbosity(mut self: Box<Self>, verbosity: Verbosity) -> Box<Self> {
        self.diagnostics.verbosity = verbosity;
        self
    }

    #[inline(always)]
    pub fn with_debug_info(mut self: Box<Self>, debug: DebugInfo) -> Box<Self> {
        self.debug = debug;
        self
    }

    #[inline(always)]
    pub fn with_optimization(mut self: Box<Self>, level: OptLevel) -> Box<Self> {
        self.optimize = level;
        self
    }

    pub fn with_warnings(mut self: Box<Self>, warnings: Warnings) -> Box<Self> {
        self.diagnostics.warnings = warnings;
        self
    }

    pub fn with_output_types(
        mut self: Box<Self>,
        mut output_types: OutputTypes,
        output_file: Option<OutputFile>,
    ) -> Box<Self> {
        use crate::OutputType;
        let has_final_output = output_types.keys().any(|ty| matches!(ty, OutputType::Masp));
        if !has_final_output {
            // By default, we always produce a final artifact; `--emit` selects additional outputs.
            output_types.insert(OutputType::Masp, output_file);
        } else if output_file.is_some() && output_types.get(&OutputType::Masp).is_some() {
            // The -o flag overrides --emit
            output_types.insert(OutputType::Masp, output_file);
        }
        self.output_types = output_types;
        self
    }

    #[doc(hidden)]
    pub fn with_extra_flags(mut self: Box<Self>, flags: CompileFlags) -> Box<Self> {
        self.flags = flags;
        self
    }

    #[doc(hidden)]
    pub fn set_extra_flags(&mut self, flags: CompileFlags) {
        self.flags = flags;
    }

    /// Use this configuration to obtain a [crate::Session] used for compilation
    pub fn into_session(
        self: Box<Self>,
        input: InputFile,
        emitter: Option<Arc<dyn Emitter>>,
        source_manager: Option<Arc<dyn SourceManager + Send + Sync>>,
    ) -> Result<crate::Session, Report> {
        use crate::diagnostics::DefaultSourceManager;

        let source_manager =
            source_manager.unwrap_or_else(|| Arc::new(DefaultSourceManager::default()));
        crate::Session::new(input, self, emitter, source_manager)
    }

    /// Resolve the input a compilation request names.
    ///
    /// `input` is the input file given on the command line, if any. Without one, `--manifest-path`
    /// names the project to build. Absent both, the project is the `miden-project.toml` in the
    /// working directory, or the `Cargo.toml` there when no Miden manifest exists beside it. Given
    /// both, they must name the same file — a `Cargo.toml` counting as the `miden-project.toml`
    /// beside it — and the input is kept as given. Anything else is rejected rather than silently
    /// building one of the two.
    ///
    /// Whenever `--manifest-path` is given it is validated first, as a flag: it must name an
    /// existing manifest file, whether or not an input was given alongside it.
    ///
    /// A relative `--manifest-path` is relative to the directory the compiler is run from, exactly
    /// like a relative input file, and `--working-dir` does not move it. The default is the one
    /// thing here that *is* looked up in the working directory, which `--working-dir` sets.
    #[cfg(feature = "std")]
    pub fn resolve_input(&self, input: Option<InputFile>) -> Result<InputFile, Report> {
        use crate::diagnostics::IntoDiagnostic;

        match (input, self.manifest_path.as_deref()) {
            (Some(input), None) => Ok(input),
            (Some(input), Some(manifest_path)) => {
                // Checked before the comparison, so that a `--manifest-path` naming something
                // that is not a manifest is reported as such rather than as a mismatch — or, if
                // the input happens to name the same file, accepted.
                validate_manifest_path(manifest_path)?;
                // Compared by identity when the file exists, else by absolute path, so
                // `foo/Cargo.toml`, `./foo/miden-project.toml` and a path through `..` or a
                // symlink agree.
                let names_same_file = match input.as_path() {
                    Some(input_path) => {
                        project_identity(input_path)? == project_identity(manifest_path)?
                    }
                    // Bytes on standard input name no file, so they cannot name this one.
                    None => false,
                };
                if names_same_file {
                    Ok(input)
                } else {
                    Err(Report::msg(alloc::format!(
                        "input file '{}' and --manifest-path '{}' name different files; give one \
                         or the other",
                        input.file_name().as_str(),
                        manifest_path.display()
                    )))
                }
            }
            (None, Some(manifest_path)) => {
                validate_manifest_path(manifest_path)?;
                InputFile::from_path(manifest_path).into_diagnostic()
            }
            (None, None) => {
                let miden_manifest = self.current_dir.join(crate::MIDEN_MANIFEST_FILE_NAME);
                let cargo_manifest = self.current_dir.join("Cargo.toml");
                // The Miden manifest wins, and is also what a directory holding neither is
                // reported as missing — the project is named by its own manifest, not by Cargo's.
                let locator = if miden_manifest.is_file() || !cargo_manifest.is_file() {
                    miden_manifest
                } else {
                    cargo_manifest
                };
                InputFile::from_path(locator).into_diagnostic()
            }
        }
    }

    /// Get a new [Emitter] based on the current options.
    pub fn default_emitter(&self) -> Arc<dyn Emitter> {
        use crate::diagnostics::{DefaultEmitter, NullEmitter};

        match self.diagnostics.verbosity {
            Verbosity::Silent => Arc::new(NullEmitter::new(self.color)),
            _ => Arc::new(DefaultEmitter::new(self.color)),
        }
    }

    /// Returns true if source location information should be emitted by the compiler
    #[inline(always)]
    pub fn emit_source_locations(&self) -> bool {
        matches!(self.debug, DebugInfo::Line | DebugInfo::Full)
    }

    /// Returns true if rich debugging information should be emitted by the compiler.
    /// This enables AssemblyOp decorators which carry source location info for runtime errors.
    #[inline(always)]
    pub fn emit_debug_decorators(&self) -> bool {
        matches!(self.debug, DebugInfo::Line | DebugInfo::Full)
    }

    /// Returns true if debug assertions are enabled
    #[inline(always)]
    pub fn emit_debug_assertions(&self) -> bool {
        self.debug != DebugInfo::None && matches!(self.optimize, OptLevel::None | OptLevel::Basic)
    }

    /// Returns true if the requested target type is a protocol target
    pub fn target_requires_protocol(&self) -> bool {
        use miden_project::TargetType;
        !matches!(
            self.target_type,
            Some(TargetType::Kernel | TargetType::Executable | TargetType::Library) | None
        )
    }

    /// Returns true if the requested verbosity level is silent
    pub fn quiet(&self) -> bool {
        matches!(self.diagnostics.verbosity, Verbosity::Silent)
    }
}

/// This enum describes the degree to which compiled programs will be optimized
#[derive(Debug, Copy, Clone, Default)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum OptLevel {
    /// No optimizations at all
    None,
    /// Only basic optimizations are applied, e.g. constant propagation
    Basic,
    /// Most optimizations are applied, except when the cost is particularly high.
    #[default]
    Balanced,
    /// All optimizations are applied, with all tradeoffs in favor of runtime performance
    Max,
    /// Most optimizations are applied, but tuned to trade runtime performance for code size
    Size,
    /// Only optimizations which reduce code size are applied
    SizeMin,
}

/// This enum describes what type of debugging information to emit in compiled programs
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum DebugInfo {
    /// Do not emit debug info in the final output
    None,
    /// Emit source location information in the final output
    #[default]
    Line,
    /// Emit all available debug information in the final output
    Full,
}

/// This enum represents the behavior of the compiler with regard to warnings
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum Warnings {
    /// Disable all warnings
    None,
    /// Enable all warnings
    #[default]
    All,
    /// Promotes warnings to errors
    Error,
}
impl Warnings {
    #[inline]
    pub fn should_be_pedantic(&self) -> bool {
        matches!(self, Self::All)
    }

    #[inline]
    pub fn warnings_as_errors(&self) -> bool {
        matches!(self, Self::Error)
    }
}
impl fmt::Display for Warnings {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::All => f.write_str("auto"),
            Self::Error => f.write_str("error"),
        }
    }
}
impl FromStr for Warnings {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "all" => Ok(Self::All),
            "error" => Ok(Self::Error),
            _ => Err(()),
        }
    }
}

/// This enum represents the type of messages produced by the compiler during execution
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum Verbosity {
    /// Emit additional debug/trace information during compilation
    Debug,
    /// Emit the standard informational, warning, and error messages
    #[default]
    Info,
    /// Only emit warnings and errors
    Warning,
    /// Only emit errors
    Error,
    /// Do not emit anything to stdout/stderr
    Silent,
}

/// Represents the `--remap-path-prefix` flag, which rewrites source paths encoded in debug info
/// with a different path, typically to avoid encoding machine-specific details in artifacts.
#[derive(Debug, Clone)]
pub struct RemapPathPrefix {
    /// The path prefix to remap
    pub from: Box<crate::Path>,
    /// The remapped path prefix
    ///
    /// If `None`, the value `.` is used, representing the current working directory
    pub to: Option<Box<crate::Path>>,
}

impl RemapPathPrefix {
    pub fn source_prefix(&self) -> &crate::Path {
        &self.from
    }

    pub fn target_prefix(&self) -> &crate::Path {
        self.to.as_deref().unwrap_or(crate::Path::new(""))
    }
}

/// Parses `--remap-path-prefix=<from>`, `--remap-path-prefix=<from>=<to>`
#[doc(hidden)]
#[derive(Clone)]
#[cfg(feature = "std")]
pub struct RemapPathPrefixParser;

#[cfg(feature = "std")]
impl clap::builder::TypedValueParser for RemapPathPrefixParser {
    type Value = RemapPathPrefix;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let input = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;

        Ok(match input.split_once('=') {
            Some((from, to)) => RemapPathPrefix {
                from: PathBuf::from(from.trim()).into_boxed_path(),
                to: Some(PathBuf::from(to.trim()).into_boxed_path()),
            },
            None => RemapPathPrefix {
                from: PathBuf::from(input.trim()).into_boxed_path(),
                to: None,
            },
        })
    }
}

/// Check that `manifest_path`, as `--manifest-path` gave it, names a project manifest on disk.
///
/// The flag is validated here, on its own terms: it is not an input to compile but a way of
/// naming the project, so nothing downstream would report it as a flag. A *positional* input
/// keeps the checks its parser makes, and `normalize_locator` in `midenc-compile` is where a
/// positional locator's name is refused.
///
/// An existing file named `Cargo.toml` or `miden-project.toml` is accepted; everything else — a
/// missing path, a directory, a file by any other name — is a usage error.
#[cfg(feature = "std")]
fn validate_manifest_path(manifest_path: &crate::Path) -> Result<(), Report> {
    if !manifest_path.is_file() {
        return Err(Report::msg(alloc::format!(
            "--manifest-path '{}' does not exist or is not a file",
            manifest_path.display()
        )));
    }
    if !(crate::is_cargo_manifest(manifest_path) || crate::is_miden_manifest(manifest_path)) {
        return Err(Report::msg(alloc::format!(
            "--manifest-path '{}' is not a project manifest; expected a miden-project.toml or the \
             Cargo.toml beside it",
            manifest_path.display()
        )));
    }
    Ok(())
}

/// The identity of the Miden project the locator `path` names, for comparing two locators.
///
/// Canonical when the manifest is on disk, so that a path through `..` or a symlink is
/// recognized as the file it reaches; absolute otherwise, which is as far as two paths to a file
/// that does not exist can be compared. Both are exact: an unrepresentable path is an error, not
/// a silent mismatch.
#[cfg(feature = "std")]
fn project_identity(path: &crate::Path) -> Result<PathBuf, Report> {
    use crate::diagnostics::IntoDiagnostic;

    let manifest_path = crate::project_manifest_path(path);
    match std::fs::canonicalize(&manifest_path) {
        Ok(canonical) => Ok(canonical),
        Err(_) => std::path::absolute(&manifest_path).into_diagnostic(),
    }
}

#[cfg(feature = "std")]
fn current_dir() -> PathBuf {
    std::env::current_dir().expect("could not get working directory")
}

#[cfg(not(feature = "std"))]
fn current_dir() -> PathBuf {
    PathBuf::from(".")
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    /// Options for a compiler whose working directory is `/work`, with the given `--manifest-path`.
    fn options(manifest_path: Option<PathBuf>) -> Options {
        Options {
            manifest_path,
            current_dir: PathBuf::from("/work"),
            ..Options::default()
        }
    }

    fn input(path: impl AsRef<std::path::Path>) -> InputFile {
        InputFile::from_path(path).unwrap()
    }

    fn resolved_path(options: &Options, input: Option<InputFile>) -> PathBuf {
        options.resolve_input(input).unwrap().as_path().unwrap().to_path_buf()
    }

    /// Create a project manifest at `relative` under `dir`, and hand back the path to it.
    ///
    /// The contents are a minimal package manifest: `--manifest-path` is validated on the file's
    /// name and its existence, and nothing on this path parses it.
    fn manifest_at(dir: &std::path::Path, relative: &str) -> PathBuf {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[package]\nname = \"contract\"\n").unwrap();
        path
    }

    /// The manifest is taken as given: the working directory, `/work`, plays no part in it.
    #[test]
    fn the_manifest_path_names_the_project_when_no_input_is_given() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = manifest_at(dir.path(), "contract/Cargo.toml");

        assert_eq!(resolved_path(&options(Some(manifest.clone())), None), manifest);
    }

    /// Options for a compiler run from `dir`, with no `--manifest-path`.
    fn options_in(dir: &std::path::Path) -> Options {
        Options {
            current_dir: dir.to_path_buf(),
            ..Options::default()
        }
    }

    #[test]
    fn without_either_the_miden_manifest_in_the_working_directory_wins() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"work\"\n").unwrap();
        std::fs::write(dir.path().join("miden-project.toml"), "[package]\nname = \"work\"\n")
            .unwrap();

        assert_eq!(
            resolved_path(&options_in(dir.path()), None),
            dir.path().join("miden-project.toml")
        );
    }

    #[test]
    fn without_either_a_lone_cargo_manifest_is_the_project() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"work\"\n").unwrap();

        assert_eq!(resolved_path(&options_in(dir.path()), None), dir.path().join("Cargo.toml"));
    }

    /// A directory with no manifest at all is reported as missing its *Miden* manifest.
    #[test]
    fn without_either_and_without_any_manifest_the_miden_manifest_is_the_project() {
        let dir = tempfile::TempDir::new().unwrap();

        assert_eq!(
            resolved_path(&options_in(dir.path()), None),
            dir.path().join("miden-project.toml")
        );
    }

    #[test]
    fn an_input_alone_is_kept_as_given() {
        assert_eq!(
            resolved_path(&options(None), Some(input("foo.wasm"))),
            PathBuf::from("foo.wasm")
        );
    }

    /// A `Cargo.toml` names the `miden-project.toml` beside it, and `.` is normalized away.
    #[test]
    fn an_input_and_a_manifest_path_naming_the_same_project_keep_the_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let contract = manifest_at(dir.path(), "contract/miden-project.toml")
            .parent()
            .unwrap()
            .to_path_buf();
        let options = options(Some(contract.join(".").join("miden-project.toml")));

        assert_eq!(
            resolved_path(&options, Some(input(contract.join("Cargo.toml")))),
            contract.join("Cargo.toml")
        );
    }

    /// A manifest that exists is compared by identity, so a detour through `..` is not a mismatch.
    #[test]
    fn an_input_reaching_the_manifest_through_a_detour_keeps_the_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let contract = dir.path().join("contract");
        std::fs::create_dir(&contract).unwrap();
        std::fs::write(contract.join("miden-project.toml"), "[package]\nname = \"c\"\n").unwrap();

        let manifest_path = contract.join("miden-project.toml");
        let detour = contract.join("..").join("contract").join("Cargo.toml");
        let options = Options {
            manifest_path: Some(manifest_path),
            ..Options::default()
        };

        assert_eq!(
            resolved_path(&options, Some(input(detour.to_str().unwrap()))),
            detour,
            "the input is kept as given once both sides are seen to be the same manifest"
        );
    }

    /// A flag naming anything but an existing project manifest is a usage error, not an input.
    #[test]
    fn a_manifest_path_that_is_not_a_manifest_is_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let directory = dir.path().join("contract");
        std::fs::create_dir(&directory).unwrap();
        for name in ["toolchain", "rust-toolchain.toml", "foo.wat"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }

        let cases = [
            (directory, "does not exist or is not a file"),
            (dir.path().join("nowhere").join("Cargo.toml"), "does not exist or is not a file"),
            // A file that exists is refused on its name alone: neither an extension nor being
            // TOML makes a file a project manifest, and only the two manifest names name one.
            (dir.path().join("toolchain"), "is not a project manifest"),
            (dir.path().join("rust-toolchain.toml"), "is not a project manifest"),
            (dir.path().join("foo.wat"), "is not a project manifest"),
        ];

        for (manifest_path, expected) in cases {
            let err = options(Some(manifest_path.clone())).resolve_input(None).unwrap_err();
            assert!(
                err.to_string().contains(expected),
                "expected '{expected}' for '{}', got: {err}",
                manifest_path.display()
            );
        }
    }

    #[test]
    fn an_input_and_a_manifest_path_naming_different_files_are_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = manifest_at(dir.path(), "contract/Cargo.toml");

        for input_path in [
            dir.path().join("other").join("Cargo.toml"),
            dir.path().join("contract").join("target").join("foo.wasm"),
        ] {
            let err = options(Some(manifest.clone()))
                .resolve_input(Some(input(input_path)))
                .unwrap_err();
            assert!(err.to_string().contains("name different files"), "{err}");
        }
    }
}
