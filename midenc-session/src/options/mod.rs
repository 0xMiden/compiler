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
    /// Build all packages in the current workspace (used by `cargo miden`)
    pub workspace: bool,
    /// Build the specified packages in the current workspace (used by `cargo miden`)
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
    /// Path prefixes to try when resolving relative paths in DWARF debug info
    pub trim_path_prefixes: Vec<PathBuf>,
    /// Print source location information in HIR output
    pub print_hir_source_locations: bool,
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
            trim_path_prefixes: vec![],
            print_hir_source_locations: false,
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

    /// Use this configuration to obtain a [Session] used for compilation
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

    /// Returns true if rich debugging information should be emitted by the compiler
    #[inline(always)]
    pub fn emit_debug_decorators(&self) -> bool {
        matches!(self.debug, DebugInfo::Full)
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

#[cfg(feature = "std")]
fn current_dir() -> PathBuf {
    std::env::current_dir().expect("could not get working directory")
}

#[cfg(not(feature = "std"))]
fn current_dir() -> PathBuf {
    PathBuf::from(".")
}
