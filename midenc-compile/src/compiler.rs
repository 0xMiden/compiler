use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::{ffi::OsString, path::PathBuf};

use clap::{builder::ArgPredicate, Parser};
use midenc_session::{
    diagnostics::{DefaultSourceManager, Emitter},
    ColorChoice, DebugInfo, InputFile, LinkLibrary, OptLevel, Options, OutputFile, OutputType,
    OutputTypeSpec, OutputTypes, ProjectType, Session, TargetEnv, Verbosity, Warnings,
};

/// Compile a program from WebAssembly or Miden IR, to Miden Assembly.
#[derive(Debug, Parser)]
#[command(name = "midenc")]
pub struct Compiler {
    /// Write all intermediate compiler artifacts to `<dir>`
    ///
    /// Defaults to a directory named `target/midenc` in the current working directory
    #[arg(
        long,
        value_name = "DIR",
        env = "MIDENC_TARGET_DIR",
        default_value = "target/midenc",
        help_heading = "Output"
    )]
    pub target_dir: PathBuf,
    /// The working directory for the compiler
    ///
    /// By default this will be the working directory the compiler is executed from
    #[arg(long, value_name = "DIR", help_heading = "Output")]
    pub working_dir: Option<PathBuf>,
    /// The path to the root directory of the Miden toolchain libraries
    ///
    /// By default this is assumed to be ~/.miden/toolchains/<version>
    #[arg(
        long,
        value_name = "DIR",
        env = "MIDENC_SYSROOT",
        help_heading = "Compiler"
    )]
    pub sysroot: Option<PathBuf>,
    /// Write compiled output to compiler-chosen filename in `<dir>`
    #[arg(
        long,
        short = 'O',
        value_name = "DIR",
        env = "MIDENC_OUT_DIR",
        help_heading = "Output"
    )]
    pub output_dir: Option<PathBuf>,
    /// Write compiled output to `<filename>`
    #[arg(long, short = 'o', value_name = "FILENAME", help_heading = "Output")]
    pub output_file: Option<PathBuf>,
    /// Write output to stdout
    #[arg(long, conflicts_with("output_file"), help_heading = "Output")]
    pub stdout: bool,
    /// Specify the name of the project being compiled
    ///
    /// The default is derived from the name of the first input file, or if reading from stdin,
    /// the base name of the working directory.
    #[arg(
        long,
        short = 'n',
        value_name = "NAME",
        default_value = None,
        help_heading = "Diagnostics"
    )]
    pub name: Option<String>,
    /// Specify what type and level of informational output to emit
    #[arg(
        long = "verbose",
        short = 'v',
        value_enum,
        value_name = "LEVEL",
        default_value_t = Verbosity::Info,
        default_missing_value = "debug",
        num_args(0..=1),
        help_heading = "Diagnostics"
    )]
    pub verbosity: Verbosity,
    /// Specify how warnings should be treated by the compiler.
    #[arg(
        long,
        short = 'W',
        value_enum,
        value_name = "LEVEL",
        default_value_t = Warnings::All,
        default_missing_value = "all",
        num_args(0..=1),
        help_heading = "Diagnostics"
    )]
    pub warn: Warnings,
    /// Whether, and how, to color terminal output
    #[arg(
        long,
        value_enum,
        default_value_t = ColorChoice::Auto,
        default_missing_value = "auto",
        num_args(0..=1),
        help_heading = "Diagnostics"
    )]
    pub color: ColorChoice,
    /// The target environment to compile for
    #[arg(
        long,
        value_name = "TARGET",
        default_value_t = TargetEnv::Base,
        help_heading = "Compiler"
    )]
    pub target: TargetEnv,
    /// Specify the function to call as the entrypoint for the program
    #[arg(long, help_heading = "Compiler", hide(true))]
    pub entrypoint: Option<String>,
    /// Tells the compiler to produce an executable Miden program
    ///
    /// Implied by `--entrypoint`, defaults to true for non-rollup targets.
    #[arg(
        long = "exe",
        default_value_t = true,
        default_value_ifs([
            // When targeting the rollup, never build an executable
            ("target", "rollup".into(), Some("false")),
            // Setting the entrypoint implies building an executable in all other cases
            ("entrypoint", ArgPredicate::IsPresent, Some("true")),
        ]),
        help_heading = "Linker"
    )]
    pub is_program: bool,
    /// Tells the compiler to produce a Miden library
    ///
    /// Implied by `--target rollup`, defaults to false.
    #[arg(
        long = "lib",
        conflicts_with("is_program"),
        conflicts_with("entrypoint"),
        default_value_t = false,
        default_value_ifs([
            // When an entrypoint is specified, always set the default to false
            ("entrypoint", ArgPredicate::IsPresent, Some("false")),
            // When targeting the rollup, we always build as a library
            ("target", "rollup".into(), Some("true")),
        ]),
        help_heading = "Linker"
    )]
    pub is_library: bool,
    /// Specify one or more search paths for link libraries requested via `-l`
    #[arg(
        long = "search-path",
        short = 'L',
        value_name = "PATH",
        help_heading = "Linker"
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
    #[arg(
        long = "link-library",
        short = 'l',
        value_name = "[KIND=]NAME",
        value_delimiter = ',',
        default_value_ifs([
            ("target", "base", "std"),
            ("target", "rollup", "std,base"),
        ]),
        next_line_help(true),
        help_heading = "Linker"
    )]
    pub link_libraries: Vec<LinkLibrary>,
    /// Specify one or more output types for the compiler to emit
    ///
    /// The format for SPEC is `KIND[=PATH]`. You can specify multiple items at
    /// once by separating each SPEC with a comma, you can also pass this flag
    /// multiple times.
    ///
    /// PATH must be a directory in which to place the outputs, or `-` for stdout.
    #[arg(
        long = "emit",
        value_name = "SPEC",
        value_delimiter = ',',
        next_line_help(true),
        help_heading = "Output"
    )]
    pub output_types: Vec<OutputTypeSpec>,
    /// Specify what level of debug information to emit in compilation artifacts
    #[arg(
        long,
        value_enum,
        value_name = "LEVEL",
        next_line_help(true),
        default_value_t = DebugInfo::Full,
        default_missing_value = "full",
        num_args(0..=1),
        help_heading = "Output"
    )]
    pub debug: DebugInfo,
    /// Specify what type, and to what degree, of optimizations to apply to code during
    /// compilation.
    #[arg(
        long = "optimize",
        value_enum,
        value_name = "LEVEL",
        next_line_help(true),
        default_value_t = OptLevel::None,
        default_missing_value = "balanced",
        num_args(0..=1),
        help_heading = "Output"
    )]
    pub opt_level: OptLevel,
    /// Set a codegen option
    ///
    /// Use `-C help` to print available options
    #[arg(
        long,
        short = 'C',
        value_name = "OPT[=VALUE]",
        help_heading = "Compiler"
    )]
    pub codegen: Vec<String>,
    /// Set an unstable compiler option
    ///
    /// Use `-Z help` to print available options
    #[arg(
        long,
        short = 'Z',
        value_name = "OPT[=VALUE]",
        help_heading = "Compiler"
    )]
    pub unstable: Vec<String>,
}

