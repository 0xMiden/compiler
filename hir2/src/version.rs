use core::{fmt, str::FromStr};

pub use semver::{self, VersionReq};

use crate::{define_attr_type, formatter};

/// Represents a Semantic Versioning version string.
///
/// This is a newtype wrapper around [semver::Version], in order to make it representable as an
/// attribute value in the IR.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(semver::Version);

impl Version {
    /// Create a new [Version] from the given components, with empty pre-release and build metadata.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self(semver::Version::new(major, minor, patch))
    }

    /// Create Version by parsing from string representation.
    ///
    /// # Errors
    ///
    /// Possible reasons for the parse to fail include:
    ///
    /// * `1.0` — too few numeric components. A SemVer version must have exactly three. If you are
    ///   looking at something that has fewer than three numbers in it, it’s possible it is a
    ///   [semver::VersionReq] instead (with an implicit default ^ comparison operator).
    /// * `1.0.01` — a numeric component has a leading zero.
    /// * `1.0.unknown` — unexpected character in one of the components.
    /// * `1.0.0- or 1.0.0+` — the pre-release or build metadata are indicated present but empty.
    /// * `1.0.0-alpha_123` — pre-release or build metadata have something outside the allowed characters, which are 0-9, A-Z, a-z, -, and . (dot).
    /// * `23456789999999999999.0.0` — overflow of a u64.
    pub fn parse(version: impl AsRef<str>) -> Result<Self, semver::Error> {
        semver::Version::parse(version.as_ref()).map(Self)
    }
}

define_attr_type!(Version);

impl FromStr for Version {
    type Err = semver::Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl core::ops::Deref for Version {
    type Target = semver::Version;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for Version {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl formatter::PrettyPrint for Version {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        display(&self.0)
    }
}
