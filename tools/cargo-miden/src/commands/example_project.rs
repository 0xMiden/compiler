use std::path::{Path, PathBuf};

use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath};
use clap::Args;
use toml_edit::{DocumentMut, Item};

use crate::commands::new_project::deploy_wit_files;

/// The folder name to put Miden SDK WIT files in
pub const WIT_DEPS_PATH: &str = "wit-deps";

/// Create a new Miden example project
#[derive(Args)]
#[clap(disable_version_flag = true)]
pub struct ExampleCommand {
    #[clap(help = r#"The example name to use from the compiler repository
Available examples:
basic-wallet      : Basic wallet account
p2id-note         : Pay-to-ID note
counter-contract  : Counter contract
counter-note      : Counter note
fibonacci         : Fibonacci sequence calculator
collatz           : Collatz conjecture calculator
is-prime          : Prime number checker
storage-example   : Storage operations example"#)]
    pub example_name: String,
}

use std::fs;

impl ExampleCommand {
    pub fn exec(self) -> anyhow::Result<PathBuf> {
        // Check if this is a paired project
        let paired_projects = match self.example_name.as_str() {
            "basic-wallet" | "p2id-note" => Some(("basic-wallet", "p2id-note")),
            "counter-contract" | "counter-note" => Some(("counter-contract", "counter-note")),
            _ => None,
        };

        if let Some((first, second)) = paired_projects {
            self.exec_paired_projects(first, second)
        } else {
            self.exec_single_project()
        }
    }

    fn exec_single_project(&self) -> anyhow::Result<PathBuf> {
        // Use example name as project name
        let project_name = self.example_name.clone();
        let project_path = PathBuf::from(&project_name);

        // Check if directory already exists
        if project_path.exists() {
            return Err(anyhow::anyhow!(
                "Directory '{}' already exists. Please remove it or choose a different location.",
                project_name
            ));
        }

        let mut define = vec![];
        // If we're running the test suite, specify the path to current compiler directory
        if cfg!(test) || std::env::var("TEST").is_ok() {
            set_default_test_compiler(&mut define);
        }

        let template_path = TemplatePath {
            git: Some("https://github.com/0xMiden/compiler".into()),
            auto_path: Some(format!("examples/{}", self.example_name)),
            ..Default::default()
        };

        // Generate in current directory
        let destination = {
            use path_absolutize::Absolutize;
            std::env::current_dir()?.absolutize().map(|p| p.to_path_buf())?
        };

        let generate_args = GenerateArgs {
            template_path,
            destination: Some(destination),
            name: Some(project_name.clone()),
            // Force the `name` to not be kebab-cased
            force: true,
            force_git_init: true,
            verbose: true,
            define,
            ..Default::default()
        };
        cargo_generate::generate(generate_args)
            .context("Failed to scaffold new Miden project from the template")?;

        // Check if the project has WIT files
        let wit_dir = project_path.join("wit");
        if wit_dir.exists() && wit_dir.is_dir() {
            // Deploy core WIT files to the project
            deploy_wit_files(&project_path).context("Failed to deploy WIT files")?;
        }

        // Process the Cargo.toml to update dependencies and WIT paths
        process_cargo_toml(&project_path).context("Failed to process Cargo.toml")?;

        Ok(project_path)
    }

    fn exec_paired_projects(
        &self,
        first_project: &str,
        second_project: &str,
    ) -> anyhow::Result<PathBuf> {
        // Create main directory with the requested example name
        let main_dir = PathBuf::from(&self.example_name);

        if main_dir.exists() {
            return Err(anyhow::anyhow!(
                "Directory '{}' already exists. Please remove it or choose a different location.",
                self.example_name
            ));
        }

        // Create the main directory
        fs::create_dir_all(&main_dir)?;

        let mut define = vec![];
        if cfg!(test) || std::env::var("TEST").is_ok() {
            set_default_test_compiler(&mut define);
        }

        // Generate both projects
        let examples = [first_project, second_project];
        for example in &examples {
            let template_path = TemplatePath {
                git: Some("https://github.com/0xMiden/compiler".into()),
                auto_path: Some(format!("examples/{}", example)),
                ..Default::default()
            };

            let destination = {
                use path_absolutize::Absolutize;
                main_dir.absolutize().map(|p| p.to_path_buf())?
            };

            let generate_args = GenerateArgs {
                template_path,
                destination: Some(destination),
                name: Some(example.to_string()),
                force: true,
                force_git_init: false, // Don't init git for subdirectories
                verbose: true,
                define: define.clone(),
                ..Default::default()
            };

            cargo_generate::generate(generate_args)
                .context(format!("Failed to scaffold {} project", example))?;

            let project_path = main_dir.join(example);

            // Check if the project has WIT files
            let wit_dir = project_path.join("wit");
            if wit_dir.exists() && wit_dir.is_dir() {
                deploy_wit_files(&project_path)
                    .context(format!("Failed to deploy WIT files for {}", example))?;
            }

            // Process the Cargo.toml
            process_cargo_toml(&project_path)
                .context(format!("Failed to process Cargo.toml for {}", example))?;
        }

        // Update dependencies for paired projects
        match (first_project, second_project) {
            ("basic-wallet", "p2id-note") => update_note_dependencies(
                &main_dir,
                "p2id-note",
                "miden:basic-wallet",
                "basic-wallet",
                "basic-wallet.wit",
            )?,
            ("counter-contract", "counter-note") => update_note_dependencies(
                &main_dir,
                "counter-note",
                "miden:counter-contract",
                "counter-contract",
                "counter.wit",
            )?,
            _ => {}
        }

        Ok(main_dir)
    }
}

