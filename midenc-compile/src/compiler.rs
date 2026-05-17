#[cfg(feature = "std")]
use alloc::{borrow::ToOwned, format, string::ToString, vec};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
#[cfg(feature = "std")]
use std::ffi::OsString;

#[cfg(feature = "std")]
use clap::{Parser, builder::ArgPredicate};
use miden_mast_package::TargetType;
use midenc_session::{
    ColorChoice, DebugInfo, InputFile, IrFilter, LinkLibrary, OptLevel, Options, OutputFile,
    OutputTypeSpec, OutputTypes, PathBuf, Session, Verbosity, Warnings, add_target_link_libraries,
    diagnostics::Emitter,
};

/// Compile a program from WebAssembly or Miden IR, to Miden Assembly.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(Parser))]
#[cfg_attr(feature = "std", command(name = "midenc"))]
pub struct Compiler {
    /// Write all intermediate compiler artifacts to `<dir>`
    ///
    /// Defaults to a directory named `target/midenc` in the current working directory
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "DIR",
            env = "MIDENC_TARGET_DIR",
            default_value = "target/midenc",
            help_heading = "Output"
        )
    )]
    pub target_dir: PathBuf,
    /// The working directory for the compiler
    ///
    /// By default this will be the working directory the compiler is executed from
    #[cfg_attr(
        feature = "std",
        arg(long, value_name = "DIR", help_heading = "Output")
    )]
    pub working_dir: Option<PathBuf>,
    /// The path to the root directory of the current Miden toolchain.
    ///
    /// This is expected to be set by `midenup`, but may be specified manually if needed.
    ///
    /// The path given should be a directory whose layout matches `$MIDENUP_HOME/toolchains/<version>`
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "DIR",
            env = "MIDEN_SYSROOT",
            help_heading = "Compiler"
        )
    )]
    pub sysroot: Option<PathBuf>,
    /// The path to the `midenup` home directory.
    ///
    /// By default this is assumed to be `$HOME/.miden`, if `$MIDENUP_HOME` is unset
    #[cfg_attr(
        feature = "std",
        arg(long, value_name = "DIR", env = "MIDENUP_HOME", hide = true)
    )]
    pub midenup_home: Option<PathBuf>,
    /// The name of the toolchain to use during compilation, e.g. `0.14.0` or `stable`
    ///
    /// This is read from `$MIDENUP_TOOLCHAIN` by default - if unset, the compiler will assume no
    /// toolchain is availabe.
    #[cfg_attr(
        feature = "std",
        arg(long, value_name = "NAME", env = "MIDEN_SYSROOT", hide = true)
    )]
    pub toolchain: Option<String>,
    /// Write compiled output to compiler-chosen filename in `<dir>`
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            short = 'O',
            value_name = "DIR",
            env = "MIDENC_OUT_DIR",
            help_heading = "Output"
        )
    )]
    pub output_dir: Option<PathBuf>,
    /// Write compiled output to `<filename>`
    #[cfg_attr(
        feature = "std",
        arg(long, short = 'o', value_name = "FILENAME", help_heading = "Output")
    )]
    pub output_file: Option<PathBuf>,
    /// Write output to stdout
    #[cfg_attr(
        feature = "std",
        arg(long, conflicts_with("output_file"), help_heading = "Output")
    )]
    pub stdout: bool,
    /// Specify the name of the project target being compiled
    ///
    /// By default, if this is not specified, then the target is inferred based on the type of
    /// target requested, and the available project context.
    ///
    /// If no project context is available, then the default project name is derived from the
    /// input file, or the base name of the working directory if the input file is read from stdin.
    #[cfg_attr(feature = "std", arg(
        long,
        value_name = "TARGET",
        default_value = None,
        help_heading = "Diagnostics"
    ))]
    pub target: Option<String>,
    /// The target environment to compile for
    #[cfg_attr(feature = "std", arg(
        long,
        value_name = "TYPE",
        help_heading = "Compiler",
        value_parser(TargetTypeValueParser),
        default_value_ifs([
            // When an entrypoint is specified, always set the target type to executable
            ("entrypoint", ArgPredicate::IsPresent, Some("executable")),
            // When --exe is specified, always set the target type to executable
            ("is_program", ArgPredicate::IsPresent, Some("executable")),
            // When --lib is specified, always set the target type to library
            ("is_library", ArgPredicate::IsPresent, Some("library")),
        ]),
    ))]
    pub target_type: Option<TargetType>,
    /// Specify what type and level of informational output to emit
    #[cfg_attr(feature = "std", arg(
        long = "verbose",
        short = 'v',
        value_enum,
        value_name = "LEVEL",
        default_value_t = Verbosity::Info,
        default_missing_value = "debug",
        num_args(0..=1),
        help_heading = "Diagnostics"
    ))]
    pub verbosity: Verbosity,
    /// Specify how warnings should be treated by the compiler.
    #[cfg_attr(feature = "std", arg(
        long,
        short = 'W',
        value_enum,
        value_name = "LEVEL",
        default_value_t = Warnings::All,
        default_missing_value = "all",
        num_args(0..=1),
        help_heading = "Diagnostics"
    ))]
    pub warn: Warnings,
    /// Whether, and how, to color terminal output
    #[cfg_attr(feature = "std", arg(
        long,
        value_enum,
        default_value_t = ColorChoice::Auto,
        default_missing_value = "auto",
        num_args(0..=1),
        help_heading = "Diagnostics"
    ))]
    pub color: ColorChoice,
    /// Specify the function to call as the entrypoint for the program
    /// in the format `<module_name>::<function>`
    #[cfg_attr(feature = "std", arg(long, help_heading = "Compiler", hide(true)))]
    pub entrypoint: Option<String>,
    /// Tells the compiler to produce an executable Miden program
    ///
    /// Implied by `--entrypoint`, defaults to true for non-rollup targets.
    #[cfg_attr(feature = "std", arg(
        long = "exe",
        conflicts_with("target_type"),
        default_value_t = false,
        default_value_ifs([
            // When the executable target is explicit, set this to true
            ("target_type", "executable".into(), Some("true")),
            // Setting the entrypoint implies building an executable in all other cases
            ("entrypoint", ArgPredicate::IsPresent, Some("true")),
        ]),
        help_heading = "Linker"
    ))]
    pub is_program: bool,
    /// Tells the compiler to produce a Miden library
    ///
    /// Defaults to true, unless `--target-type` is a library target
    #[cfg_attr(feature = "std", arg(
        long = "lib",
        conflicts_with("is_program"),
        conflicts_with("entrypoint"),
        conflicts_with("target_type"),
        default_value_t = true,
        default_value_ifs([
            // When an entrypoint is specified, always set the default to false
            ("entrypoint", ArgPredicate::IsPresent, Some("false")),
            // When targeting the rollup, we always build as a library
            ("target_type", "executable".into(), Some("false")),
        ]),
        help_heading = "Linker"
    ))]
    pub is_library: bool,
    /// Specify one or more search paths for link libraries requested via `-l`
    #[cfg_attr(
        feature = "std",
        arg(
            long = "search-path",
            short = 'L',
            value_name = "PATH",
            help_heading = "Linker"
        )
    )]
    pub search_path: Vec<PathBuf>,
    /// Link compiled projects to the specified library NAME.
    ///
    /// The optional KIND can be provided to indicate what type of library it is.
    ///
    /// NAME must either be an absolute path (with extension when applicable), or
    /// a library namespace (no extension). The former will be used as the path
    /// to load the library, without looking for it in the library search paths,
    /// while the latter will be located in the search path based on its KIND.
    ///
    /// See below for valid KINDs:
    #[cfg_attr(
        feature = "std",
        arg(
            long = "link-library",
            short = 'l',
            value_name = "[KIND=]NAME",
            value_delimiter = ',',
            next_line_help(true),
            help_heading = "Linker"
        )
    )]
    pub link_libraries: Vec<LinkLibrary>,
    /// Specify one or more output types for the compiler to emit
    ///
    /// The format for SPEC is `KIND[=PATH]`. You can specify multiple items at
    /// once by separating each SPEC with a comma, you can also pass this flag
    /// multiple times.
    ///
    /// PATH must be a directory in which to place the outputs, or `-` for stdout.
    #[cfg_attr(
        feature = "std",
        arg(
            long = "emit",
            value_name = "SPEC",
            value_delimiter = ',',
            env = "MIDENC_EMIT",
            next_line_help(true),
            help_heading = "Output"
        )
    )]
    pub output_types: Vec<OutputTypeSpec>,
    /// Specify what level of debug information to emit in compilation artifacts
    #[cfg_attr(feature = "std", arg(
        long,
        value_enum,
        value_name = "LEVEL",
        next_line_help(true),
        default_value_t = DebugInfo::Full,
        default_missing_value = "full",
        num_args(0..=1),
        help_heading = "Output"
    ))]
    pub debug: DebugInfo,
    /// Specify what type, and to what degree, of optimizations to apply to code during
    /// compilation.
    #[cfg_attr(feature = "std", arg(
        long = "optimize",
        value_enum,
        value_name = "LEVEL",
        next_line_help(true),
        default_value_t = OptLevel::None,
        default_missing_value = "balanced",
        num_args(0..=1),
        help_heading = "Output"
    ))]
    pub opt_level: OptLevel,
    /// Set a codegen option
    ///
    /// Use `-C help` to print available options
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            short = 'C',
            value_name = "OPT[=VALUE]",
            help_heading = "Compiler"
        )
    )]
    pub codegen: Vec<String>,
    /// Set an unstable compiler option
    ///
    /// Use `-Z help` to print available options
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            short = 'Z',
            value_name = "OPT[=VALUE]",
            help_heading = "Compiler"
        )
    )]
    pub unstable: Vec<String>,
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "NAME",
            help_heading = "Compiler",
            default_value = "dev",
            default_value_ifs([
                // When an entrypoint is specified, always set the target type to executable
                ("release", ArgPredicate::IsPresent, Some("release")),
            ]),
        )
    )]
    pub profile: String,
    /// Build in release mode (used by cargo miden build)
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            help_heading = "Compiler",
            conflicts_with("profile"),
            default_value_t = false,
            default_value_ifs([
                ("profile", ArgPredicate::from("release"), Some("true")),
            ])
        )
    )]
    pub release: bool,
    /// Build all packages in the workspace
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            help_heading = "Compiler",
            conflicts_with("package"),
            default_value_t = false,
        )
    )]
    pub workspace: bool,
    /// Package(s) to build
    #[cfg_attr(
        feature = "std",
        arg(long, short = 'p', value_name = "SPEC", conflicts_with("workspace"),)
    )]
    pub package: Vec<String>,
    /// Path to the package/project manifest
    ///
    /// If unspecified, the compiler will create a virtual manifest for the given input
    #[cfg_attr(feature = "std", arg(long, value_name = "PATH",))]
    pub manifest_path: Option<PathBuf>,
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "std", derive(Parser))]
#[cfg_attr(feature = "std", command(name = "-C"))]
pub struct CodegenOptions {
    /// Tell the compiler to exit after it has parsed the inputs
    #[cfg_attr(feature = "std", arg(
        long,
        conflicts_with_all(["analyze_only", "link_only"]),
        default_value_t = false,
    ))]
    pub parse_only: bool,
    /// Tell the compiler to exit after it has performed semantic analysis on the inputs
    #[cfg_attr(feature = "std", arg(
        long,
        conflicts_with_all(["parse_only", "link_only"]),
        default_value_t = false,
    ))]
    pub analyze_only: bool,
    /// Tell the compiler to exit after linking the inputs, without generating Miden Assembly
    #[cfg_attr(feature = "std", arg(
        long,
        conflicts_with_all(["no_link"]),
        default_value_t = false,
    ))]
    pub link_only: bool,
    /// Tell the compiler to generate Miden Assembly from the inputs without linking them
    #[cfg_attr(feature = "std", arg(long, default_value_t = false))]
    pub no_link: bool,
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "std", derive(Parser))]
#[cfg_attr(feature = "std", command(name = "-Z"))]
pub struct UnstableOptions {
    /// Run the experimental Miden Assembly linter prior to assembly
    #[cfg_attr(
        feature = "std",
        arg(long, default_value_t = false, help_heading = "Analysis")
    )]
    pub lint: bool,
    /// Print the CFG after each HIR pass is applied
    #[cfg_attr(
        feature = "std",
        arg(long, default_value_t = false, help_heading = "Passes")
    )]
    pub print_cfg_after_all: bool,
    /// Print the CFG after running a specific HIR pass
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "PASS",
            value_delimiter = ',',
            help_heading = "Passes"
        )
    )]
    pub print_cfg_after_pass: Vec<String>,
    /// Print the IR before each compiler stage.
    ///
    /// The available stages are:
    ///
    /// * `link`     - performs initial translation to HIR, and links dependencies into the graph
    ///
    /// * `rewrite`  - performs rewrites/optimizations on the initial unmodified HIR
    ///
    /// * `codegen`  - performs lowering from HIR to Miden Assembly
    ///
    /// * `assemble` - performs assembly of a program or library package
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "STAGE",
            value_delimiter = ',',
            next_line_help(true),
            help_heading = "Passes"
        )
    )]
    pub print_ir_before_stage: Vec<String>,
    /// Print the IR after each pass is applied
    #[cfg_attr(
        feature = "std",
        arg(long, default_value_t = false, help_heading = "Passes")
    )]
    pub print_ir_after_all: bool,
    /// Print the IR after running a specific pass
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            value_name = "PASS",
            value_delimiter = ',',
            help_heading = "Passes"
        )
    )]
    pub print_ir_after_pass: Vec<String>,
    /// Only print the IR if the pass modified the IR structure. If this flag is set, and no IR
    /// filter flag is; then the default behavior is to print the IR after every pass.
    #[cfg_attr(
        feature = "std",
        arg(long, default_value_t = false, help_heading = "Passes")
    )]
    pub print_ir_after_modified: bool,
    /// Only print IR that matches the given filter.
    ///
    /// The syntax for filters are as follows:
    ///
    /// * `any` (default)         - matches any operation
    ///
    /// * `symbol:*`              - matches any symbol
    ///
    /// * `symbol:<pattern>`      - matches any symbol whose name contains <pattern>
    ///
    /// * `op:<dialect>.<opcode>` - matches any instance of the given operation name
    #[cfg_attr(
        feature = "std",
        arg(
            long,
            action = clap::ArgAction::Append,
            value_name = "FILTER",
            value_delimiter = ',',
            next_line_help(true),
            help_heading = "Passes"
        )
    )]
    pub print_ir_filter: Vec<IrFilter>,
    /// Print source location information in HIR output
    ///
    /// When enabled, HIR output will include #loc() annotations showing the source file,
    /// line, and column for each operation.
    #[cfg_attr(
        feature = "std",
        arg(
            long = "print-hir-source-locations",
            default_value_t = false,
            help_heading = "Printers"
        )
    )]
    pub print_hir_source_locations: bool,
    /// Specify path prefixes to try when resolving relative paths from DWARF debug info
    #[cfg_attr(
        feature = "std",
        arg(
            long = "trim-path-prefix",
            value_name = "PATH",
            help_heading = "Debugging"
        )
    )]
    pub trim_path_prefixes: Vec<PathBuf>,
}

