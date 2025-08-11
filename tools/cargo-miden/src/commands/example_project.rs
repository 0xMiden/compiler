use std::path::{Path, PathBuf};

use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath};
use clap::Args;
use toml_edit::{DocumentMut, Item};

use crate::{commands::new_project::deploy_wit_files, utils::compiler_path};

/// The folder name to put Miden SDK WIT files in
pub const WIT_DEPS_PATH: &str = "wit-deps";

/// Paired project mappings for examples that create multiple related projects
const PAIRED_PROJECTS: &[(&str, &str)] = &[("counter-contract", "counter-note")];

/// Triple project mappings for examples that create three related projects
/// Each tuple contains (tx-script, account, note) project names
/// Any of these names can be used to create all three projects
const TRIPLE_PROJECTS: &[(&str, &str, &str)] =
    &[("basic-wallet-tx-script", "basic-wallet", "p2id-note")];

/// Core WIT dependency mappings (package name, file name)
const CORE_WIT_DEPS: &[(&str, &str)] = &[
    ("miden:base", "miden.wit"),
    ("miden:core-intrinsics", "miden-core-intrinsics.wit"),
    ("miden:core-stdlib", "miden-core-stdlib.wit"),
    ("miden:core-base", "miden-core-base.wit"),
];

/// Create a new Miden example project
#[derive(Args)]
#[clap(disable_version_flag = true)]
pub struct ExampleCommand {
    #[clap(help = r#"The example name to use from the compiler repository
Available examples:
basic-wallet          : Basic wallet account
p2id-note             : Pay-to-ID note
basic-wallet-tx-script: Transaction script used in basic-wallet and p2id-note
counter-contract      : Counter contract
counter-note          : Counter note
fibonacci             : Fibonacci sequence calculator
collatz               : Collatz conjecture calculator
is-prime              : Prime number checker
storage-example       : Storage operations example"#)]
    pub example_name: String,
}

use std::fs;

