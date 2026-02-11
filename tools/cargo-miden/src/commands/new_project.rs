use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Args;
use toml_edit::{DocumentMut, Item, Value};

use crate::template::{GenerateArgs, TemplatePath, generate};

/// The tag used in checkout of the new contract project template (`cargo miden new --account <NAME>`, `--note`, etc) .
///
/// Before changing it make sure the new tag exists in the rust-templates repo and points to the
/// desired commit.
const PROJECT_TEMPLATES_REPO_TAG: &str = "v0.27.0";

/// The tag used in checkout of the new Miden project template (`cargo miden new <NAME>`)
///
/// Before changing it make sure the new tag exists in the rust-templates repo and points to the
/// desired commit.
const MIDEN_PROJECT_TEMPLATE_REPO_TAG: &str = "v0.9";

// This should have been an enum but I could not bend `clap` to expose variants as flags
/// Project template
#[derive(Clone, Debug, Args)]
pub struct ProjectTemplate {
    /// Rust program
    #[clap(long, group = "template", conflicts_with_all(["account", "note", "tx_script", "auth_component"]))]
    program: bool,
    /// Miden rollup account
    #[clap(long, group = "template", conflicts_with_all(["program", "note", "tx_script", "auth_component"]))]
    account: bool,
    /// Miden rollup note script
    #[clap(long, group = "template", conflicts_with_all(["program", "account", "tx_script", "auth_component"]))]
    note: bool,
    /// Miden rollup transaction script
    #[clap(long, group = "template", conflicts_with_all(["program", "account", "note", "auth_component"]))]
    tx_script: bool,
    /// Miden rollup authentication component
    #[clap(long, group = "template", conflicts_with_all(["program", "account", "note", "tx_script"]))]
    auth_component: bool,
}

#[allow(unused)]
impl ProjectTemplate {
    pub fn program() -> Self {
        Self {
            program: true,
            account: false,
            note: false,
            tx_script: false,
            auth_component: false,
        }
    }

    pub fn account() -> Self {
        Self {
            program: false,
            account: true,
            note: false,
            tx_script: false,
            auth_component: false,
        }
    }

    pub fn note() -> Self {
        Self {
            program: false,
            account: false,
            note: true,
            tx_script: false,
            auth_component: false,
        }
    }

    pub fn tx_script() -> Self {
        Self {
            program: false,
            account: false,
            note: false,
            tx_script: true,
            auth_component: false,
        }
    }

    pub fn auth_component() -> Self {
        Self {
            program: false,
            account: false,
            note: false,
            tx_script: false,
            auth_component: true,
        }
    }
}

impl Default for ProjectTemplate {
    fn default() -> Self {
        Self::account()
    }
}

impl fmt::Display for ProjectTemplate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.program {
            write!(f, "program")
        } else if self.account {
            write!(f, "account")
        } else if self.note {
            write!(f, "note")
        } else if self.tx_script {
            write!(f, "tx-script")
        } else if self.auth_component {
            write!(f, "auth-component")
        } else {
            panic!("Invalid project template, at least one variant must be set")
        }
    }
}

/// Create a new clean slate Miden project at <path>
#[derive(Debug, Args)]
#[clap(disable_version_flag = true)]
pub struct NewCommand {
    /// The pash for the generated project
    #[clap()]
    pub path: PathBuf,
    /// The template name to use to generate the package
    #[clap(flatten)]
    pub template: Option<ProjectTemplate>,
    /// The path to the template to use to generate the project
    #[clap(long, conflicts_with("template"))]
    pub template_path: Option<PathBuf>,
    /// Use a locally cloned compiler in the generated package
    #[clap(long, hide(true), conflicts_with_all(["compiler_rev", "compiler_branch"]))]
    pub compiler_path: Option<PathBuf>,
    /// Use a specific revision of the compiler in the generated package
    #[clap(long, hide(true), conflicts_with("compiler_branch"))]
    pub compiler_rev: Option<String>,
    /// Use a specific branch of the compiler in the generated package
    #[clap(long, hide(true))]
    pub compiler_branch: Option<String>,
}
use crate::utils::set_default_test_compiler;