impl CodegenOptions {
    #[cfg(feature = "std")]
    fn parse_argv(argv: Vec<String>) -> Self {
        let command = <CodegenOptions as clap::CommandFactory>::command()
            .no_binary_name(true)
            .arg_required_else_help(false)
            .help_template(
                "\
Available codegen options:

Usage: midenc -C <opt>

{all-args}{after-help}

NOTE: When specifying these options, strip the leading '--'",
            );

        let argv = if argv.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h" | "help")) {
            vec!["--help".to_string()]
        } else {
            argv.into_iter()
                .flat_map(|arg| match arg.split_once('=') {
                    None => vec![format!("--{arg}")],
                    Some((opt, value)) => {
                        vec![format!("--{opt}"), value.to_string()]
                    }
                })
                .collect::<Vec<_>>()
        };

        let mut matches = command.try_get_matches_from(argv).unwrap_or_else(|err| err.exit());
        <CodegenOptions as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<CodegenOptions>)
            .unwrap_or_else(|err| err.exit())
    }

    #[cfg(not(feature = "std"))]
    fn parse_argv(_argv: Vec<String>) -> Self {
        Self::default()
    }
}

impl UnstableOptions {
    #[cfg(feature = "std")]
    fn parse_argv(argv: Vec<String>) -> Self {
        let command = <UnstableOptions as clap::CommandFactory>::command()
            .no_binary_name(true)
            .arg_required_else_help(false)
            .help_template(
                "\
Available unstable options:

Usage: midenc -Z <opt>

{all-args}{after-help}

NOTE: When specifying these options, strip the leading '--'",
            );

        let argv = if argv.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h" | "help")) {
            vec!["--help".to_string()]
        } else {
            argv.into_iter()
                .flat_map(|arg| match arg.split_once('=') {
                    None => vec![format!("--{arg}")],
                    Some((opt, value)) => {
                        vec![format!("--{opt}"), value.to_string()]
                    }
                })
                .collect::<Vec<_>>()
        };

