use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use liquid::{model::Value, Object, Parser};
use tempfile::TempDir;
use toml_edit::DocumentMut;
use walkdir::WalkDir;

/// Describes the source template location.
#[derive(Clone, Debug, Default)]
pub struct TemplatePath {
    /// Local filesystem path containing the template.
    pub path: Option<String>,
    /// Remote git repository hosting the template.
    pub git: Option<String>,
    /// Git branch to checkout after cloning.
    pub branch: Option<String>,
    /// Git tag to checkout after cloning.
    pub tag: Option<String>,
    /// Git revision (commit SHA) to checkout after cloning.
    pub rev: Option<String>,
    /// Subdirectory inside the template repository that contains the actual template.
    pub auto_path: Option<String>,
}

/// Arguments required to expand a template into a project.
#[derive(Clone, Debug, Default)]
pub struct GenerateArgs {
    pub template_path: TemplatePath,
    pub destination: Option<PathBuf>,
    pub name: Option<String>,
    pub force: bool,
    pub force_git_init: bool,
    pub verbose: bool,
    pub define: Vec<String>,
}

/// Expands a project template into the requested destination directory.
pub fn generate(args: GenerateArgs) -> Result<PathBuf> {
    let project_name = args
        .name
        .clone()
        .context("A project name must be provided to generate a template")?;

    let template_source = prepare_template(&args.template_path)?;
    let mut source_root = match &args.template_path.auto_path {
        Some(auto_path) => template_source.root.join(auto_path),
        None => template_source.root.clone(),
    };

    if !source_root.exists() {
        bail!("Template directory '{}' does not exist", source_root.display());
    }

    if source_root.join("template").is_dir() {
        source_root = source_root.join("template");
    }

    let config = load_template_config(&source_root)?;

    let destination_root = args
        .destination
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current directory is accessible"));
    fs::create_dir_all(&destination_root).with_context(|| {
        format!("Failed to create destination directory '{}'", destination_root.display())
    })?;

    let project_dir = destination_root.join(&project_name);
    prepare_destination(&project_dir, args.force)?;

    let crate_name = sanitize_crate_name(&project_name);
    let mut variables = build_variable_map(crate_name, &args.define)?;
    // Expose the original project name as well.
    variables.insert("project_name".into(), Value::scalar(project_name.clone()));
    variables.insert("project-name".into(), Value::scalar(project_name.clone()));

    let parser = liquid::ParserBuilder::with_stdlib()
        .build()
        .context("Failed to initialise Liquid template parser")?;

    render_template(&source_root, &project_dir, &parser, &variables, &config)?;

    if args.force_git_init {
        initialise_git_repo(&project_dir)?;
    }

    if args.verbose {
        log::info!("Generated project '{}' in '{}'", project_name, project_dir.display());
    }

    println!("Created project {}", project_dir.display());

    Ok(project_dir)
}

struct TemplateSource {
    root: PathBuf,
    _keepalive: Option<TempDir>,
}

fn prepare_template(template_path: &TemplatePath) -> Result<TemplateSource> {
    if let Some(path) = template_path.path.as_ref() {
        let root = PathBuf::from(path);
        return Ok(TemplateSource {
            root,
            _keepalive: None,
        });
    }

    let repo = template_path
        .git
        .as_ref()
        .context("Template source must specify either `path` or `git`")?;
    let temp_dir = TempDir::new().context("Failed to create temporary directory for template")?;

    clone_repository(repo, template_path, temp_dir.path())?;

    Ok(TemplateSource {
        root: temp_dir.path().to_path_buf(),
        _keepalive: Some(temp_dir),
    })
}

fn clone_repository(repo: &str, template_path: &TemplatePath, destination: &Path) -> Result<()> {
    let mut command = Command::new("git");
    command
        .arg("clone")
        .arg("--single-branch")
        .arg("--depth")
        .arg("1")
        .arg("--quiet");

    if let Some(branch) = template_path.branch.as_ref() {
        command.arg("--branch").arg(branch);
    } else if let Some(tag) = template_path.tag.as_ref() {
        command.arg("--branch").arg(tag);
    }

    command.arg(repo).arg(destination);

    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("Failed to clone template repository '{repo}'"))?;
    if !status.success() {
        bail!("`git clone {repo}` exited with {}", status);
    }

    if let Some(rev) = template_path.rev.as_ref() {
        let status = Command::new("git")
            .arg("checkout")
            .arg("--quiet")
            .arg(rev)
            .current_dir(destination)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("Failed to checkout revision '{rev}'"))?;

        if !status.success() {
            bail!("`git checkout {rev}` exited with {}", status);
        }
    }

    Ok(())
}

