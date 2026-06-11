#[cfg(feature = "std")]
use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[cfg(feature = "std")]
use midenc_hir::formatter::DisplayMany;
#[cfg(feature = "std")]
use midenc_session::{Options, Session};

use super::*;

/// Parses single-file Rust inputs and extracts Wasm input for next stage
pub struct ParseRustStage;

impl Stage for ParseRustStage {
    type Input = InputFile;
    type Output = InputFile;

    #[cfg(not(feature = "std"))]
    fn run(&mut self, _input: Self::Input, _context: Rc<Context>) -> CompilerResult<Self::Output> {
        Err(Report::msg("compilation of Rust sources in no-std builds is unsupported"))
    }

    #[cfg(feature = "std")]
    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let file_type = input.file_type();
        if !matches!(input.file_type(), midenc_session::FileType::Rust) {
            return Err(Report::msg(format!(
                "invalid input file: expected '.rs', got {file_type}"
            )));
        }
        let session = context.session();
        let options = &session.options;
        let dependencies = if options.cargo_frontmatter {
            match &input.file {
                InputType::Real(path) => {
                    let working_dir = if path.is_absolute() {
                        path.parent().unwrap().to_path_buf()
                    } else {
                        let path = path.canonicalize().map_err(|err| {
                            Report::msg(format!("unable to canonicalize input file path: {err}"))
                        })?;
                        path.parent().unwrap().to_path_buf()
                    };
                    let input = std::fs::read_to_string(path)
                        .map_err(|err| Report::msg(format!("unable to read input: {err}")))?;
                    crate::cargo::parse_cargo_frontmatter(&input, &working_dir)?
                }
                InputType::Stdin { input, .. } => {
                    let input = core::str::from_utf8(input)
                        .map_err(|err| Report::msg(format!("input is not valid utf-8: {err}")))?;
                    let working_dir = std::env::current_dir().map_err(|err| {
                        Report::msg(format!("unable to obtain current working directory: {err}"))
                    })?;
                    crate::cargo::parse_cargo_frontmatter(input, &working_dir)?
                }
            }
        } else {
            None
        };
        let use_cargo = dependencies.is_some()
            || options.target_requires_protocol()
            || options.link_libraries.iter().any(|lib| lib.is_protocol());
        match &input.file {
            InputType::Real(path) if use_cargo => {
                let filename = path
                    .file_name()
                    .ok_or_else(|| Report::msg("invalid input path: not a valid file name"))?;
                let project_dir = prepare_temporary_cargo_project(path, filename, session)?;
                cargo_build(&project_dir, filename, dependencies, session, options)
            }
            InputType::Real(path) => rustc(path, None, context.session(), options),
            InputType::Stdin {
                name: filename,
                input,
            } => {
                let tmp = std::env::temp_dir();
                let name = context.session().project.package().name().into_inner();
                if use_cargo {
                    let project_dir = tmp.join(&*name);
                    let src_dir = project_dir.join("src");
                    std::fs::create_dir_all(&src_dir).map_err(|err| {
                        Report::msg(format!("failed to create temporary Cargo project: {err}"))
                    })?;
                    let filename = format!("{}.rs", filename.file_stem().unwrap_or("lib"));
                    let tmp_rs = src_dir.join(&filename);
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    cargo_build(
                        &project_dir,
                        Path::new(&filename).as_os_str(),
                        dependencies,
                        session,
                        options,
                    )
                } else {
                    let tmp_rs = tmp.join(&*name).with_extension("rs");
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    rustc(&tmp_rs, Some(&tmp), session, options)
                }
            }
            #[cfg(not(feature = "std"))]
            _ => Err(Report::msg("compilation of Rust sources in no-std builds is unsupported")),
        }
    }
}

/// Creates a temporary Cargo project structure for the Rust source file at `path`
///
/// NOTE: This does not write the `Cargo.toml` file - that is handled by the caller, depending on
/// how the Cargo metadata is derived.
#[cfg(feature = "std")]
fn prepare_temporary_cargo_project(
    path: &Path,
    filename: &OsStr,
    session: &Session,
) -> CompilerResult<PathBuf> {
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    let name = session.project.package().name().into_inner();
    let project_dir = tmp.join(&*name);
    let src_dir = project_dir.join("src");
    let tmp_rs = src_dir.join(filename);
    std::fs::create_dir_all(&src_dir)
        .map_err(|err| Report::msg(format!("failed to create temporary Cargo project: {err}")))?;
    std::fs::copy(path, &tmp_rs).map_err(|err| {
        Report::msg(format!("failed to copy input file to temporary Cargo project: {err}"))
    })?;
    Ok(project_dir)
}

