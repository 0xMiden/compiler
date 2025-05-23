use std::{ffi::OsString, path::PathBuf, sync::Arc};

use clap::{ColorChoice, Parser};
use midenc_session::{
    add_target_link_libraries,
    diagnostics::{DefaultSourceManager, Emitter},
    ColorChoice as MDColorChoice, InputFile, LinkLibrary, Options, ProjectType, Session, TargetEnv,
};

/// Run a compiled Miden program with the Miden VM
#[derive(Default, Debug, Parser)]
#[command(name = "debug")]
pub struct Debugger {
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
        next_line_help(true),
        help_heading = "Linker"
    )]
    pub link_libraries: Vec<LinkLibrary>,
}

impl Debugger {
    /// Construct a [Compiler] programatically
    pub fn new_session<I, A, S>(inputs: I, emitter: Option<Arc<dyn Emitter>>, argv: A) -> Session
    where
        I: IntoIterator<Item = InputFile>,
        A: IntoIterator<Item = S>,
        S: Into<OsString> + Clone,
    {
        let argv = [OsString::from("run")]
            .into_iter()
            .chain(argv.into_iter().map(|arg| arg.into()));
        let mut matches = <Self as clap::CommandFactory>::command()
            .try_get_matches_from(argv)
            .unwrap_or_else(|err| err.exit());

        let opts = <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(format_error::<Self>)
            .unwrap_or_else(|err| err.exit());

        let inputs = inputs.into_iter().collect();
        opts.into_session(inputs, emitter)
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

        // Map clap color choices to internal color choice
        let color = match self.color {
            ColorChoice::Auto => MDColorChoice::Auto,
            ColorChoice::Always => MDColorChoice::Always,
            ColorChoice::Never => MDColorChoice::Never,
        };

        // Consolidate all compiler options
        let mut options = Options::new(None, self.target, ProjectType::Program, cwd, self.sysroot)
            .with_color(color);
        options.search_paths = self.search_path;

        let link_libraries = add_target_link_libraries(self.link_libraries, &self.target);
        options.link_libraries = link_libraries;

        options.entrypoint = self.entrypoint;

        let target_dir = std::env::temp_dir();
        let source_manager = Arc::new(DefaultSourceManager::default());
        Session::new(inputs, None, None, target_dir, options, emitter, source_manager)
    }
}

fn format_error<I: clap::CommandFactory>(err: clap::Error) -> clap::Error {
    let mut cmd = I::command();
    err.format(&mut cmd)
}
