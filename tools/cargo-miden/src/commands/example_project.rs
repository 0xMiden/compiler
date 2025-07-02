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
    /// The example name to use from the compiler repository (will also be used as project name)
    #[clap()]
    pub example_name: String,
}

use std::fs;

impl ExampleCommand {
    pub fn exec(self) -> anyhow::Result<PathBuf> {
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
            std::env::current_dir()?
                .absolutize()
                .map(|p| p.to_path_buf())?
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
            deploy_wit_files(&project_path)
                .context("Failed to deploy WIT files")?;
        }

        // Process the Cargo.toml to update dependencies and WIT paths
        process_cargo_toml(&project_path)
            .context("Failed to process Cargo.toml")?;

        Ok(project_path)
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
    if let Some(metadata) = doc.get_mut("package")
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
                            *path_value = toml_edit::Value::from(format!("{}/miden.wit", WIT_DEPS_PATH));
                        }
                        "miden:core-intrinsics" => {
                            *path_value = toml_edit::Value::from(format!("{}/miden-core-intrinsics.wit", WIT_DEPS_PATH));
                        }
                        "miden:core-stdlib" => {
                            *path_value = toml_edit::Value::from(format!("{}/miden-core-stdlib.wit", WIT_DEPS_PATH));
                        }
                        "miden:core-base" => {
                            *path_value = toml_edit::Value::from(format!("{}/miden-core-base.wit", WIT_DEPS_PATH));
                        }
                        _ => {
                            // For project-specific WIT files, check if they exist in wit/
                            if let Some(path_str) = path_value.as_str() {
                                let path = Path::new(path_str);
                                if let Some(file_name) = path.file_name() {
                                    let wit_file = project_path.join("wit").join(file_name);
                                    if wit_file.exists() {
                                        // Update to use the wit/ directory path
                                        *path_value = toml_edit::Value::from(format!("wit/{}", file_name.to_string_lossy()));
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