#[cfg(feature = "std")]
fn cargo_build(
    project_dir: &Path,
    filename: &OsStr,
    frontmatter_dependencies: Option<toml_edit::Table>,
    session: &Session,
    options: &Options,
) -> CompilerResult<InputFile> {
    let package = session.project.package();
    let package_name = package.name().into_inner();
    let package_version = package.version().into_inner();

    let mut dependencies = frontmatter_dependencies.unwrap_or_default();
    let requires_protocol = options.target_requires_protocol();
    if let Ok(path) = std::env::var("MIDENC_SOURCE_TREE")
        && std::env::var("MIDENC_LINK_FROM_SOURCE_TREE").is_ok_and(|v| v == "1")
    {
        let path = Path::new(&path);
        if !dependencies.contains_key("miden-sdk-alloc") {
            let alloc_path = path.join("sdk/alloc");
            dependencies.insert("miden-sdk-alloc", dependency_path_to_toml_item(&alloc_path));
        }
        if requires_protocol && !dependencies.contains_key("miden") {
            let miden_path = path.join("sdk/sdk");
            dependencies.insert("miden", dependency_path_to_toml_item(&miden_path));
        } else if !requires_protocol && !dependencies.contains_key("miden-stdlib-sys") {
            let stdlib_sys_path = path.join("sdk/stdlib-sys");
            dependencies.insert("miden-stdlib-sys", dependency_path_to_toml_item(&stdlib_sys_path));
        }
    } else {
        if !dependencies.contains_key("miden-sdk-alloc") {
            dependencies.insert("miden-sdk-alloc", str_to_toml_item("*"));
        }
        let requires_protocol = options.target_requires_protocol();
        if requires_protocol && !dependencies.contains_key("miden") {
            dependencies.insert("miden", str_to_toml_item("*"));
        } else if !requires_protocol && !dependencies.contains_key("miden-stdlib-sys") {
            dependencies.insert("miden-stdlib-sys", str_to_toml_item("*"));
        }
    }

    let cargo_toml = format!(
        "\
cargo-features = [\"trim-paths\"]

[package]
name = \"{package_name}\"
version = \"{package_version}\"
edition = \"2024\"
authors = []

[dependencies]
{dependencies}

[lib]
crate-type = [\"cdylib\"]
path = \"src/{filename}\"

[profile.release]
panic = \"abort\"
# optimize for size
opt-level = \"s\"
debug = true
trim-paths = [\"diagnostics\", \"object\"]
",
        filename = filename.display(),
    );

    let manifest_path = project_dir.join("Cargo.toml");
    std::fs::write(&manifest_path, &cargo_toml)
        .map_err(|err| Report::msg(format!("failed to generate temporary Cargo.toml: {err}")))?;

    let cargo_build_args = build_cargo_args(&manifest_path, options);

    // Enable memcopy and 128-bit arithmetic ops
    let mut rustflags = String::from("-C target-feature=+bulk-memory,+wide-arithmetic");
    // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
    // cfg-based dependency selection.
    rustflags.push_str(" --cfg miden");
    // Enable errors on missing stub functions
    rustflags.push_str(" -C link-args=--fatal-warnings");
    // Remove the source file paths in the data segment for panics
    // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
    rustflags.push_str(" -Zlocation-detail=none");
    // Build with panic=immediate-abort
    rustflags.push_str(" -Zunstable-options");
    rustflags.push_str(" -Cpanic=immediate-abort");
    if let Ok(inherited) = std::env::var("RUSTFLAGS")
        && !inherited.is_empty()
    {
        rustflags.push(' ');
        rustflags.push_str(&inherited);
    }
    if let Some(explicit) = options.rustflags.as_deref() {
        rustflags.push(' ');
        rustflags.push_str(explicit);
    }

    let wasi = if options.target_requires_protocol() {
        "wasip2"
    } else {
        "wasip1"
    };

    let cargo_env = std::env::var("CARGO").map(PathBuf::from).ok();
    let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));

    let mut cargo = std::process::Command::new(cargo_path);
    // Ensure we specify the nightly toolchain if a specific cargo wasn't set. An inherited
    // RUSTUP_TOOLCHAIN (e.g. from the rust-toolchain.toml override of the invoking project) takes
    // precedence: forcing the generic `nightly` channel would bypass that pin, and the build
    // below runs from a temporary directory where directory-based overrides do not apply.
    if cargo_env.is_none() && std::env::var_os("RUSTUP_TOOLCHAIN").is_none() {
        cargo.arg("+nightly");
    }
    cargo.env("RUSTFLAGS", rustflags);
    // This env var is used by crates (e.g. `miden-field`) to distinguish compiling to Wasm for a
    // "real" Wasm runtime vs compiling to Wasm as an intermediate artifact that will be compiled
    // to Miden VM code by `midenc`.
    cargo.env("MIDENC_TARGET_IS_MIDEN_VM", "1");
    cargo.args(&cargo_build_args);

    // Handle the target for buildable commands
    crate::rust::install_wasm32_target(
        wasi,
        if cargo_env.is_none() {
            Some("nightly")
        } else {
            None
        },
    )?;

    cargo.arg("--target").arg(format!("wasm32-{wasi}"));

    // It will output the message as json so we can extract the wasm files that will be
    // componentized
    cargo.arg("--message-format").arg("json-render-diagnostics");
    cargo.stdout(std::process::Stdio::piped());
    cargo.stderr(std::process::Stdio::inherit());

    let artifacts = crate::rust::spawn_cargo(cargo, cargo_path)?;

    let mut outputs: Vec<PathBuf> = artifacts
        .into_iter()
        .flat_map(|a| a.filenames)
        .filter_map(|path| {
            if path.extension().is_some_and(|ext| ext == "wasm") {
                Some(path.into_std_path_buf())
            } else {
                None
            }
        })
        .collect();

    // We expect just a single artifact named `{package_name}.wasm`
    if outputs.len() != 1 {
        Err(Report::msg(format!(
            "expected `cargo build` to produce a single artifact, got: {outputs:#?}"
        )))
    } else {
        Ok(InputFile::from_path(outputs.pop().unwrap())
            .expect("wasm is always a valid input file type"))
    }
}