fn set_default_test_compiler(define: &mut Vec<String>) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let compiler_path = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    define.push(format!("compiler_path={}", compiler_path.display()));
}

/// Process the generated Cargo.toml to update dependencies and WIT paths
fn process_cargo_toml(project_path: &Path) -> anyhow::Result<()> {
    let cargo_toml_path = project_path.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml_path)?;
    let mut doc = content.parse::<DocumentMut>()?;

    // Update miden dependency to use git repository
    if let Some(deps) = doc.get_mut("dependencies").and_then(|d| d.as_table_mut()) {
        if let Some(miden_dep) = deps.get_mut("miden") {
            *miden_dep = Item::Value(toml_edit::Value::InlineTable({
                let mut table = toml_edit::InlineTable::new();
                table.insert("git", "https://github.com/0xMiden/compiler".into());
                table
            }));
        }
    }

    // Update WIT file paths to use the deployed files
    if let Some(metadata) = doc
        .get_mut("package")
        .and_then(|p| p.as_table_mut())
        .and_then(|t| t.get_mut("metadata"))
        .and_then(|m| m.as_table_mut())
        .and_then(|t| t.get_mut("component"))
        .and_then(|c| c.as_table_mut())
        .and_then(|t| t.get_mut("target"))
        .and_then(|t| t.as_table_mut())
        .and_then(|t| t.get_mut("dependencies"))
        .and_then(|d| d.as_table_mut())
    {
        // Update each WIT dependency to use the deployed files
        for (key, value) in metadata.iter_mut() {
            if let Some(table) = value.as_inline_table_mut() {
                if let Some(path_value) = table.get_mut("path") {
                    match key.as_ref() {
                        "miden:base" => {
                            *path_value =
                                toml_edit::Value::from(format!("{}/miden.wit", WIT_DEPS_PATH));
                        }
                        "miden:core-intrinsics" => {
                            *path_value = toml_edit::Value::from(format!(
                                "{}/miden-core-intrinsics.wit",
                                WIT_DEPS_PATH
                            ));
                        }
                        "miden:core-stdlib" => {
                            *path_value = toml_edit::Value::from(format!(
                                "{}/miden-core-stdlib.wit",
                                WIT_DEPS_PATH
                            ));
                        }
                        "miden:core-base" => {
                            *path_value = toml_edit::Value::from(format!(
                                "{}/miden-core-base.wit",
                                WIT_DEPS_PATH
                            ));
                        }
                        _ => {
                            // For project-specific WIT files, check if they exist in wit/
                            if let Some(path_str) = path_value.as_str() {
                                let path = Path::new(path_str);
                                if let Some(file_name) = path.file_name() {
                                    let wit_file = project_path.join("wit").join(file_name);
                                    if wit_file.exists() {
                                        // Update to use the wit/ directory path
                                        *path_value = toml_edit::Value::from(format!(
                                            "wit/{}",
                                            file_name.to_string_lossy()
                                        ));
                                    }
                                    // Don't remove anything, just leave other paths as they are
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Write the updated Cargo.toml
    fs::write(&cargo_toml_path, doc.to_string())?;
    Ok(())
}

/// Update note project's dependencies to use local contract
fn update_note_dependencies(
    main_dir: &Path,
    note_dir: &str,
    dependency_name: &str,
    contract_dir: &str,
    wit_file_name: &str,
) -> anyhow::Result<()> {
    let note_cargo_toml = main_dir.join(note_dir).join("Cargo.toml");
    let content = fs::read_to_string(&note_cargo_toml)?;
    let mut doc = content.parse::<DocumentMut>()?;

    // Update miden dependency to use local path
    if let Some(miden_deps) = doc
        .get_mut("package")
        .and_then(|p| p.as_table_mut())
        .and_then(|t| t.get_mut("metadata"))
        .and_then(|m| m.as_table_mut())
        .and_then(|t| t.get_mut("miden"))
        .and_then(|m| m.as_table_mut())
        .and_then(|t| t.get_mut("dependencies"))
        .and_then(|d| d.as_table_mut())
    {
        if let Some(dep) = miden_deps.get_mut(dependency_name) {
            *dep = Item::Value(toml_edit::Value::InlineTable({
                let mut table = toml_edit::InlineTable::new();
                table.insert("path", format!("../{}", contract_dir).into());
                table
            }));
        }
    }

    // Update WIT file dependency to use local contract
    if let Some(wit_deps) = doc
        .get_mut("package")
        .and_then(|p| p.as_table_mut())
        .and_then(|t| t.get_mut("metadata"))
        .and_then(|m| m.as_table_mut())
        .and_then(|t| t.get_mut("component"))
        .and_then(|c| c.as_table_mut())
        .and_then(|t| t.get_mut("target"))
        .and_then(|t| t.as_table_mut())
        .and_then(|t| t.get_mut("dependencies"))
        .and_then(|d| d.as_table_mut())
    {
        if let Some(wit_dep) = wit_deps.get_mut(dependency_name) {
            if let Some(table) = wit_dep.as_inline_table_mut() {
                if let Some(path_value) = table.get_mut("path") {
                    *path_value = toml_edit::Value::from(format!(
                        "../{}/wit/{}",
                        contract_dir, wit_file_name
                    ));
                }
            }
        }
    }

    fs::write(&note_cargo_toml, doc.to_string())?;
    Ok(())
}
