mod debuginfo;

use clap::{Subcommand, ValueEnum};

/// Dump useful information from assembled Miden packages
#[derive(Debug, Subcommand)]
#[command(name = "debuginfo", rename_all = "kebab-case")]
pub enum Dump {
    /// Dump debug information encoded in a .masp file
    DebugInfo(debuginfo::Config),
}

/// The set of known sections that we've added dump support for
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Section {
    /// Show string table
    Strings,
    /// Show type information
    Types,
    /// Show source file information
    Files,
    /// Show function debug information
    Functions,
    /// Show variable information within functions
    Variables,
    /// Show variable location decorators from MAST (similar to DWARF .debug_loc)
    Locations,
}

#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse package: {0}")]
    Parse(String),
    #[error("no debug_info section found in package")]
    NoDebugInfo,
}

impl From<miden_core::serde::DeserializationError> for DumpError {
    #[inline]
    fn from(err: miden_core::serde::DeserializationError) -> Self {
        Self::Parse(err.to_string())
    }
}

pub fn run(command: &Dump) -> Result<(), DumpError> {
    match command {
        Dump::DebugInfo(config) => debuginfo::dump(config),
    }
}