#[cfg(feature = "std")]
fn rustc(
    input: &Path,
    tmp_dir: Option<&Path>,
    session: &Session,
    options: &Options,
) -> CompilerResult<InputFile> {
    use std::string::ToString;

    let package_name = session.project.package().name().into_inner();

    log::debug!(target: "rustc", "preparing to invoke rustc for {package_name}");
    log::debug!(target: "rustc", "  current_dir = {}", options.current_dir.display());
    log::debug!(target: "rustc", "  target_dir  = {}", options.target_dir.display());
    log::debug!(target: "rustc", "  tmp_dir     = {}", tmp_dir.unwrap_or(Path::new("unknown")).display());

    // Output is the same name as the input, just with a different extension
    let output_file = options.target_dir.join(format!("{package_name}.wasm"));

    // Set up the command used to compile the test inputs (typically Rust -> Wasm)
    let mut command = std::process::Command::new("rustc");
    // Pipe output of command to terminal
    if options.diagnostics.is_verbose() {
        command.stdout(std::process::Stdio::inherit());
        command.stderr(std::process::Stdio::inherit());
    } else {
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
    }

    // If `RUSTFLAGS` is present, convert them to `rustc` flags
    let mut rustflags = std::env::var("RUSTFLAGS").ok().unwrap_or_default();
    if let Some(explicit) = options.rustflags.as_deref() {
        if !rustflags.is_empty() {
            rustflags.push(' ');
        }
        rustflags.push_str(explicit);
    }
    let rustflags = rustflags.split_ascii_whitespace().collect::<Vec<_>>();
    let mut rustc_flags = Vec::with_capacity(rustflags.len());
    let mut rustflags = rustflags.into_iter();
    let mut target = None;
    while let Some(flag) = rustflags.next() {
        if flag == "--target"
            && let Some(value) = rustflags.next()
        {
            target = Some(value.to_string());
            continue;
        } else if flag == "-C"
            && let Some(value) = rustflags.next()
        {
            if value == "panic=immediate-abort" {
                continue;
            }
            rustc_flags.extend([flag, value]);
        } else {
            rustc_flags.push(flag);
        }
    }

    let mut command = command;
    command
        .arg("--crate-name")
        .arg(package_name.replace("-", "_"))
        .args(["--crate-type", "cdylib"])
        .args(["--edition", "2024"])
        // Propagate the Miden VM target signal to the entire crate graph so Cargo can use it for
        // cfg-based dependency selection.
        .args(["--cfg", "miden"])
        // Enable errors on missing stub functions
        .args(["-C", "link-args=--fatal-warnings"])
        .arg("--remap-path-scope=diagnostics,debuginfo,coverage,object")
        .arg("--remap-path-prefix")
        .arg(format!("{}=.", options.current_dir.display()));
    for remap_prefix in options.remap_path_prefixes.iter() {
        command.args([
            "--remap-path-prefix".into(),
            format!(
                "{}={}",
                remap_prefix.source_prefix().display(),
                remap_prefix.target_prefix().display()
            ),
        ]);
    }
    if let Some(tmp_dir) = tmp_dir {
        command.arg("--remap-path-prefix").arg(format!("{}=.", tmp_dir.display()));
    }
    command
        .args(["-Z", "unstable-options"])
        // Remove the source file paths in the data segment for panics
        // https://doc.rust-lang.org/beta/unstable-book/compiler-flags/location-detail.html
        .args(["-Z", "location-detail=none"])
        .arg("-g") // generate debug info
        .args(["-C", "opt-level=s"]) // optimize for size
        .args(["-C", "target-feature=+wide-arithmetic"])
        .args(rustc_flags)
        .arg("--target")
        .arg(target.as_deref().unwrap_or("wasm32-wasip1"))
        .arg("-o")
        .arg(&output_file)
        .arg(input);
    log::debug!(target: "rustc", "executing `{} {}`", command.get_program().display(),
        DisplayMany::new(command.get_args().map(|arg| arg.display()), " ")
    );
    let output = command
        .output()
        .map_err(|err| Report::msg(format!("failed to execute `rustc`: {err}")))?;
    if !output.status.success() {
        return Err(Report::msg(
            "`rustc` returned an error when compiling the input, see stderr output for more \
             details",
        ));
    }

    Ok(InputFile::from_path(output_file).expect("wasm is always a valid output"))
}