impl NewCommand {
    pub fn exec(self) -> anyhow::Result<PathBuf> {
        let name = self
            .path
            .file_name()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to get the last segment of the provided path for the project name"
                )
            })?
            .to_str()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "The last segment of the provided path must be valid UTF8 to generate a valid \
                     project name"
                )
            })?
            .to_string();

        let mut define = vec![];
        if let Some(compiler_path) = self.compiler_path.as_deref() {
            define.push(format!("compiler_path={}", compiler_path.display()));
        }
        if let Some(compiler_rev) = self.compiler_rev.as_deref() {
            define.push(format!("compiler_rev={compiler_rev}"));
        }
        if let Some(compiler_branch) = self.compiler_branch.as_deref() {
            define.push(format!("compiler_branch={compiler_branch}"));
        }

        // If we're running the test suite, and no specific options have been provided for what
        // compiler to use - specify the path to current compiler directory
        if cfg!(test) || std::env::var("TEST").is_ok() {
            let use_local_compiler = self.compiler_path.is_none()
                && self.compiler_rev.is_none()
                && self.compiler_branch.is_none();
            if use_local_compiler {
                set_default_test_compiler(&mut define);
            }
        }

        let template_path = match self.template_path.as_ref() {
            Some(template_path) => TemplatePath {
                path: Some(template_path.display().to_string()),
                ..Default::default()
            },
            None => match self.template.as_ref() {
                Some(project_template) => TemplatePath {
                    git: Some("https://github.com/0xMiden/rust-templates".into()),
                    tag: Some(PROJECT_TEMPLATES_REPO_TAG.into()),
                    auto_path: Some(project_template.to_string()),
                    ..Default::default()
                },
                None => TemplatePath {
                    git: Some("https://github.com/0xMiden/project-template".into()),
                    tag: Some(MIDEN_PROJECT_TEMPLATE_REPO_TAG.into()),
                    ..Default::default()
                },
            },
        };

        let destination = self
            .path
            .parent()
            .map(|p| {
                use path_absolutize::Absolutize;
                p.absolutize().map(|p| p.to_path_buf())
            })
            .transpose()
            .context("Failed to convert destination path to an absolute path")?;
        // Determine whether we should initialize a new Git repository.
        // If the destination directory (where the new project directory will be created)
        // is already inside a Git repository, avoid running `git init` to prevent creating
        // a nested repo.
        let should_git_init = {
            // Resolve the directory where the project will be created (destination root).
            // Use a concrete PathBuf to avoid lifetime issues.
            let dest_root: PathBuf = match &destination {
                Some(dest) => dest.clone(),
                None => {
                    // Fall back to current directory; cargo-generate will create a subdir here.
                    std::env::current_dir()?
                }
            };
            !is_inside_git_repo(&dest_root)
        };

        let generate_args = GenerateArgs {
            template_path,
            destination,
            name: Some(name),
            // Force the `name` to not be kebab-cased
            force: true,
            force_git_init: should_git_init,
            verbose: true,
            define,
        };
        let _project_path = generate(generate_args)
            .context("Failed to scaffold new Miden project from the template")?;

        // Try to add the new crate to workspace Cargo.toml if one exists
        use path_absolutize::Absolutize;
        let project_path_abs = self
            .path
            .absolutize()
            .context("Failed to convert project path to absolute path")?
            .to_path_buf();
        if let Err(e) = add_to_workspace_if_exists(&project_path_abs) {
            // Log warning but don't fail the command if workspace update fails
            eprintln!("Warning: Failed to add crate to workspace: {e}");
        }

        Ok(self.path)
    }
}

/// Returns true if `path` is inside an existing Git repository.
///
/// This checks for a `.git` directory or file in `path` or any of its ancestor
/// directories. A `.git` file is used by worktrees/submodules and should be treated
/// as an indicator of a Git repository as well.
fn is_inside_git_repo(path: &Path) -> bool {
    // Walk up the directory tree from `path` to the filesystem root.
    for ancestor in path.ancestors() {
        let git_marker = ancestor.join(".git");
        if git_marker.exists() {
            return true;
        }
    }
    false
}

/// Finds a workspace Cargo.toml by walking up the directory tree from the given path.
///
/// Returns the path to the workspace Cargo.toml if found, or None if not found.
fn find_workspace_cargo_toml(start_path: &Path) -> Option<PathBuf> {
    // Start from the parent directory of the new project (where it was created)
    let start = start_path.parent()?;

    // Walk up the directory tree
    for ancestor in start.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check if it's a workspace by reading and parsing it
            if let Ok(content) = fs::read_to_string(&cargo_toml)
                && content.contains("[workspace]")
            {
                return Some(cargo_toml);
            }
        }
    }
    None
}

/// Adds a new crate to the workspace Cargo.toml if one exists.
///
/// The member path is relative to the workspace root.
fn add_to_workspace_if_exists(project_path: &Path) -> anyhow::Result<()> {
    let workspace_cargo_toml = match find_workspace_cargo_toml(project_path) {
        Some(path) => path,
        None => {
            // No workspace found, nothing to do
            return Ok(());
        }
    };

    // Read the workspace Cargo.toml
    let content =
        fs::read_to_string(&workspace_cargo_toml).context("Failed to read workspace Cargo.toml")?;

    // Parse the TOML document
    let mut doc = content.parse::<DocumentMut>().context("Failed to parse workspace Cargo.toml")?;

    // Get the workspace root directory
    let workspace_root = workspace_cargo_toml
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Workspace Cargo.toml has no parent directory"))?;

    // Calculate the relative path from workspace root to the new project
    let member_path = project_path
        .strip_prefix(workspace_root)
        .context("Failed to calculate relative path from workspace root to project")?
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Project path contains invalid UTF-8"))?
        .to_string();

    // Ensure the workspace section exists
    if !doc.contains_key("workspace") {
        doc.insert("workspace", Item::Table(toml_edit::Table::new()));
    }

    let workspace = doc["workspace"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Workspace section is not a table"))?;

    // Get or create the members array
    let members = if workspace.contains_key("members") {
        workspace["members"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Workspace members is not an array"))?
    } else {
        let members_array = toml_edit::Array::new();
        workspace.insert("members", Item::Value(Value::Array(members_array)));
        workspace["members"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Failed to create members array"))?
    };

    // Check if the member is already in the list
    let member_path_str = member_path.as_str();
    let already_exists = members.iter().any(|item| {
        if let Some(val) = item.as_str() {
            val == member_path_str
        } else {
            false
        }
    });

    if !already_exists {
        // Add the new member
        members.push(member_path_str);
    }

    // Write the updated Cargo.toml back
    fs::write(&workspace_cargo_toml, doc.to_string())
        .context("Failed to write updated workspace Cargo.toml")?;

    Ok(())
}