fn prepare_destination(project_dir: &Path, force: bool) -> Result<()> {
    if project_dir.exists() {
        if !project_dir.is_dir() {
            bail!("Destination '{}' exists and is not a directory", project_dir.display());
        }

        if !force && !is_empty_directory(project_dir)? {
            bail!(
                "Destination '{}' already exists. Use --force to overwrite.",
                project_dir.display()
            );
        }
    } else {
        fs::create_dir_all(project_dir).with_context(|| {
            format!("Failed to create project directory '{}'", project_dir.display())
        })?;
    }

    Ok(())
}

fn is_empty_directory(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read destination directory '{}'", path.display()))?;
    Ok(entries.next().is_none())
}

fn build_variable_map(crate_name: String, define: &[String]) -> Result<Object> {
    let mut variables = Object::new();
    variables.insert("crate_name".into(), Value::scalar(crate_name));

    for define_arg in define {
        let (key, value) = parse_define(define_arg)?;
        variables.insert(key.into(), Value::scalar(value));
    }

    Ok(variables)
}

fn parse_define(input: &str) -> Result<(String, String)> {
    let mut parts = input.splitn(2, '=');
    let key = parts.next().context("Invalid define argument: missing key")?.trim();
    if key.is_empty() {
        bail!("Invalid define argument: key must not be empty");
    }
    let value = parts.next().context("Invalid define argument: missing value")?;
    Ok((key.to_string(), value.to_string()))
}

fn render_template(
    source_root: &Path,
    destination: &Path,
    parser: &Parser,
    variables: &Object,
    config: &TemplateConfig,
) -> Result<()> {
    for entry in WalkDir::new(source_root) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                log::warn!("Skipping template entry due to error: {err}");
                continue;
            }
        };

        let relative = match entry.path().strip_prefix(source_root) {
            Ok(relative) if relative.as_os_str().is_empty() => continue,
            Ok(relative) => relative,
            Err(_) => continue,
        };

        if should_ignore(relative, config) {
            continue;
        }

        if relative.file_name() == Some(OsStr::new("cargo-generate.toml")) {
            continue;
        }

        if relative
            .components()
            .any(|component| component.as_os_str() == OsStr::new(".git"))
        {
            continue;
        }

        let target_path = destination.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target_path).with_context(|| {
                format!("Failed to create directory '{}'", target_path.display())
            })?;
            continue;
        }

        render_file(entry.path(), &target_path, parser, variables)?;
    }

    Ok(())
}

fn render_file(
    source: &Path,
    destination: &Path,
    parser: &Parser,
    variables: &Object,
) -> Result<()> {
    let bytes = fs::read(source)
        .with_context(|| format!("Failed to read template file '{}'", source.display()))?;

    match std::str::from_utf8(&bytes) {
        Ok(content) => {
            let template = parser
                .parse(content)
                .with_context(|| format!("Failed to parse template '{}'", source.display()))?;
            let rendered = template
                .render(variables)
                .with_context(|| format!("Failed to render template '{}'", source.display()))?;
            fs::write(destination, rendered).with_context(|| {
                format!("Failed to write rendered file '{}'", destination.display())
            })?;
        }
        Err(_) => {
            // Binary data - copy verbatim.
            fs::write(destination, &bytes).with_context(|| {
                format!("Failed to write binary file '{}'", destination.display())
            })?;
        }
    }

    // Preserve executable bit when present.
    let metadata = fs::metadata(source)?;
    fs::set_permissions(destination, metadata.permissions())
        .with_context(|| format!("Failed to set permissions on '{}'", destination.display()))?;

    Ok(())
}

fn initialise_git_repo(project_dir: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(project_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to execute `git init`")?;

    if !status.success() {
        bail!("`git init` exited with {}", status);
    }

    Ok(())
}

#[derive(Default)]
struct TemplateConfig {
    ignore: Vec<String>,
}

fn load_template_config(template_root: &Path) -> Result<TemplateConfig> {
    let mut config = TemplateConfig::default();
    let config_path = template_root.join("cargo-generate.toml");
    if !config_path.exists() {
        return Ok(config);
    }

    let contents = fs::read_to_string(&config_path).with_context(|| {
        format!("Failed to read template configuration '{}'", config_path.display())
    })?;

    let document: DocumentMut = contents
        .parse()
        .with_context(|| format!("Invalid template configuration '{}'", config_path.display()))?;

    if let Some(ignore) = document
        .get("template")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("ignore"))
        .and_then(|item| item.as_array())
    {
        for value in ignore {
            if let Some(value) = value.as_str() {
                config.ignore.push(value.to_string());
            }
        }
    }

    Ok(config)
}

fn should_ignore(relative_path: &Path, config: &TemplateConfig) -> bool {
    config.ignore.iter().any(|pattern| {
        let pattern_path = Path::new(pattern);
        relative_path.starts_with(pattern_path)
    })
}