impl ExampleCommand {
    pub fn exec(self) -> anyhow::Result<PathBuf> {
        // Check if this is a triple project - any of the three names can be used
        if let Some((tx_script, account, note)) =
            TRIPLE_PROJECTS.iter().find(|(tx_script, account, note)| {
                *tx_script == self.example_name
                    || *account == self.example_name
                    || *note == self.example_name
            })
        {
            // Always use the tx-script name as the main directory name
            self.exec_triple_projects(tx_script, account, note)
        } else if let Some((first, second)) = PAIRED_PROJECTS
            .iter()
            .find(|(first, second)| *first == self.example_name || *second == self.example_name)
        {
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

        let template_path = template_path(&project_name);

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

    /// Create a pair (account and note script) projects in a sub-folder
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

        // Generate both projects
        let project_names = [first_project, second_project];
        for project_name in &project_names {
            let template_path = template_path(project_name);

            let destination = {
                use path_absolutize::Absolutize;
                main_dir.absolutize().map(|p| p.to_path_buf())?
            };

            let generate_args = GenerateArgs {
                template_path,
                destination: Some(destination),
                name: Some(project_name.to_string()),
                force: true,
                force_git_init: false, // Don't init git for subdirectories
                verbose: true,
                ..Default::default()
            };

            cargo_generate::generate(generate_args)
                .context(format!("Failed to scaffold {project_name} project"))?;

            let project_path = main_dir.join(project_name);

            // Check if the project has WIT files
            let wit_dir = project_path.join("wit");
            if wit_dir.exists() && wit_dir.is_dir() {
                deploy_wit_files(&project_path)
                    .context(format!("Failed to deploy WIT files for {project_name}"))?;
            }

            // Process the Cargo.toml
            process_cargo_toml(&project_path)
                .context(format!("Failed to process Cargo.toml for {project_name}"))?;
        }

        // Update dependencies for paired projects
        if first_project == "counter-contract" && second_project == "counter-note" {
            update_project_dependency(
                &main_dir,
                "counter-note",
                "miden:counter-contract",
                "counter-contract",
                "counter.wit",
            )?;
        }

        Ok(main_dir)
    }

    /// Create a triple (tx-script, account and note script) projects in a sub-folder
    fn exec_triple_projects(
        &self,
        tx_script: &str,
        account: &str,
        note: &str,
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

        // Generate all three projects
        let project_names = [tx_script, account, note];
        for project_name in &project_names {
            let template_path = template_path(project_name);

            let destination = {
                use path_absolutize::Absolutize;
                main_dir.absolutize().map(|p| p.to_path_buf())?
            };

            let generate_args = GenerateArgs {
                template_path,
                destination: Some(destination),
                name: Some(project_name.to_string()),
                force: true,
                force_git_init: false, // Don't init git for subdirectories
                verbose: true,
                ..Default::default()
            };

            cargo_generate::generate(generate_args)
                .context(format!("Failed to scaffold {project_name} project"))?;

            let project_path = main_dir.join(project_name);

            // Check if the project has WIT files
            let wit_dir = project_path.join("wit");
            if wit_dir.exists() && wit_dir.is_dir() {
                deploy_wit_files(&project_path)
                    .context(format!("Failed to deploy WIT files for {project_name}"))?;
            }

            // Process the Cargo.toml
            process_cargo_toml(&project_path)
                .context(format!("Failed to process Cargo.toml for {project_name}"))?;
        }

        // Update dependencies for triple projects
        update_triple_project_dependencies(&main_dir, tx_script, account, note)?;

        Ok(main_dir)
    }
}

/// Update dependencies for triple projects in a generic way
fn update_triple_project_dependencies(
    main_dir: &Path,
    tx_script: &str,
    account: &str,
    note: &str,
) -> anyhow::Result<()> {
    // Use the actual WIT file name (keep hyphens)
    let account_wit = format!("{account}.wit");

    // Update note to depend on account
    update_project_dependency(main_dir, note, &format!("miden:{account}"), account, &account_wit)?;

    // Update tx script to depend on account
    update_project_dependency(
        main_dir,
        tx_script,
        &format!("miden:{account}"),
        account,
        &account_wit,
    )?;

    Ok(())
}

/// Process the generated Cargo.toml to update dependencies and WIT paths
/// The projects in `example` folder set Miden SDK dependencies as local paths.
/// After copying we need to change them to be git dependency (Miden SDK crate) and local WIT files
/// (deployed from the Miden SDK crates by `deploy_wit_files()`)
fn process_cargo_toml(project_path: &Path) -> anyhow::Result<()> {
    let cargo_toml_path = project_path.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml_path)?;
    let mut doc = content.parse::<DocumentMut>()?;

    // Update miden dependency to use git repository
    if let Some(deps) = doc.get_mut("dependencies").and_then(|d| d.as_table_mut()) {
        if let Some(miden_dep) = deps.get_mut("miden") {
            *miden_dep = Item::Value(toml_edit::Value::InlineTable({
                let mut table = toml_edit::InlineTable::new();
                if cfg!(test) || std::env::var("TEST").is_ok() {
                    table.insert("path", compiler_path().join("sdk/sdk").to_str().unwrap().into());
                } else {
                    table.insert("git", "https://github.com/0xMiden/compiler".into());
                }

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
                    if let Some((_, wit_file)) =
                        CORE_WIT_DEPS.iter().find(|(dep, _)| *dep == key.get())
                    {
                        *path_value = toml_edit::Value::from(format!("{WIT_DEPS_PATH}/{wit_file}"));
                    };
                }
            }
        }
    }

    // Write the updated Cargo.toml
    fs::write(&cargo_toml_path, doc.to_string())?;
    Ok(())
}

/// Update a project's dependencies to use another local project
/// This is used when one project in a pair/triple depends on another
fn update_project_dependency(
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
                table.insert("path", format!("../{contract_dir}").into());
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
                    *path_value =
                        toml_edit::Value::from(format!("../{contract_dir}/wit/{wit_file_name}"));
                }
            }
        }
    }

    fs::write(&note_cargo_toml, doc.to_string())?;
    Ok(())
}

fn template_path(project_name: &str) -> TemplatePath {
    if cfg!(test) || std::env::var("TEST").is_ok() {
        TemplatePath {
            path: Some(
                compiler_path()
                    .join("examples")
                    .join(project_name)
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
            ..Default::default()
        }
    } else {
        TemplatePath {
            git: Some("https://github.com/0xMiden/compiler".into()),
            auto_path: Some(format!("examples/{project_name}")),
            ..Default::default()
        }
    }
}
