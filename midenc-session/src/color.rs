use alloc::string::ToString;
use core::str::FromStr;

/// ColorChoice represents the color preferences of an end user.
///
/// The `Default` implementation for this type will select `Auto`, which tries
/// to do the right thing based on the current environment.
///
/// The `FromStr` implementation for this type converts a lowercase kebab-case
/// string of the variant name to the corresponding variant. Any other string
/// results in an error.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(clap::ValueEnum))]
pub enum ColorChoice {
    /// Try very hard to emit colors. This includes emitting ANSI colors
    /// on Windows if the console API is unavailable.
    Always,
    /// AlwaysAnsi is like Always, except it never tries to use anything other
    /// than emitting ANSI color codes.
    AlwaysAnsi,
    /// Try to use colors, but don't force the issue. If the console isn't
    /// available on Windows, or if TERM=dumb, or if `NO_COLOR` is defined, for
    /// example, then don't use colors.
    #[default]
    Auto,
    /// Never emit colors.
    Never,
}

#[cfg(feature = "std")]
impl From<ColorChoice> for termcolor::ColorChoice {
    fn from(choice: ColorChoice) -> Self {
        match choice {
            ColorChoice::Always => Self::Always,
            ColorChoice::AlwaysAnsi => Self::AlwaysAnsi,
            ColorChoice::Auto => Self::Auto,
            ColorChoice::Never => Self::Never,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid color choice: {0}")]
pub struct ColorChoiceParseError(alloc::borrow::Cow<'static, str>);

impl FromStr for ColorChoice {
    type Err = ColorChoiceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "always" => Ok(ColorChoice::Always),
            "always-ansi" => Ok(ColorChoice::AlwaysAnsi),
            "never" => Ok(ColorChoice::Never),
            "auto" => Ok(ColorChoice::Auto),
            unknown => Err(ColorChoiceParseError(unknown.to_string().into())),
        }
    }
}

impl ColorChoice {
    /// Returns true if we should attempt to write colored output.
    pub fn should_attempt_color(&self) -> bool {
        match *self {
            ColorChoice::Always => true,
            ColorChoice::AlwaysAnsi => true,
            ColorChoice::Never => false,
            #[cfg(feature = "std")]
            ColorChoice::Auto => self.env_allows_color(),
            #[cfg(not(feature = "std"))]
            ColorChoice::Auto => false,
        }
    }

    #[cfg(all(feature = "std", not(windows)))]
    pub fn env_allows_color(&self) -> bool {
        match std::env::var_os("TERM") {
            // If TERM isn't set, then we are in a weird environment that
            // probably doesn't support colors.
            None => return false,
            Some(k) => {
                if k == "dumb" {
                    return false;
                }
            }
        }
        // If TERM != dumb, then the only way we don't allow colors at this
        // point is if NO_COLOR is set.
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        true
    }

    #[cfg(all(feature = "std", windows))]
    pub fn env_allows_color(&self) -> bool {
        // On Windows, if TERM isn't set, then we shouldn't automatically
        // assume that colors aren't allowed. This is unlike Unix environments
        // where TERM is more rigorously set.
        if let Some(k) = std::env::var_os("TERM") {
            if k == "dumb" {
                return false;
            }
        }
        // If TERM != dumb, then the only way we don't allow colors at this
        // point is if NO_COLOR is set.
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        true
    }

    /// Returns true if this choice should forcefully use ANSI color codes.
    ///
    /// It's possible that ANSI is still the correct choice even if this
    /// returns false.
    #[cfg(all(feature = "std", windows))]
    pub fn should_ansi(&self) -> bool {
        match *self {
            ColorChoice::Always => false,
            ColorChoice::AlwaysAnsi => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                match std::env::var("TERM") {
                    Err(_) => false,
                    // cygwin doesn't seem to support ANSI escape sequences
                    // and instead has its own variety. However, the Windows
                    // console API may be available.
                    Ok(k) => k != "dumb" && k != "cygwin",
                }
            }
        }
    }

    /// Returns true if this choice should forcefully use ANSI color codes.
    ///
    /// It's possible that ANSI is still the correct choice even if this
    /// returns false.
    #[cfg(not(feature = "std"))]
    pub fn should_ansi(&self) -> bool {
        match *self {
            ColorChoice::Always => false,
            ColorChoice::AlwaysAnsi => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => false,
        }
    }
}