fn sanitize_crate_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            'a'..='z' | '0'..='9' => result.push(ch),
            'A'..='Z' => result.push(ch.to_ascii_lowercase()),
            '-' | ' ' | '.' => {
                result.push('_');
            }
            '_' => result.push('_'),
            _ => result.push('_'),
        }
    }
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{result}")
    } else if result.is_empty() {
        "_".into()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn crate_name_is_sanitized() {
        assert_eq!(sanitize_crate_name("hello-world"), "hello_world");
        assert_eq!(sanitize_crate_name("HelloWorld"), "helloworld");
        assert_eq!(sanitize_crate_name("123abc"), "_123abc");
        assert_eq!(sanitize_crate_name("with spaces"), "with_spaces");
        assert_eq!(sanitize_crate_name("already_ok"), "already_ok");
        assert_eq!(sanitize_crate_name("@invalid!"), "_invalid_");
    }

    #[test]
    fn generate_local_template_renders_all_variables() -> Result<()> {
        let template_dir = tempdir()?;
        let template_root = template_dir.path().join("template");
        fs::create_dir_all(&template_root)?;

        fs::write(
            template_root.join("Cargo.toml"),
            r#"# crate={{crate_name}}
name = "{{project_name}}"
package = "miden:{{project-name}}""#,
        )?;

        let destination_dir = tempdir()?;
        let args = GenerateArgs {
            template_path: TemplatePath {
                path: Some(template_dir.path().to_string_lossy().into_owned()),
                ..Default::default()
            },
            destination: Some(destination_dir.path().to_path_buf()),
            name: Some("demo-project".into()),
            force: true,
            ..Default::default()
        };

        let project_dir = generate(args)?;
        let rendered = fs::read_to_string(project_dir.join("Cargo.toml"))?;

        assert!(rendered.contains("crate=demo_project"));
        assert!(rendered.contains("name = \"demo-project\""));
        assert!(rendered.contains("miden:demo-project"));

        Ok(())
    }

    #[test]
    fn generate_supports_auto_path_and_template_subdir() -> Result<()> {
        let repo_dir = tempdir()?;
        let nested = repo_dir.path().join("nested").join("template");
        fs::create_dir_all(&nested)?;
        fs::write(nested.join("README.md"), "{{project_name}}")?;

        let destination_dir = tempdir()?;
        let args = GenerateArgs {
            template_path: TemplatePath {
                path: Some(repo_dir.path().to_string_lossy().into_owned()),
                auto_path: Some("nested".into()),
                ..Default::default()
            },
            destination: Some(destination_dir.path().to_path_buf()),
            name: Some("auto_case".into()),
            force: true,
            ..Default::default()
        };

        let project_dir = generate(args)?;
        let rendered = fs::read_to_string(project_dir.join("README.md"))?;
        assert!(rendered.contains("auto_case"));

        Ok(())
    }

    #[test]
    fn generate_respects_cargo_generate_ignore_entries() -> Result<()> {
        let template_dir = tempdir()?;
        let template_root = template_dir.path().join("template");
        fs::create_dir_all(template_root.join("skip-me"))?;
        fs::create_dir_all(template_root.join("keep-me"))?;

        fs::write(
            template_root.join("cargo-generate.toml"),
            r#"[template]
ignore = ["skip-me"]
"#,
        )?;

        fs::write(template_root.join("keep-me").join("file.txt"), "keep")?;

        let destination_dir = tempdir()?;
        let args = GenerateArgs {
            template_path: TemplatePath {
                path: Some(template_dir.path().to_string_lossy().into_owned()),
                ..Default::default()
            },
            destination: Some(destination_dir.path().to_path_buf()),
            name: Some("ignore-check".into()),
            force: true,
            ..Default::default()
        };

        let project_dir = generate(args)?;

        assert!(project_dir.join("keep-me").join("file.txt").exists());
        assert!(!project_dir.join("skip-me").exists());

        Ok(())
    }

    #[test]
    fn generate_requires_force_for_non_empty_destination() -> Result<()> {
        let template_dir = tempdir()?;
        let template_root = template_dir.path().join("template");
        fs::create_dir_all(&template_root)?;
        fs::write(template_root.join("file.txt"), "content")?;

        let destination_dir = tempdir()?;
        let project_dir = destination_dir.path().join("existing");
        fs::create_dir_all(&project_dir)?;
        fs::write(project_dir.join("keep.txt"), "keep")?;

        let args = GenerateArgs {
            template_path: TemplatePath {
                path: Some(template_dir.path().to_string_lossy().into_owned()),
                ..Default::default()
            },
            destination: Some(destination_dir.path().to_path_buf()),
            name: Some("existing".into()),
            force: false,
            ..Default::default()
        };

        let err = generate(args).expect_err("expected failure without --force");
        assert!(err.to_string().contains("Use --force to overwrite"));

        Ok(())
    }

    #[test]
    fn parse_define_rejects_invalid_inputs() {
        assert!(parse_define("missing_value").is_err());
        assert!(parse_define("=value").is_err());
        assert!(parse_define("").is_err());
    }
}