#[derive(Debug, Clone, Parser)]
#[command(name = "-C")]
pub struct CodegenOptions {
    /// Tell the compiler to exit after it has parsed the inputs
    #[arg(
        long,
        conflicts_with_all(["analyze_only", "link_only"]),
        default_value_t = false,
    )]
    pub parse_only: bool,
    /// Tell the compiler to exit after it has performed semantic analysis on the inputs
    #[arg(
        long,
        conflicts_with_all(["parse_only", "link_only"]),
        default_value_t = false,
    )]
    pub analyze_only: bool,
    /// Tell the compiler to exit after linking the inputs, without generating Miden Assembly
    #[arg(
        long,
        conflicts_with_all(["no_link"]),
        default_value_t = false,
    )]
    pub link_only: bool,
    /// Tell the compiler to generate Miden Assembly from the inputs without linking them
    #[arg(long, default_value_t = false)]
    pub no_link: bool,
}

#[derive(Debug, Clone, Parser)]
#[command(name = "-Z")]
pub struct UnstableOptions {
    /// Print the CFG after each HIR pass is applied
    #[arg(long, default_value_t = false, help_heading = "Passes")]
    pub print_cfg_after_all: bool,
    /// Print the CFG after running a specific HIR pass
    #[arg(
        long,
        value_name = "PASS",
        value_delimiter = ',',
        help_heading = "Passes"
    )]
    pub print_cfg_after_pass: Vec<String>,
    /// Print the IR after each pass is applied
    #[arg(long, default_value_t = false, help_heading = "Passes")]
    pub print_ir_after_all: bool,
    /// Print the IR after running a specific pass
    #[arg(
        long,
        value_name = "PASS",
        value_delimiter = ',',
        help_heading = "Passes"
    )]
    pub print_ir_after_pass: Vec<String>,
}

