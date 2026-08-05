#![no_std]
#![feature(debug_closure_helpers)]
#![feature(specialization)]
// Specialization
#![allow(incomplete_features)]
#![deny(warnings)]

#[macro_use]
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
#[cfg(any(test, feature = "std"))]
mod package_cache;
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
pub use miden_mast_package::PackageId;
pub use miden_package_registry;
pub use miden_project;
use midenc_hir_symbol::Symbol;

pub use self::{
    color::ColorChoice,
    diagnostics::{DiagnosticsHandler, Emitter, Report, SourceManager},
    emit::{Emit, Writer},
    flags::{ArgMatches, CompileFlag, CompileFlags, FlagAction},
    inputs::{FileName, FileType, InputFile, InputType, InvalidInputError},
    libs::{LibraryPath, LibraryPathComponent, LinkLibrary, add_target_link_libraries},
    options::*,
    outputs::{OutputFile, OutputFiles, OutputMode, OutputType, OutputTypeSpec, OutputTypes},
    path::{Path, PathBuf},
};
#[cfg(feature = "std")]
pub use self::{duration::HumanDuration, emit::EmitExt, statistics::Statistics};

/// This struct provides access to all of the metadata and configuration
/// needed during a single compilation session.
#[derive(Clone)]
pub struct Session {
    /// The name of this session
    pub name: String,
    /// Configuration for the current compiler session
    pub options: Box<Options>,
    /// The current source manager
    pub source_manager: Arc<dyn SourceManager>,
    /// The current diagnostics handler
    pub diagnostics: Arc<DiagnosticsHandler>,
    /// The inputs being compiled
    pub input: Option<InputFile>,
    /// The outputs to be produced by the compiler during compilation
    pub output_files: OutputFiles,
    /// Statistics gathered from the current compiler session
    #[cfg(feature = "std")]
    pub statistics: Statistics,
    /// The build-input fingerprint used to isolate this session's package cache.
    ///
    /// Memoization assumes fingerprint-relevant [`Options`] are not mutated after the first cache
    /// path request.
    #[cfg(feature = "std")]
    package_cache_fingerprint: std::sync::OnceLock<String>,
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
    /// Open a session compiling `input` under `options`.
    ///
    /// # A project locator is read for its facts, not loaded as a project
    ///
    /// A `.toml` input is a *locator*: it names the project to build rather than being something
    /// the compiler compiles. Three facts about that project are needed before this session
    /// exists, because this constructor is downstream of none of them:
    ///
    /// - the **package name**, which is the artifact name absent `--name`, and which
    ///   [`OutputFiles`] is built from below;
    /// - the **library target's kind**, which is what [`Options::target_type`] defaults to, and
    ///   which [`add_target_link_libraries`] then consults to decide whether the Miden protocol
    ///   is linked;
    /// - the **executable targets' names**, from which [`Options::entrypoint`] is defaulted.
    ///
    /// All three come from `ProjectManifest`, which parses the manifest's *AST* and reads
    /// exactly those three things out of it with `miden_project`'s own extractors. What it
    /// deliberately does not do is build a [`miden_project::Project`]: loading the project is
    /// `prepare_project`'s, in `midenc-compile`, and the package it loads is the one that gets
    /// assembled. A session that loaded its own would be loading the same manifest twice to
    /// produce a package nothing compiles.
    ///
    /// **Failing to read the manifest is not an error here.** Which project a locator names, and
    /// whether it names one at all, is decided and reported downstream — where the locator is
    /// normalized anyway, and where a Cargo workspace root gets the diagnostic that belongs to
    /// it rather than a "no such file" for the `miden-project.toml` a workspace root does not
    /// have. So an unreadable manifest falls back to the same name derivation a source-file
    /// input uses, and leaves `target_type` and `entrypoint` alone.
    pub fn new(
        input: InputFile,
        mut options: Box<Options>,
        emitter: Option<Arc<dyn Emitter>>,
        source_manager: Arc<dyn SourceManager + Send + Sync>,
    ) -> Result<Self, Report> {
        let manifest = if matches!(input.file_type(), FileType::Toml) {
            ProjectManifest::read(&input, source_manager.as_ref())?
        } else {
            None
        };

        if let Some(manifest) = manifest.as_ref() {
            if options.target_type.is_none() {
                options.target_type = Some(manifest.library_target_type());
            }
            if is_cargo_project_input(&input) {
                infer_cargo_project_entrypoint(manifest, &mut options)?;
            }
        }

        let name = options
            .name
            .clone()
            .or_else(|| manifest.as_ref().map(|manifest| manifest.name.to_string()))
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

        // Where `prepare_temporary_cargo_project` copies a standalone Rust source to, mapped back
        // to where the source came from, so that debug information names the file the user wrote.
        // Only for a source-file input: a project is built by `cargo` in place, and is never
        // copied anywhere.
        if !matches!(input.file_type(), FileType::Toml)
            && let InputType::Real(path) = &input.file
        {
            #[cfg(feature = "std")]
            {
                let tmp = std::env::temp_dir().canonicalize().unwrap();
                let project_dir = tmp.join(&name).join("src");
                let project_remap_target = if path.is_absolute() {
                    Some(
                        path.strip_prefix(&options.current_dir)
                            .ok()
                            .or(path.as_path().parent())
                            .unwrap()
                            .to_path_buf()
                            .into_boxed_path(),
                    )
                } else {
                    path.parent().map(|p| p.to_path_buf().into_boxed_path())
                };
                options.remap_path_prefixes.push(RemapPathPrefix {
                    from: project_dir.into_boxed_path(),
                    to: project_remap_target,
                });
            }
        }

        Ok(Self::new_project(name, Some(input), options, emitter, source_manager))
    }

