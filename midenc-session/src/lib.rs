#![no_std]
#![feature(debug_closure_helpers)]
#![feature(specialization)]
// Specialization
#![allow(incomplete_features)]
#![deny(warnings)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
};

mod color;
pub mod diagnostics;
#[cfg(feature = "std")]
mod duration;
mod emit;
mod emitter;
pub mod flags;
mod inputs;
mod libs;
mod options;
mod outputs;
pub mod path;
pub mod registry;
#[cfg(feature = "std")]
mod statistics;

use alloc::{boxed::Box, fmt, sync::Arc};

/// The version associated with the current compiler toolchain
pub const MIDENC_BUILD_VERSION: &str = env!("MIDENC_BUILD_VERSION");

/// The git revision associated with the current compiler toolchain
pub const MIDENC_BUILD_REV: &str = env!("MIDENC_BUILD_REV");

pub use miden_assembly_syntax;
pub use miden_project;
use midenc_hir_symbol::Symbol;

pub use self::{
    color::ColorChoice,
    diagnostics::{DiagnosticsHandler, Emitter, Report, SourceManager},
    emit::{Emit, Writer},
    flags::{ArgMatches, CompileFlag, CompileFlags, FlagAction},
    inputs::{FileName, FileType, InputFile, InputType, InvalidInputError},
    libs::{LibraryPath, LibraryPathComponent, LinkLibrary, STDLIB, add_target_link_libraries},
    options::*,
    outputs::{OutputFile, OutputFiles, OutputMode, OutputType, OutputTypeSpec, OutputTypes},
    path::{Path, PathBuf},
};
#[cfg(feature = "std")]
pub use self::{duration::HumanDuration, emit::EmitExt, statistics::Statistics};

