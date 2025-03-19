mod arg_matches;
mod flag;

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

pub use self::{
    arg_matches::ArgMatches,
    flag::{CompileFlag, FlagAction},
};
use crate::diagnostics::Report;

pub struct CompileFlags {
    flags: Vec<CompileFlag>,
    arg_matches: ArgMatches,
}

#[cfg(feature = "std")]
impl Default for CompileFlags {
    fn default() -> Self {
        Self::new(None::<std::ffi::OsString>).unwrap()
    }
}

#[cfg(not(feature = "std"))]
impl Default for CompileFlags {
    fn default() -> Self {
        Self::new(None::<alloc::string::String>).unwrap()
    }
}

impl From<ArgMatches> for CompileFlags {
    fn from(arg_matches: ArgMatches) -> Self {
        let flags = inventory::iter::<CompileFlag>.into_iter().cloned().collect();
        Self { flags, arg_matches }
    }
}

impl CompileFlags {
    /// Create a new [CompileFlags] from the given argument vector
    #[cfg(feature = "std")]
    pub fn new<I, V>(argv: I) -> Result<Self, Report>
    where
        I: IntoIterator<Item = V>,
        V: Into<std::ffi::OsString> + Clone,
    {
        use crate::diagnostics::IntoDiagnostic;

        let flags = inventory::iter::<CompileFlag>.into_iter().cloned().collect();
        fake_compile_command()
            .try_get_matches_from(argv)
            .into_diagnostic()
            .map(|arg_matches| Self { flags, arg_matches })
    }

    /// Get [clap::ArgMatches] for registered command-line flags, without a [clap::Command]
    #[cfg(not(feature = "std"))]
    pub fn new<I, V>(argv: I) -> Result<Self, Report>
    where
        I: IntoIterator<Item = V>,
        V: Into<Cow<'static, str>> + Clone,
    {
        use alloc::collections::{BTreeMap, VecDeque};

        let argv = argv.into_iter().map(|arg| arg.into()).collect::<VecDeque<_>>();
        let flags = inventory::iter::<CompileFlag>
            .into_iter()
            .map(|flag| (flag.name, flag))
            .collect::<BTreeMap<_, _>>();

        let arg_matches = ArgMatches::parse(argv, &flags)?;
        let this = Self {
            flags: flags.values().copied().cloned().collect(),
            arg_matches,
        };

        Ok(this)
    }

    pub fn flags(&self) -> &[CompileFlag] {
        self.flags.as_slice()
    }

    /// Get the value of a custom flag with action `FlagAction::SetTrue` or `FlagAction::SetFalse`
    pub fn get_flag(&self, name: &str) -> bool {
        self.arg_matches.get_flag(name)
    }

    /// Get the count of a specific custom flag with action `FlagAction::Count`
    pub fn get_flag_count(&self, name: &str) -> usize {
        self.arg_matches.get_count(name) as usize
    }

    /// Get the remaining [ArgMatches] left after parsing the base session configuration
    pub fn matches(&self) -> &ArgMatches {
        &self.arg_matches
    }
}

impl fmt::Debug for CompileFlags {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for id in self.arg_matches.ids() {
            use clap::parser::ValueSource;
            // Don't print CompilerOptions arg group
            if id.as_str() == "CompilerOptions" {
                continue;
            }
            // Don't print default values
            if matches!(self.arg_matches.value_source(id.as_str()), Some(ValueSource::DefaultValue))
            {
                continue;
            }
            map.key(&id.as_str()).value_with(|f| {
                let mut list = f.debug_list();
                if let Some(occurs) =
                    self.arg_matches.try_get_raw_occurrences(id.as_str()).expect("expected flag")
                {
                    list.entries(occurs.flatten());
                }
                list.finish()
            });
        }
        map.finish()
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (name, raw_values) in self.arg_matches.iter() {
            map.key(&name)
                .value_with(|f| f.debug_list().entries(raw_values.iter()).finish());
        }
        map.finish()
    }
}

/// Generate a fake compile command for use with default options
#[cfg(feature = "std")]
fn fake_compile_command() -> clap::Command {
    let cmd = clap::Command::new("compile")
        .no_binary_name(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true);
    register_flags(cmd)
}

/// Register dynamic flags to be shown via `midenc help compile`
#[cfg(feature = "std")]
pub fn register_flags(cmd: clap::Command) -> clap::Command {
    inventory::iter::<CompileFlag>.into_iter().fold(cmd, |cmd, flag| {
        let arg = clap::Arg::new(flag.name)
            .long(flag.long.unwrap_or(flag.name))
            .action(clap::ArgAction::from(flag.action));
        let arg = if let Some(help) = flag.help {
            arg.help(help)
        } else {
            arg
        };
        let arg = if let Some(help_heading) = flag.help_heading {
            arg.help_heading(help_heading)
        } else {
            arg
        };
        let arg = if let Some(short) = flag.short {
            arg.short(short)
        } else {
            arg
        };
        let arg = if let Some(env) = flag.env {
            arg.env(env)
        } else {
            arg
        };
        let arg = if let Some(value) = flag.default_missing_value {
            arg.default_missing_value(value)
        } else {
            arg
        };
        let arg = if let Some(value) = flag.default_value {
            arg.default_value(value)
        } else {
            arg
        };
        let arg = if let Some(value) = flag.hide {
            arg.hide(value)
        } else {
            arg
        };
        cmd.arg(arg)
    })
}