        let mut matches = command.try_get_matches_from(argv).unwrap_or_else(|err| err.exit());
        <UnstableOptions as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<UnstableOptions>)
            .unwrap_or_else(|err| err.exit())
    }

    #[cfg(not(feature = "std"))]
    fn parse_argv(_argv: Vec<String>) -> Self {
        Self::default()
    }
}

impl Compiler {
    /// Parse command-line arguments into compiler [Options].
    ///
    /// Returns the parsed options or an error if parsing failed.
    ///
    /// This is used by `cargo miden build` to parse all arguments into `Compiler` options before
    /// selectively forwarding them to `cargo build` and `midenc`.
    #[cfg(feature = "std")]
    pub fn try_parse_from<I, T>(cwd: PathBuf, iter: I) -> Result<Box<Options>, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv = [OsString::from("midenc")]
            .into_iter()
            .chain(iter.into_iter().map(|arg| arg.into()));
        let command = <Self as clap::CommandFactory>::command();
        let command = midenc_session::flags::register_flags(command);
        let mut matches = command.try_get_matches_from(argv)?;
        let compile_matches = matches.clone();

        let opts = <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<Self>)?;

        let mut opts = opts.into_options(cwd);
        opts.set_extra_flags(compile_matches.into());
        Ok(opts)
    }

    /// Construct a [Compiler] programatically
    #[cfg(feature = "std")]
    pub fn new_session<A, S>(
        cwd: PathBuf,
        input: Option<InputFile>,
        emitter: Option<Arc<dyn Emitter>>,
        argv: A,
    ) -> Session
    where
        A: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        let opts = Self::try_parse_from(cwd, argv).unwrap_or_else(|err| err.exit());
        Self::into_session(opts, input, emitter)
    }

    pub fn into_options(self, cwd: PathBuf) -> Box<Options> {
        let Self {
            target_dir,
            working_dir,
            sysroot,
            midenup_home,
            toolchain,
            output_dir,
            output_file,
            stdout,
            target,
            target_type,
            verbosity,
            warn,
            color,
            entrypoint,
            is_program: _,
            is_library: _,
            search_path,
            mut link_libraries,
            output_types,
            debug,
            opt_level,
            codegen,
            unstable,
            profile,
            release: _,
            workspace,
            package,
            manifest_path,
        } = self;
        let codegen = CodegenOptions::parse_argv(codegen);
        let unstable = UnstableOptions::parse_argv(unstable);

        // Determine if a specific output file has been requested
        let output_file = match output_file {
            Some(path) => Some(OutputFile::Real(path)),
            None if stdout => Some(OutputFile::Stdout),
            None => None,
        };

        // Initialize output types
        let output_types = OutputTypes::new(output_types).unwrap_or_else(|err| err.exit());

        let cwd = working_dir.unwrap_or(cwd);

        // Establish --target-dir
        let target_dir = if target_dir.is_absolute() {
            target_dir
        } else {
            cwd.join(&target_dir)
        };

        // Consolidate all compiler options
        let mut options = Box::new(Options::new(
            target.clone(),
            target_type,
            cwd,
            target_dir,
            output_dir,
            sysroot,
        ))
        .with_color(color)
        .with_verbosity(verbosity)
        .with_warnings(warn)
        .with_debug_info(debug)
        .with_optimization(opt_level)
        .with_output_types(output_types, output_file);
        options.target = target;
        options.profile = profile;
        options.manifest_path = manifest_path;
        options.midenup_home = midenup_home;
        options.toolchain = toolchain;
        options.search_paths.extend(search_path);
        add_target_link_libraries(&mut link_libraries, options.target_requires_protocol());
        options.link_libraries = link_libraries;
        options.entrypoint = entrypoint;
        options.workspace = workspace;
        options.packages = package;
        options.parse_only = codegen.parse_only;
        options.analyze_only = codegen.analyze_only;
        options.link_only = codegen.link_only;
        options.no_link = codegen.no_link;
        options.lint = unstable.lint;
        options.print_cfg_after_all = unstable.print_cfg_after_all;
        options.print_cfg_after_pass = unstable.print_cfg_after_pass;
        options.print_ir_after_all = unstable.print_ir_after_all;
        options.print_ir_after_pass = unstable.print_ir_after_pass;
        options.print_ir_after_modified = unstable.print_ir_after_modified;
        options.print_ir_filters = unstable.print_ir_filter;
        options.print_hir_source_locations = unstable.print_hir_source_locations;
        options.trim_path_prefixes = unstable.trim_path_prefixes;

        options
    }

    /// Use this configuration to obtain a [Session] used for compilation
    fn into_session(
        options: Box<Options>,
        input: Option<InputFile>,
        emitter: Option<Arc<dyn Emitter>>,
    ) -> Session {
        // Raise an error if no inputs were provided
        let Some(input) = input else {
            let cmd = <Compiler as clap::CommandFactory>::command();
            let mut err =
                clap::Error::new(clap::error::ErrorKind::MissingRequiredArgument).with_cmd(&cmd);
            err.insert(
                clap::error::ContextKind::InvalidArg,
                clap::error::ContextValue::String("INPUT".to_string()),
            );
            err.exit();
        };

        log::trace!(target: "driver", "current working directory = {}", options.current_dir.display());

        match options.into_session(input, emitter, None) {
            Ok(session) => session,
            Err(err) => {
                let cmd = <Compiler as clap::CommandFactory>::command();
                let err =
                    clap::Error::raw(clap::error::ErrorKind::ValueValidation, err).with_cmd(&cmd);
                err.exit();
            }
        }
    }
}

#[cfg(feature = "std")]
fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}

#[derive(Clone)]
struct TargetTypeValueParser;

impl clap::builder::TypedValueParser for TargetTypeValueParser {
    type Value = TargetType;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_string_lossy();
        value.parse::<TargetType>().map_err(|err| {
            let mut err = clap::Error::raw(clap::error::ErrorKind::ValueValidation, err);
            if let Some(arg) = arg {
                err.insert(
                    clap::error::ContextKind::InvalidArg,
                    clap::error::ContextValue::String(arg.to_string()),
                );
            }
            err
        })
    }
}