/// This struct provides access to all of the metadata and configuration
/// needed during a single compilation session.
pub struct Session {
    /// The name of this session
    pub name: String,
    /// Configuration for the current compiler session
    pub options: Box<Options>,
    /// The current source manager
    pub source_manager: Arc<dyn SourceManager + Send + Sync>,
    /// The current diagnostics handler
    pub diagnostics: Arc<DiagnosticsHandler>,
    /// The inputs being compiled
    pub input: Option<InputFile>,
    /// The outputs to be produced by the compiler during compilation
    pub output_files: OutputFiles,
    /// The project being compiled
    ///
    /// This may be a virtual manifest (i.e. materialized only in-memory)
    pub project: miden_project::Project,
    /// Statistics gathered from the current compiler session
    #[cfg(feature = "std")]
    pub statistics: Statistics,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .field("options", &self.options)
            .field("inputs", &self.input)
            .field("output_files", &self.output_files)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn new(
        input: InputFile,
        options: Box<Options>,
        emitter: Option<Arc<dyn Emitter>>,
        source_manager: Arc<dyn SourceManager + Send + Sync>,
    ) -> Result<Self, Report> {
        use miden_debug_types::Span;

        if matches!(input.file_type(), FileType::Toml) {
            let (pkgid, project) = match &input.file {
                InputType::Real(path) => {
                    let project = miden_project::Project::load(path, &source_manager)?;
                    let pkgid = match &project {
                        miden_project::Project::Package(pkg)
                        | miden_project::Project::WorkspacePackage { package: pkg, .. } => {
                            pkg.name().inner().clone()
                        }
                    };
                    (pkgid, project)
                }
                InputType::Stdin { name, input } => {
                    let content = core::str::from_utf8(input).map_err(|err| {
                        Report::msg(format!(
                            "unable to load source file '{name}' due to invalid utf-8: {err}"
                        ))
                    })?;
                    let source_file = source_manager.load(
                        miden_debug_types::SourceLanguage::Other("toml"),
                        miden_debug_types::Uri::new(name.as_str()),
                        content.to_string(),
                    );
                    let package = miden_project::Package::load(source_file)?;
                    let pkgid = package.name().inner().clone();
                    (pkgid, miden_project::Project::Package(package.into()))
                }
            };
            let name = options.name.clone().unwrap_or_else(|| pkgid.to_string());
            Ok(Self::new_project(name, None, project, options, emitter, source_manager))
        } else {
            let name = options
                .name
                .clone()
                .or_else(|| {
                    log::debug!(target: "driver", "no name specified, attempting to derive from output file");
                    options.output_file.as_ref().and_then(|of| of.filestem().map(|stem| stem.to_string()))
                })
                .unwrap_or_else(|| {
                    log::debug!(target: "driver", "unable to derive name from output file, deriving from input");
                    match &input {
                        InputFile {
                            file: InputType::Real(path),
                            ..
                        } => path
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .or_else(|| path.extension().and_then(|stem| stem.to_str()))
                            .unwrap_or_else(|| {
                                panic!(
                                    "invalid input path: '{}' has no file stem or extension",
                                    path.display()
                                )
                            })
                            .to_string(),
                            input @ InputFile {
                                file: InputType::Stdin { name, .. },
                                ..
                            } => {
                            let name = name.as_str();
                            if matches!(name, "empty" | "stdin") {
                                log::debug!(target: "driver", "no good input file name to use, using current directory base name");
                                options
                                    .current_dir
                                    .file_stem()
                                    .and_then(|stem| stem.to_str())
                                    .unwrap_or(name)
                                    .to_string()
                            } else {
                                input.filestem().to_owned()
                            }
                        }
                    }
                });
            log::debug!(target: "driver", "artifact name set to '{name}'");

            let mut default_target = miden_project::Target::r#virtual(
                options.target_type,
                name.clone(),
                miden_assembly_syntax::Path::new(name.as_str()).to_absolute().into_owned(),
            );
            if let InputType::Real(path) = &input.file {
                default_target.path = Some(Span::unknown(miden_project::Uri::from(path.as_path())));
            }
            let package = miden_project::Package::new(name.clone(), default_target);
            let project = miden_project::Project::Package(package.into());
            Ok(Self::new_project(name, Some(input), project, options, emitter, source_manager))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_project(
        name: String,
        input: Option<InputFile>,
        project: miden_project::Project,
        options: Box<Options>,
        emitter: Option<Arc<dyn Emitter>>,
        source_manager: Arc<dyn SourceManager + Send + Sync>,
    ) -> Self {
        log::debug!(target: "driver", "creating session {name}");
        if log::log_enabled!(target: "driver", log::Level::Debug) {
            if let Some(input) = input.as_ref() {
                log::debug!(
                    target: "driver",
                    " | input = {} ({})",
                    input.file_name(),
                    input.file_type(),
                );
            }
            log::debug!(
                target: "driver",
                " | outputs_dir = {}",
                options.output_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or("<unset>".to_string())
            );
            log::debug!(
                target: "driver",
                " | output_file = {}",
                options.output_file.as_ref().map(|of| of.to_string()).unwrap_or("<unset>".to_string())
            );
            log::debug!(target: "driver", " | target_dir = {}", options.target_dir.display());
        }
        let diagnostics = Arc::new(DiagnosticsHandler::new(
            options.diagnostics,
            source_manager.clone(),
            emitter.unwrap_or_else(|| options.default_emitter()),
        ));

        let output_dir = options
            .output_dir
            .as_deref()
            .or_else(|| options.output_file.as_ref().and_then(|of| of.parent()))
            .map(|path| path.to_path_buf());

        if let Some(output_dir) = output_dir.as_deref() {
            log::debug!(target: "driver", " | output dir = {}", output_dir.display());
        } else {
            log::debug!(target: "driver", " | output dir = <unset>");
        }

        log::debug!(target: "driver", " | target = {}", &options.target_type);
        if log::log_enabled!(target: "driver", log::Level::Debug) {
            for lib in options.link_libraries.iter() {
                if let Some(path) = lib.path.as_deref() {
                    log::debug!(target: "driver", " | linking library '{}' from {}", &lib.name, path.display());
                } else {
                    log::debug!(target: "driver", " | linking library '{}'", &lib.name);
                }
            }
        }

        let output_files = OutputFiles::new(
            name.clone(),
            options.current_dir.clone(),
            options.output_dir.clone().unwrap_or_else(|| options.current_dir.clone()),
            options.output_file.clone(),
            options.target_dir.clone(),
            options.output_types.clone(),
        );

        Self {
            name,
            options,
            source_manager,
            diagnostics,
            input,
            output_files,
            project,
            #[cfg(feature = "std")]
            statistics: Default::default(),
        }
    }

    #[doc(hidden)]
    pub fn with_output_type(mut self, ty: OutputType, path: Option<OutputFile>) -> Self {
        self.output_files.outputs.insert(ty, path.clone());
        self.options.output_types.insert(ty, path.clone());
        self
    }

    #[doc(hidden)]
    pub fn with_extra_flags(mut self, flags: CompileFlags) -> Self {
        self.options.set_extra_flags(flags);
        self
    }

    /// Get the value of a custom flag with action `FlagAction::SetTrue` or `FlagAction::SetFalse`
    #[inline]
    pub fn get_flag(&self, name: &str) -> bool {
        self.options.flags.get_flag(name)
    }

    /// Get the count of a specific custom flag with action `FlagAction::Count`
    #[inline]
    pub fn get_flag_count(&self, name: &str) -> usize {
        self.options.flags.get_flag_count(name)
    }

    /// Get the remaining [ArgMatches] left after parsing the base session configuration
    #[inline]
    pub fn matches(&self) -> &ArgMatches {
        self.options.flags.matches()
    }

    /// The name of this session (used as the name of the project, output file, etc.)
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a new package registry instance for this session
    pub fn package_registry(&self) -> Result<Box<registry::HybridPackageRegistry>, Report> {
        registry::HybridPackageRegistry::new(&self.options).map(Box::new)
    }

    /// Get the [OutputFile] to write the assembled MAST output to
    pub fn out_file(&self) -> OutputFile {
        let out_file = self.output_files.output_file(OutputType::Masp, None);

        if let OutputFile::Real(ref path) = out_file {
            self.check_file_is_writeable(path);
        }

        out_file
    }

    #[cfg(not(feature = "std"))]
    fn check_file_is_writeable(&self, file: &Path) {
        panic!(
            "Compiler exited with a fatal error: cannot write '{}' - compiler was built without \
             standard library",
            file.display()
        );
    }

    #[cfg(feature = "std")]
    fn check_file_is_writeable(&self, file: &Path) {
        if let Ok(m) = file.metadata()
            && m.permissions().readonly()
        {
            panic!("Compiler exited with a fatal error: file is not writeable: {}", file.display());
        }
    }

    /// Returns true if the compiler should exit after parsing the input
    pub fn parse_only(&self) -> bool {
        self.options.parse_only
    }

    /// Returns true if the compiler should exit after performing semantic analysis
    pub fn analyze_only(&self) -> bool {
        self.options.analyze_only
    }

    /// Returns true if the compiler should exit after applying rewrites to the IR
    pub fn rewrite_only(&self) -> bool {
        let link_or_masm_requested = self.should_link() || self.should_codegen();
        !self.options.parse_only && !self.options.analyze_only && !link_or_masm_requested
    }

    /// Returns true if an [OutputType] that requires linking + assembly was requested
    pub fn should_link(&self) -> bool {
        self.options.output_types.should_link() && !self.options.no_link
    }

    /// Returns true if an [OutputType] that requires generating Miden Assembly was requested
    pub fn should_codegen(&self) -> bool {
        self.options.output_types.should_codegen() && !self.options.link_only
    }

    /// Returns true if an [OutputType] that requires assembling MAST was requested
    pub fn should_assemble(&self) -> bool {
        self.options.output_types.should_assemble() && !self.options.link_only
    }

    /// Returns true if the given [OutputType] should be emitted as an output
    pub fn should_emit(&self, ty: OutputType) -> bool {
        self.options.output_types.contains_key(&ty)
    }

    /// Returns true if IR should be printed to stdout, after executing a pass named `pass`
    pub fn should_print_ir(&self, pass: &str) -> bool {
        self.options.print_ir_after_all
            || self.options.print_ir_after_pass.iter().any(|p| p == pass)
    }

    /// Returns true if IR should be printed to stdout, at the start of `stage`
    pub fn should_print_ir_before_stage(&self, stage: &str) -> bool {
        self.options.print_ir_before_stage.iter().any(|s| s == stage)
    }

    /// Returns true if CFG should be printed to stdout, after executing a pass named `pass`
    pub fn should_print_cfg(&self, pass: &str) -> bool {
        self.options.print_cfg_after_all
            || self.options.print_cfg_after_pass.iter().any(|p| p == pass)
    }

    /// Print the given emittable IR to stdout, as produced by a pass with name `pass`
    #[cfg(feature = "std")]
    pub fn print(&self, ir: impl Emit, pass: &str) -> anyhow::Result<()> {
        if self.should_print_ir(pass) {
            ir.write_to_stdout(self)?;
        }
        Ok(())
    }

    /// Get the path to emit the given [OutputType] to
    pub fn emit_to(&self, ty: OutputType, name: Option<Symbol>) -> Option<PathBuf> {
        if self.should_emit(ty) {
            match self.output_files.output_file(ty, name.map(|n| n.as_str())) {
                OutputFile::Real(path) => Some(path),
                OutputFile::Directory(_) => {
                    unreachable!("OutputFiles::output_file never returns OutputFile::Directory")
                }
                OutputFile::Stdout => None,
            }
        } else {
            None
        }
    }

    /// Emit an item to stdout/file system depending on the current configuration
    #[cfg(feature = "std")]
    pub fn emit<E: Emit>(&self, mode: OutputMode, item: &E) -> anyhow::Result<()> {
        let output_type = item.output_type(mode);
        if self.should_emit(output_type) {
            let name = item.name().map(|n| n.as_str());
            match self.output_files.output_file(output_type, name) {
                OutputFile::Real(path) => {
                    item.write_to_file(&path, mode, self)?;
                }
                OutputFile::Directory(_) => {
                    unreachable!("OutputFiles::output_file never returns OutputFile::Directory")
                }
                OutputFile::Stdout => {
                    let stdout = std::io::stdout().lock();
                    item.write_to(stdout, mode, self)?;
                }
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "std"))]
    pub fn emit<E: Emit>(&self, _mode: OutputMode, _item: &E) -> anyhow::Result<()> {
        Ok(())
    }
}