impl CodegenOptions {
    fn parse_argv(argv: Vec<String>) -> Self {
        let command = <CodegenOptions as clap::CommandFactory>::command()
            .no_binary_name(true)
            .arg_required_else_help(false)
            .help_template(
                "\
Available codegen options:

Usage: midenc compile -C <opt>

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
}

impl UnstableOptions {
    fn parse_argv(argv: Vec<String>) -> Self {
        let command = <UnstableOptions as clap::CommandFactory>::command()
            .no_binary_name(true)
            .arg_required_else_help(false)
            .help_template(
                "\
Available unstable options:

Usage: midenc compile -Z <opt>

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
}

impl Compiler {
    /// Construct a [Compiler] programatically
    pub fn new_session<I, A, S>(inputs: I, emitter: Option<Arc<dyn Emitter>>, argv: A) -> Session
    where
        I: IntoIterator<Item = InputFile>,
        A: IntoIterator<Item = S>,
        S: Into<OsString> + Clone,
    {
        let argv = [OsString::from("midenc")]
            .into_iter()
            .chain(argv.into_iter().map(|arg| arg.into()));
        let command = <Self as clap::CommandFactory>::command();
        let command = midenc_session::flags::register_flags(command);
        let mut matches = command.try_get_matches_from(argv).unwrap_or_else(|err| err.exit());
        let compile_matches = matches.clone();

        let opts = <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<Self>)
            .unwrap_or_else(|err| err.exit());

        let inputs = inputs.into_iter().collect();
        opts.into_session(inputs, emitter).with_extra_flags(compile_matches.into())
    }

    /// Use this configuration to obtain a [Session] used for compilation
    pub fn into_session(
        self,
        inputs: Vec<InputFile>,
        emitter: Option<Arc<dyn Emitter>>,
    ) -> Session {
        let cwd = self
            .working_dir
            .unwrap_or_else(|| std::env::current_dir().expect("no working directory available"));

        // Determine if a specific output file has been requested
        let output_file = match self.output_file {
            Some(path) => Some(OutputFile::Real(path)),
            None if self.stdout => Some(OutputFile::Stdout),
            None => None,
        };

        // Initialize output types
        let mut output_types = OutputTypes::new(self.output_types).unwrap_or_else(|err| err.exit());
        if output_types.is_empty() {
            output_types.insert(OutputType::Masp, output_file.clone());
        } else if output_file.is_some() && output_types.get(&OutputType::Masp).is_some() {
            // The -o flag overrides --emit
            output_types.insert(OutputType::Masp, output_file.clone());
        }

        // Convert --exe or --lib to project type
        let project_type = if self.is_program {
            ProjectType::Program
        } else {
            ProjectType::Library
        };

        let codegen = CodegenOptions::parse_argv(self.codegen);
        let unstable = UnstableOptions::parse_argv(self.unstable);

        // Consolidate all compiler options
        let mut options = Options::new(self.name, self.target, project_type, cwd, self.sysroot)
            .with_color(self.color)
            .with_verbosity(self.verbosity)
            .with_warnings(self.warn)
            .with_debug_info(self.debug)
            .with_optimization(self.opt_level)
            .with_output_types(output_types);
        options.search_paths = self.search_path;
        options.link_libraries = self.link_libraries;
        options.entrypoint = self.entrypoint;
        options.parse_only = codegen.parse_only;
        options.analyze_only = codegen.analyze_only;
        options.link_only = codegen.link_only;
        options.no_link = codegen.no_link;
        options.print_cfg_after_all = unstable.print_cfg_after_all;
        options.print_cfg_after_pass = unstable.print_cfg_after_pass;
        options.print_ir_after_all = unstable.print_ir_after_all;
        options.print_ir_after_pass = unstable.print_ir_after_pass;

        // Establish --target-dir
        let target_dir = if self.target_dir.is_absolute() {
            self.target_dir
        } else {
            options.current_dir.join(&self.target_dir)
        };
        std::fs::create_dir_all(&target_dir).unwrap_or_else(|err| {
            panic!("unable to create --target-dir '{}': {err}", target_dir.display())
        });

        let source_manager = Arc::new(DefaultSourceManager::default());
        Session::new(
            inputs,
            self.output_dir,
            output_file,
            target_dir,
            options,
            emitter,
            source_manager,
        )
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}
