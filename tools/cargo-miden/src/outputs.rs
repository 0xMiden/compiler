use std::path::PathBuf;

/// Represents the structured output of a successful `cargo miden` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    /// Output from the `new` command.
    NewCommandOutput {
        /// The path to the newly created project directory.
        project_path: PathBuf,
    },
    /// Output from the `build` command.
    BuildCommandOutput {
        /// The type and path of the artifact produced by the build.
        output: BuildOutput,
    },
    // Add other variants here if other commands need structured output later.
}

impl CommandOutput {
    /// Panics if the output is not `BuildCommandOutput`, otherwise returns the inner `BuildOutput`.
    pub fn unwrap_build_output(self) -> BuildOutput {
        match self {
            CommandOutput::BuildCommandOutput { output } => output,
            _ => panic!("called `unwrap_build_output()` on a non-BuildCommandOutput value"),
        }
    }
}

/// Represents the specific artifact produced by the `build` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutput {
    /// Miden Assembly (.masm) output.
    Masm {
        /// Path to the compiled MASM file or directory containing artifacts.
        artifact_path: PathBuf,
        // Potentially add other relevant info like package name, component type etc.
    },
    /// WebAssembly (.wasm) output.
    Wasm {
        /// Path to the compiled WASM file.
        artifact_path: PathBuf,
        /// Miden package for each dependency
        dependencies: Vec<PathBuf>,
    },
}