    /// Open a session named `name`, for a caller that already knows what it is building.
    ///
    /// [`Session::new`] derives `name` from its input; this takes it. Both then do the same
    /// thing, and neither knows anything about the project being built beyond its name.
    pub fn new_project(
        name: String,
        input: Option<InputFile>,
        mut options: Box<Options>,
        emitter: Option<Arc<dyn Emitter>>,
        source_manager: Arc<dyn SourceManager>,
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

        log::debug!(target: "driver", " | target = {}", options.target_type.map(|tt| tt.to_string()).unwrap_or("none specified".to_string()));
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

        create_target_dir(options.target_dir.as_path());
        create_target_dir(&options.target_dir.as_path().join(&options.profile));

        // Link against implicitly required libraries
        let requires_protocol = options.target_requires_protocol();
        add_target_link_libraries(&mut options.link_libraries, requires_protocol);

        Self {
            name,
            options,
            source_manager,
            diagnostics,
            input,
            output_files,
            #[cfg(feature = "std")]
            statistics: Default::default(),
            #[cfg(feature = "std")]
            package_cache_fingerprint: Default::default(),
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
        registry::HybridPackageRegistry::new_with_filesystem_cache(
            &self.options,
            self.filesystem_package_cache_dir(),
        )
        .map(Box::new)
    }

    /// Where compiled dependency packages of this session's project are published and looked for.
    ///
    /// `None` unless this session's input is a project locator: with `std`, the cache lives under
    /// the project's own `target/miden/packages/<fingerprint>/` directory, and a session compiling
    /// a standalone source file has no project directory to put one under. The fingerprint covers
    /// the compiler identity, relevant build options, and the project's manifest closure. Both
    /// readers — this session's package registry and the nested `cargo` builds a Rust project's
    /// dependencies run through — must agree on the answer, which is why there is one derivation
    /// of it. Without `std`, this returns the existing flat `target/miden/packages/` path without a
    /// fingerprint component.
    ///
    /// Derived from the input locator rather than from a loaded manifest, which is what
    /// [`Session::new`] no longer has. That is also a repair: the manifest path was previously
    /// taken from a package that `fixup_cargo_target` had rebuilt for every executable
    /// `Cargo.toml` input, and a rebuilt package has no manifest path — so an executable project
    /// silently got no filesystem cache at all, while a library project of the same shape got one.
    pub fn filesystem_package_cache_dir(&self) -> Option<PathBuf> {
        let input = self.input.as_ref()?;
        if !matches!(input.file_type(), FileType::Toml) {
            return None;
        }
        let project_dir = input.as_path()?.parent()?;
        let project_dir = if project_dir.is_absolute() {
            project_dir.to_path_buf()
        } else {
            self.options.current_dir.join(project_dir)
        };
        // Canonicalized because the loaded manifest path this replaces was: the cache directory
        // is compared by path across nested builds, so `.`-relative and symlinked spellings of
        // one directory must not resolve to two caches.
        #[cfg(feature = "std")]
        let project_dir = project_dir.canonicalize().unwrap_or(project_dir);
        let package_cache_dir = project_dir.join("target").join("miden").join("packages");
        #[cfg(feature = "std")]
        {
            let fingerprint = self.package_cache_fingerprint.get_or_init(|| {
                let inherited_rustflags = std::env::var_os("RUSTFLAGS");
                let inherited_rustup_toolchain = std::env::var_os("RUSTUP_TOOLCHAIN");
                package_cache::fingerprint(
                    &self.options,
                    &project_dir,
                    inherited_rustflags.as_deref(),
                    inherited_rustup_toolchain.as_deref(),
                    MIDENC_BUILD_VERSION,
                    MIDENC_BUILD_REV,
                )
            });
            Some(package_cache_dir.join(fingerprint))
        }
        #[cfg(not(feature = "std"))]
        {
            Some(package_cache_dir)
        }
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

fn is_cargo_project_input(input: &InputFile) -> bool {
    matches!(
        &input.file,
        InputType::Real(path) if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Cargo.toml"))
    )
}

/// What a project's manifest says about the targets it declares.
///
/// The facts are read with `miden_project`'s own extractors — the very ones
/// `miden_project::Package::parse` uses — so that a target's kind, a target's defaulted name and
/// the package's name mean here exactly what they mean to a loaded project. None of them is
/// inheritable from a workspace, which is what makes reading the package manifest alone correct:
/// `[package] name` is a required key of the package's own file, `[lib] kind` defaults to
/// `library` there, and a `[[bin]]` with no name takes the package's.
///
/// # This is the one place the rules live
///
/// Two questions are answered from these facts — [what target type a project builds by
/// default](Self::library_target_type) and [which executable it builds](Self::selected_executable)
/// — and both have to be answered identically everywhere, because the answers decide different
/// halves of one build. `Session::new` uses them to set [`Options::target_type`] and
/// [`Options::entrypoint`]; the Rust frontend's nested `cargo` build uses the second to reject a
/// project it could not build, and used to carry its own copy of both rules over a separately
/// loaded project. Two implementations of one rule can only ever agree by coincidence.
///
/// `read` parses a manifest for them, and [`from_package`](Self::from_package) takes them off a
/// project that is already loaded. That is the whole difference between the callers: where the
/// facts come from, never what is done with them.
pub struct ProjectManifest {
    /// The `[package] name`.
    name: String,
    /// The declared library target, if the manifest declares one.
    library: Option<miden_project::Target>,
    /// The declared executable targets, with names defaulted to the package's.
    executables: alloc::vec::Vec<miden_project::Target>,
}

impl ProjectManifest {
    /// Take these facts off an already-loaded `package`.
    ///
    /// For a caller that has a [`miden_project::Package`] in hand and must not load a second one
    /// — either because it just loaded that one, or because it came from a workspace and was
    /// never a file of its own.
    pub fn from_package(package: &miden_project::Package) -> Self {
        Self {
            name: package.name().to_string(),
            library: package.library_target().map(|lib| lib.inner().clone()),
            executables: package
                .executable_targets()
                .iter()
                .map(|bin| bin.inner().clone())
                .collect(),
        }
    }

    /// The target type a project declaring these targets builds when nothing selects one.
    ///
    /// The library target's kind if there is a library target, and otherwise an executable —
    /// which is what a package declaring only `[[bin]]`s is.
    pub fn library_target_type(&self) -> miden_project::TargetType {
        match self.library.as_ref() {
            Some(library) => library.ty,
            None => miden_project::TargetType::Executable,
        }
    }

    /// The executable target this build compiles, of the ones declared.
    ///
    /// `requested` is `--target`, which names one outright. Without it there must be exactly one
    /// to choose, because nothing else distinguishes them: a package declaring several says which
    /// it means, or is asked to.
    pub fn selected_executable(
        &self,
        requested: Option<&str>,
    ) -> Result<&miden_project::Target, Report> {
        match requested {
            Some(name) => self
                .executables
                .iter()
                .find(|target| name == &**target.name.inner())
                .ok_or_else(|| Report::msg(format!("no executable target name '{name}'"))),
            None if self.executables.len() == 1 => Ok(&self.executables[0]),
            None => Err(Report::msg(
                "ambiguous executable target selection: use --target to select a specific \
                 executable target",
            )),
        }
    }

    /// Read the manifest the project locator `input` names.
    ///
    /// `Ok(None)` means the manifest could not be read or is not a package manifest, which is not
    /// an error here — see [`Session::new`] for why, and for what a session does instead. A
    /// locator piped in on standard input is the one exception: nothing downstream re-reads it, so
    /// there is no better place for its diagnostic than this one.
    fn read(input: &InputFile, source_manager: &dyn SourceManager) -> Result<Option<Self>, Report> {
        match &input.file {
            InputType::Real(path) => {
                // The same normalization `normalize_locator` performs in `midenc-compile`: a
                // `Cargo.toml` locates the `miden-project.toml` beside it, which is where
                // `cargo miden` writes the Miden manifest for a crate.
                let manifest_path =
                    if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Cargo.toml"))
                    {
                        path.with_file_name("miden-project.toml")
                    } else {
                        path.clone()
                    };
                #[cfg(feature = "std")]
                {
                    use miden_debug_types::SourceManagerExt;
                    let Ok(source) = source_manager.load_file(&manifest_path) else {
                        return Ok(None);
                    };
                    Ok(Self::parse(source).ok())
                }
                #[cfg(not(feature = "std"))]
                {
                    let _ = manifest_path;
                    Ok(None)
                }
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
                Self::parse(source_file).map(Some)
            }
        }
    }

    fn parse(source: Arc<diagnostics::SourceFile>) -> Result<Self, Report> {
        let package = match miden_project::ast::MidenProject::parse(source)? {
            miden_project::ast::MidenProject::Package(package) => package,
            // A workspace manifest declares members but no package of its own, so it names
            // nothing to derive an artifact name or a target type from. Which member was meant
            // has to come from the caller, and saying so is the job of whoever resolves the
            // locator; there is nothing for a session to do with one.
            miden_project::ast::MidenProject::Workspace(_) => {
                return Err(Report::msg(
                    "expected a package manifest, but found a workspace manifest",
                ));
            }
        };
        // The spans are dropped: every diagnostic these facts can provoke is raised against the
        // manifest downstream, by whoever loads it, and none of them is raised here.
        use miden_debug_types::Span;
        Ok(Self {
            name: package.package.name.inner().to_string(),
            library: package.extract_library_target()?.map(Span::into_inner),
            executables: package
                .extract_executable_targets()
                .into_iter()
                .map(Span::into_inner)
                .collect(),
        })
    }
}

fn infer_cargo_project_entrypoint(
    manifest: &ProjectManifest,
    options: &mut Options,
) -> Result<(), Report> {
    if options.entrypoint.is_some() {
        return Ok(());
    }

    match options.target_type {
        Some(miden_project::TargetType::Executable) => {
            let target = manifest.selected_executable(options.target.as_deref())?;
            let masm_module_name = target.name.inner().replace('-', "_");
            options.entrypoint = Some(format!("{masm_module_name}::entrypoint"));
        }
        Some(miden_project::TargetType::TransactionScript) => {
            options.entrypoint = Some("miden:base/transaction-script@1.0.0::run".to_string());
        }
        _ => (),
    }

    Ok(())
}

#[cfg(feature = "std")]
fn create_target_dir(path: &Path) {
    if !path.exists() {
        std::fs::create_dir_all(path).unwrap_or_else(|err| {
            panic!("unable to create --target-dir '{}': {err}", path.display())
        });
    }
}

#[cfg(not(feature = "std"))]
fn create_target_dir(_path: &Path) {}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn relative_manifest_locator_uses_the_configured_current_directory() {
        let temp = TempDir::new().unwrap();
        let options = Options {
            current_dir: temp.path().to_path_buf(),
            target_dir: temp.path().join("target"),
            ..Options::default()
        };
        let input = InputFile::new(FileType::Toml, InputType::Real("Cargo.toml".into()));
        let session = Session::new_project(
            "relative-manifest".into(),
            Some(input),
            Box::new(options),
            None,
            Arc::new(diagnostics::DefaultSourceManager::default()),
        );

        let cache_dir = session.filesystem_package_cache_dir().unwrap();
        let expected_parent = temp.path().canonicalize().unwrap().join("target/miden/packages");
        assert_eq!(cache_dir.parent(), Some(expected_parent.as_path()));
    }
}