#[cfg(feature = "std")]
fn build_cargo_args(manifest_path: &Path, options: &Options) -> Vec<String> {
    let mut args = vec!["build".to_string()];

    // Add build-std flags required for Miden compilation
    args.extend(
        [
            "-Z",
            "build-std=core,alloc,panic_abort",
            "-Z",
            "build-std-features=optimize_for_size",
        ]
        .into_iter()
        .map(|s| s.to_string()),
    );

    // Configure profile settings
    let cfg_pairs: Vec<(&str, &str)> = vec![
        ("profile.dev.panic", "\"abort\""),
        ("profile.dev.opt-level", "1"),
        ("profile.dev.overflow-checks", "false"),
        ("profile.dev.debug", "true"),
        ("profile.dev.debug-assertions", "false"),
        ("profile.release.opt-level", "\"s\""),
        ("profile.release.lto", "true"),
        ("profile.release.codegen-units", "1"),
        ("profile.release.panic", "\"abort\""),
    ];

    for (key, value) in cfg_pairs {
        args.push("--config".to_string());
        args.push(format!("{key}={value}"));
    }

    // Forward cargo-specific options
    if options.profile == "release" {
        args.push("--release".to_string());
    }

    args.push("--manifest-path".to_string());
    args.push(manifest_path.to_string_lossy().to_string());

    if options.workspace {
        args.push("--workspace".to_string());
    }

    for package in &options.packages {
        args.push("--package".to_string());
        args.push(package.to_string());
    }

    args
}

#[cfg(feature = "std")]
fn dependency_path_to_toml_item(path: &std::path::Path) -> toml_edit::Item {
    use toml_edit::*;

    let mut table = Table::new();
    table.insert("path", Item::Value(Value::String(Formatted::new(path.display().to_string()))));
    Item::Table(table)
}

#[cfg(feature = "std")]
fn str_to_toml_item(s: &str) -> toml_edit::Item {
    use toml_edit::*;

    Item::Value(Value::String(Formatted::new(s.to_string())))
}
