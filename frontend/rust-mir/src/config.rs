//! Configuration of the Rust MIR frontend.

use std::path::PathBuf;

/// The default Rust edition used to compile the input file.
const DEFAULT_EDITION: &str = "2024";

/// Controls how the Rust MIR frontend runs rustc over the input file.
#[derive(Debug, Clone)]
pub struct RustMirTranslationConfig {
    /// The name given to the compiled crate.
    ///
    /// If this is `None`, the frontend uses the file stem of the input file.
    pub crate_name: Option<String>,
    /// The directory that receives the rustc output.
    ///
    /// If this is `None`, the frontend uses a temporary directory that it removes after the
    /// translation.
    pub out_dir: Option<PathBuf>,
    /// The Rust edition of the input file.
    pub edition: String,
}

impl Default for RustMirTranslationConfig {
    fn default() -> Self {
        Self {
            crate_name: None,
            out_dir: None,
            edition: DEFAULT_EDITION.to_string(),
        }
    }
}
