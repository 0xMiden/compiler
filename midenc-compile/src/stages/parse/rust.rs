#[cfg(feature = "std")]
use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

#[cfg(feature = "std")]
use midenc_session::{Options, Session};

use super::*;

/// Parses single-file Rust inputs and extracts Wasm input for next stage
pub struct ParseRustStage;

impl Stage for ParseRustStage {
    type Input = InputFile;
    type Output = InputFile;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let file_type = input.file_type();
        if !matches!(input.file_type(), midenc_session::FileType::Rust) {
            return Err(Report::msg(format!(
                "invalid input file: expected '.rs', got {file_type}"
            )));
        }
        let options = &context.session().options;
        let use_cargo = options.target_requires_protocol()
            || options.link_libraries.iter().any(|lib| {
                lib.name == "std"
                    || lib.name == "core"
                    || lib.name == "protocol"
                    || lib.name == "standards"
            });
        match &input.file {
            #[cfg(feature = "std")]
            InputType::Real(path) if use_cargo => {
                let tmp = std::env::temp_dir();
                let name = context.session().project.package().name().into_inner();
                let project_dir = tmp.join(&*name);
                let src_dir = project_dir.join("src");
                let tmp_rs = src_dir.join(if options.target_type.is_executable() {
                    "main.rs"
                } else {
                    "lib.rs"
                });
                std::fs::create_dir_all(&src_dir).map_err(|err| {
                    Report::msg(format!("failed to create temporary Cargo project: {err}"))
                })?;
                std::fs::copy(path, &tmp_rs).map_err(|err| {
                    Report::msg(format!(
                        "failed to copy input file to temporary Cargo project: {err}"
                    ))
                })?;
                cargo_build(&project_dir, context.session(), options)
            }
            #[cfg(feature = "std")]
            InputType::Real(path) => rustc(path, context.session(), options),
            #[cfg(feature = "std")]
            InputType::Stdin { input, .. } => {
                let tmp = std::env::temp_dir();
                let name = context.session().project.package().name().into_inner();
                if use_cargo {
                    let project_dir = tmp.join(&*name);
                    let src_dir = project_dir.join("src");
                    std::fs::create_dir_all(&src_dir).map_err(|err| {
                        Report::msg(format!("failed to create temporary Cargo project: {err}"))
                    })?;
                    let tmp_rs = src_dir.join(if options.target_type.is_executable() {
                        "main.rs"
                    } else {
                        "lib.rs"
                    });
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    cargo_build(&project_dir, context.session(), options)
                } else {
                    let tmp_rs = tmp.join(&*name).with_extension("rs");
                    std::fs::write(&tmp_rs, input).map_err(|err| {
                        Report::msg(format!("failed to write Rust input to temporary file: {err}"))
                    })?;
                    rustc(&tmp_rs, context.session(), options)
                }
            }
            #[cfg(not(feature = "std"))]
            _ => Err(Report::msg("compilation of Rust sources in no-std builds is unsupported")),
        }
    }
}

#[cfg(feature = "std")]
fn cargo_build(
    project_dir: &Path,
    session: &Session,
    options: &Options,
) -> CompilerResult<InputFile> {
    use core::fmt::Write;

    let package = session.project.package();
    let package_name = package.name().into_inner();
    let package_version = package.version().into_inner();

    let mut dependencies = String::with_capacity(1024);
    if let Ok(path) = std::env::var("MIDENC_SOURCE_TREE")
        && std::env::var("MIDENC_LINK_FROM_SOURCE_TREE").is_ok_and(|v| v == "1")
    {
        let path = Path::new(&path);
        let alloc_path = path.join("sdk/alloc");
        writeln!(&mut dependencies, "miden-sdk-alloc = {{ path = \"{}\" }}", alloc_path.display())
            .unwrap();
        if options.target_requires_protocol() {
            let miden_path = path.join("sdk/sdk");
            writeln!(&mut dependencies, "miden = {{ path = \"{}\" }}", miden_path.display())
                .unwrap();
        } else {
            let stdlib_sys_path = path.join("sdk/stdlib-sys");
            writeln!(
                &mut dependencies,
                "miden-stdlib-sys = {{ path = \"{}\" }}",
                stdlib_sys_path.display()
            )
            .unwrap();
        }
    } else {
        writeln!(&mut dependencies, "miden-sdk-alloc = \"*\"").unwrap();
        if options.target_requires_protocol() {
            writeln!(&mut dependencies, "miden = \"*\"").unwrap();
        } else {
            writeln!(&mut dependencies, "miden-stdlib-sys = \"*\"").unwrap();
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

[profile.release]
panic = \"abort\"
# optimize for size
opt-level = \"s\"
debug = true
trim-paths = [\"diagnostics\", \"object\"]
"
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

    let wasi = if options.target_requires_protocol() {
        "wasip2"
    } else {
        "wasip1"
    };

    let cargo_env = std::env::var("CARGO").map(PathBuf::from).ok();
    let cargo_path = cargo_env.as_deref().unwrap_or_else(|| Path::new("cargo"));

    let mut cargo = std::process::Command::new(cargo_path);
    // Ensure we specify the nightly toolchain if a specific cargo wasn't set
    if cargo_env.is_none() {
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
fn rustc(input: &Path, session: &Session, options: &Options) -> CompilerResult<InputFile> {
    use std::string::ToString;

    let package_name = session.project.package().name().into_inner();

    // Output is the same name as the input, just with a different extension
    let output_file = options.target_dir.join(format!("{package_name}.wasm"));

    // Set up the command used to compile the test inputs (typically Rust -> Wasm)
    let mut command = std::process::Command::new("rustc");
    // Pipe output of command to terminal
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    // If `RUSTFLAGS` is present, convert them to `rustc` flags
    let rustflags = std::env::var("RUSTFLAGS").ok().unwrap_or_default();
    let rustflags = rustflags.split(' ').collect::<Vec<_>>();
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

    let output = command
        .arg("--crate-name")
        .arg(&*package_name)
        .args(["--crate-type", "cdylib"])
        .args(["--edition", "2024"])
        .arg("--remap-path-prefix")
        .arg(format!("{}=.", options.current_dir.display()))
        .arg("-g") // generate debug info
        .args(["-C", "opt-level=s"]) // optimize for size
        .args(["-C", "target-feature=+wide-arithmetic"])
        .args(rustc_flags)
        .arg("--target")
        .arg(target.as_deref().unwrap_or("wasm32-wasip1"))
        .arg("-o")
        .arg(&output_file)
        .arg(input)
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
