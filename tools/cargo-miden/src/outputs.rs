use std::path::PathBuf;

/// Represents the structured output of a successful `cargo miden` command.
#[derive(Debug, Clone)]
pub enum CommandOutput {
    /// Output from the `new` command.
    NewCommandOutput {
        /// The path to the newly created project directory.
        project_path: PathBuf,
    },
    /// Output from the `build` command.
    BuildCommandOutput {
        /// The type and path of the artifact produced by the build.
        output: Vec<PathBuf>,
    },
    // Add other variants here if other commands need structured output later.
}

impl CommandOutput {
    /// Panics if the output is not `BuildCommandOutput`, otherwise returns the inner `BuildOutput`.
    pub fn unwrap_build_output(self) -> Vec<PathBuf> {
        match self {
            CommandOutput::BuildCommandOutput { output } => output,
            _ => panic!("called `unwrap_build_output()` on a non-BuildCommandOutput value"),
        }
    }

    /// Panics if the output is not `NewCommandOutput`, otherwise returns the inner project path.
    pub fn unwrap_new_output(self) -> PathBuf {
        match self {
            CommandOutput::NewCommandOutput { project_path } => project_path,
            _ => panic!("called `unwrap_new_output()` on a non-NewCommandOutput value"),
        }
    }
}
