use std::{fmt, path::PathBuf};

use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath};
use clap::Args;

// This should have been an enum but I could not bend `clap` to expose variants as flags
/// Project template
#[derive(Clone, Args)]
pub struct ProjectTemplate {
    /// Rust program
    #[clap(long, group = "template", conflicts_with_all(["account", "note"]))]
    program: bool,
    /// Miden rollup account
    #[clap(long, group = "template", conflicts_with_all(["program", "note"]))]
    account: bool,
    /// Miden rollup note script
    #[clap(long, group = "template", conflicts_with_all(["program", "account"]))]
    note: bool,
}

#[allow(unused)]
impl ProjectTemplate {
    pub fn program() -> Self {
        Self {
            program: true,
            account: false,
            note: false,
        }
    }

    pub fn account() -> Self {
        Self {
            program: false,
            account: true,
            note: false,
        }
    }

    pub fn note() -> Self {
        Self {
            program: false,
            account: false,
            note: true,
        }
    }
}

impl Default for ProjectTemplate {
    fn default() -> Self {
        Self {
            program: false,
            account: true,
            note: false,
        }
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
        } else {
            write!(f, "account")
        }
    }
}

/// Create a new Miden project at <path>
#[derive(Args)]
#[clap(disable_version_flag = true)]
pub struct NewCommand {
    /// The path for the generated package (the directory name is used for project name)
    #[clap()]
    pub path: PathBuf,
    /// The template name to use to generate the package
    #[clap(flatten)]
    pub template: Option<ProjectTemplate>,
    /// The path to the template to use to generate the package
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
            None => {
                let project_kind_str = match self.template {
                    Some(kind) => kind.to_string(),
                    None => ProjectTemplate::default().to_string(),
                };
                TemplatePath {
                    git: Some("https://github.com/0xPolygonMiden/rust-templates".into()),
                    tag: Some("v0.6.0".into()),
                    auto_path: Some(project_kind_str),
                    ..Default::default()
                }
            }
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
        let generate_args = GenerateArgs {
            template_path,
            destination,
            name: Some(name),
            // Force the `name` to not be kebab-cased
            force: true,
            force_git_init: true,
            verbose: true,
            define,
            ..Default::default()
        };
        cargo_generate::generate(generate_args)
            .context("Failed to scaffold new Miden project from the template")?;
        Ok(self.path)
    }
}

fn set_default_test_compiler(define: &mut Vec<String>) {
    use std::path::Path;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let compiler_path = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    define.push(format!("compiler_path={}", compiler_path.display()));
}
