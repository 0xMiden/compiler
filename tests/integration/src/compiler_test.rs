use core::panic;
use std::{
    borrow::Cow,
    ffi::OsStr,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::Arc,
};

use miden_assembly::LibraryPath;
use midenc_compile::{
    compile_link_output_to_masm_with_pre_assembly_stage, compile_to_unoptimized_hir,
};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{
    demangle::demangle, dialects::builtin, interner::Symbol, Context, FunctionIdent, Ident, Op,
};
use midenc_session::{InputFile, InputType, Session};

use crate::{
    cargo_proj::project,
    testing::{format_report, setup},
};

type LinkMasmModules = Vec<(LibraryPath, String)>;

/// Configuration for tests which use as input, the artifact produced by a Cargo build
pub struct CargoTest {
    project_dir: PathBuf,
    manifest_path: Option<Cow<'static, str>>,
    target_dir: Option<PathBuf>,
    name: Cow<'static, str>,
    target: Cow<'static, str>,
    entrypoint: Option<Cow<'static, str>>,
    build_std: bool,
    build_alloc: bool,
}
impl CargoTest {
    /// Create a new `cargo` test with the given name, and project directory
    pub fn new(name: impl Into<Cow<'static, str>>, project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            manifest_path: None,
            target_dir: None,
            name: name.into(),
            target: "wasm32-wasip1".into(),
            entrypoint: None,
            build_std: false,
            build_alloc: false,
        }
    }

    /// Specify whether to build the entire standard library as part of the crate graph
    #[inline]
    pub fn with_build_std(mut self, build_std: bool) -> Self {
        self.build_std = build_std;
        self
    }

    /// Specify whether to build libcore and liballoc as part of the crate graph (implied by
    /// `with_build_std`)
    #[inline]
    pub fn with_build_alloc(mut self, build_alloc: bool) -> Self {
        self.build_alloc = build_alloc;
        self
    }

    /// Specify the target triple to pass to Cargo
    #[inline]
    pub fn with_target(mut self, target: impl Into<Cow<'static, str>>) -> Self {
        self.target = target.into();
        self
    }

    /// Specify the target directory for Cargo
    #[inline]
    pub fn with_target_dir(mut self, target_dir: impl Into<PathBuf>) -> Self {
        self.target_dir = Some(target_dir.into());
        self
    }

    /// Specify the name of the entrypoint function (just the function name, no namespace)
    #[inline]
    pub fn with_entrypoint(mut self, entrypoint: impl Into<Cow<'static, str>>) -> Self {
        self.entrypoint = Some(entrypoint.into());
        self
    }

    /// Override the Cargo manifest path
    #[inline]
    pub fn with_manifest_path(mut self, manifest_path: impl Into<Cow<'static, str>>) -> Self {
        self.manifest_path = Some(manifest_path.into());
        self
    }

    /// Get a [PathBuf] representing the path to the expected Cargo artifact
    pub fn wasm_artifact_path(&self) -> PathBuf {
        self.project_dir
            .join("target")
            .join(self.target.as_ref())
            .join("release")
            .join(self.name.as_ref())
            .with_extension("wasm")
    }
}

/// Configuration for tests which use as input, the artifact produced by an invocation of `rustc`
pub struct RustcTest {
    target_dir: Option<PathBuf>,
    name: Cow<'static, str>,
    target: Cow<'static, str>,
    #[allow(dead_code)]
    output_name: Option<Cow<'static, str>>,
    source_code: Cow<'static, str>,
    rustflags: Vec<Cow<'static, str>>,
}
impl RustcTest {
    /// Construct a new `rustc` input with the given name and source code content
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        source_code: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            target_dir: None,
            name: name.into(),
            target: "wasm32-unknown-unknown".into(),
            output_name: None,
            source_code: source_code.into(),
            // Always use spec-compliant C ABI behavior
            rustflags: vec!["-Z".into(), "wasm_c_abi=spec".into()],
        }
    }
}

/// The various types of input artifacts that can be used to drive compiler tests
pub enum CompilerTestInputType {
    /// A project that uses `cargo miden build` to produce a Wasm module to use as input
    CargoMiden(CargoTest),
    /// A project that uses `cargo build` to produce a core Wasm module to use as input
    Cargo(CargoTest),
    /// A project that uses `rustc` to produce a core Wasm module to use as input
    Rustc(RustcTest),
}
impl From<CargoTest> for CompilerTestInputType {
    fn from(config: CargoTest) -> Self {
        Self::Cargo(config)
    }
}
impl From<RustcTest> for CompilerTestInputType {
    fn from(config: RustcTest) -> Self {
        Self::Rustc(config)
    }
}

/// [CompilerTestBuilder] is used to obtain a [CompilerTest], and subsequently run that test.
///
/// Testing the compiler involves orchestrating a number of complex components. First, we must
/// obtain the input we wish to feed into `midenc` for the test. Typically, we have some Rust
/// source code, or a Cargo project, and we must compile that first, in order to get the Wasm
/// module/component which will be passed to `midenc`. This first phase requires some configuration,
/// and that configuration affects later phases (such as the name of the artifact produced).
///
/// Secondly, we need to prepare the [midenc_session::Session] object for the compiler. This is
/// where we specify inputs, and various bits of configuration that are important to the test, or
/// which are needed in order to obtain useful diagnostic output. This phase requires us to
/// construct the base configuration here, but make it possible to extend/alter in each specific
/// test.
///
/// Lastly, we must run the test, and in order to do this, we must know where our inputs and outputs
/// are, so that we can fetch files/data/etc. as needed; know the names of things to be called, and
/// more.
pub struct CompilerTestBuilder {
    /// The Wasm translation configuration
    config: WasmTranslationConfig,
    /// The source code used to compile the test
    source: CompilerTestInputType,
    /// The entrypoint function to use when building the IR
    entrypoint: Option<FunctionIdent>,
    /// The extra MASM modules to link to the compiled MASM program
    link_masm_modules: LinkMasmModules,
    /// Extra flags to pass to the midenc driver
    midenc_flags: Vec<Cow<'static, str>>,
    /// Extra RUSTFLAGS to set when compiling Rust code
    rustflags: Vec<Cow<'static, str>>,
    /// The cargo workspace directory of the compiler
    #[allow(dead_code)]
    workspace_dir: String,
}
impl CompilerTestBuilder {
    /// Construct a new [CompilerTestBuilder] for the given source type configuration
    pub fn new(source: impl Into<CompilerTestInputType>) -> Self {
        setup::enable_compiler_instrumentation();

        let workspace_dir = get_workspace_dir();
        let mut source = source.into();
        let mut rustflags = match source {
            CompilerTestInputType::Rustc(ref mut config) => core::mem::take(&mut config.rustflags),
            _ => vec![],
        };
        let entrypoint = match source {
            CompilerTestInputType::Cargo(ref mut config) => config.entrypoint.take(),
            CompilerTestInputType::Rustc(_) => Some("__main".into()),
            CompilerTestInputType::CargoMiden(ref mut config) => config.entrypoint.take(),
        };
        let name = match source {
            CompilerTestInputType::Cargo(ref mut config) => config.name.as_ref(),
            CompilerTestInputType::Rustc(ref mut config) => config.name.as_ref(),
            CompilerTestInputType::CargoMiden(ref mut config) => config.name.as_ref(),
        };
        let entrypoint = entrypoint.as_deref().map(|entry| FunctionIdent {
            module: Ident::with_empty_span(Symbol::intern(name)),
            function: Ident::with_empty_span(Symbol::intern(entry)),
        });
        rustflags.extend([
            // Enable bulk-memory features (e.g. native memcpy/memset instructions)
            "-C".into(),
            "target-feature=+bulk-memory".into(),
            // Remap the compiler workspace to `.` so that build outputs do not embed user-
            // specific paths, which would cause expect tests to break
            "--remap-path-prefix".into(),
            format!("{workspace_dir}=../../").into(),
        ]);
        let mut midenc_flags = vec!["--debug".into(), "--verbose".into()];
        if let Some(entrypoint) = entrypoint {
            midenc_flags
                .extend(["--entrypoint".into(), format!("{}", entrypoint.display()).into()]);
        }
        Self {
            config: Default::default(),
            source,
            entrypoint,
            link_masm_modules: vec![],
            midenc_flags,
            rustflags,
            workspace_dir,
        }
    }

    /// Override the default [WasmTranslationConfig] for the test
    pub fn with_wasm_translation_config(&mut self, config: WasmTranslationConfig) -> &mut Self {
        self.config = config;
        self
    }

    /// Specify the entrypoint function to call during the test
    pub fn with_entrypoint(&mut self, entrypoint: FunctionIdent) -> &mut Self {
        match self.entrypoint.replace(entrypoint) {
            Some(prev) if prev == entrypoint => return self,
            Some(prev) => {
                // Remove the previous --entrypoint ID flag
                let index = self
                    .midenc_flags
                    .iter()
                    .position(|flag| flag == "--entrypoint")
                    .unwrap_or_else(|| {
                        panic!(
                            "entrypoint was changed from '{}' -> '{}', but previous entrypoint \
                             had been set without passing --entrypoint to midenc",
                            prev.display(),
                            entrypoint.display()
                        )
                    });
                self.midenc_flags.remove(index);
                self.midenc_flags.remove(index);
            }
            None => (),
        }
        self.midenc_flags
            .extend(["--entrypoint".into(), format!("{}", entrypoint.display()).into()]);
        self
    }

    /// Append additional `midenc` compiler flags
    pub fn with_midenc_flags(
        &mut self,
        flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> &mut Self {
        self.midenc_flags.extend(flags);
        self
    }

    /// Append additional flags to the value of `RUSTFLAGS` used when invoking `cargo` or `rustc`
    pub fn with_rustflags(
        &mut self,
        flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> &mut Self {
        self.rustflags.extend(flags);
        self
    }

    /// Add additional Miden Assembly module sources, to be linked with the program under test.
    pub fn link_with_masm_module(
        &mut self,
        fully_qualified_name: impl AsRef<str>,
        source: impl Into<String>,
    ) -> &mut Self {
        let name = fully_qualified_name.as_ref();
        let path = LibraryPath::new(name)
            .unwrap_or_else(|err| panic!("invalid miden assembly module name '{name}': {err}"));
        self.link_masm_modules.push((path, source.into()));
        self
    }

    /// Consume the builder, invoke any tools required to obtain the inputs for the test, and if
    /// successful, return a [CompilerTest], ready for evaluation.
    pub fn build(self) -> CompilerTest {
        // Set up the command used to compile the test inputs (typically Rust -> Wasm)
        let mut command = match self.source {
            CompilerTestInputType::CargoMiden(_) => {
                let mut cmd = Command::new("cargo");
                cmd.arg("miden").arg("build");
                cmd
            }
            CompilerTestInputType::Cargo(_) => {
                let mut cmd = Command::new("cargo");
                cmd.arg("build");
                cmd
            }
            CompilerTestInputType::Rustc(_) => Command::new("rustc"),
        };

        // Extract the directory in which source code is presumed to exist (or will be placed)
        let project_dir = match self.source {
            CompilerTestInputType::CargoMiden(CargoTest {
                ref project_dir, ..
            })
            | CompilerTestInputType::Cargo(CargoTest {
                ref project_dir, ..
            }) => Cow::Borrowed(project_dir.as_path()),
            CompilerTestInputType::Rustc(RustcTest { ref target_dir, .. }) => target_dir
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Owned(std::env::temp_dir())),
        };

        // Cargo-based source types share a lot of configuration in common
        match self.source {
            CompilerTestInputType::CargoMiden(ref config) => {
                let manifest_path = project_dir.join("Cargo.toml");
                command
                    .arg("--manifest-path")
                    .arg(manifest_path)
                    .arg("--release")
                    .arg("--target")
                    .arg(config.target.as_ref());
            }

            CompilerTestInputType::Cargo(ref config) => {
                let manifest_path = project_dir.join("Cargo.toml");
                command
                    .arg("--manifest-path")
                    .arg(manifest_path)
                    .arg("--release")
                    .arg("--target")
                    .arg(config.target.as_ref());

                if config.build_std {
                    // compile std as part of crate graph compilation
                    // https://doc.rust-lang.org/cargo/reference/unstable.html#build-std
                    command.arg("-Z").arg("build-std=core,alloc,std,panic_abort");

                    // abort on panic without message formatting (core::fmt uses call_indirect)
                    command.arg("-Z").arg("build-std-features=panic_immediate_abort");
                } else if config.build_alloc {
                    // compile libcore and liballoc as part of crate graph compilation
                    // https://doc.rust-lang.org/cargo/reference/unstable.html#build-std
                    command.arg("-Z").arg("build-std=core,alloc");

                    // abort on panic without message formatting (core::fmt uses call_indirect)
                    command.arg("-Z").arg("build-std-features=panic_immediate_abort");
                }

                // Render Cargo output as JSON
                command.arg("--message-format=json-render-diagnostics");
            }
            _ => (),
        }

        // All test source types support custom RUSTFLAGS
        if !self.rustflags.is_empty() {
            let mut flags = String::with_capacity(
                self.rustflags.iter().map(|flag| flag.len()).sum::<usize>() + self.rustflags.len(),
            );
            for (i, flag) in self.rustflags.iter().enumerate() {
                if i > 0 {
                    flags.push(' ');
                }
                flags.push_str(flag.as_ref());
            }
            command.env("RUSTFLAGS", flags);
        }

        // Pipe output of command to terminal
        command.stdout(Stdio::piped());

        // Build test
        match self.source {
            CompilerTestInputType::CargoMiden(config) => {
                let expected_wasm_artifact_path = config.wasm_artifact_path();
                let skip_rust_compilation =
                    std::env::var("SKIP_RUST").is_ok() && expected_wasm_artifact_path.exists();
                let wasm_artifact_path = if !skip_rust_compilation {
                    let mut args = vec![command.get_program().to_str().unwrap().to_string()];
                    let cmd_args: Vec<String> = command
                        .get_args()
                        .collect::<Vec<&OsStr>>()
                        .iter()
                        .map(|s| s.to_str().unwrap().to_string())
                        .collect();
                    args.extend(cmd_args);
                    let wasm_artifacts =
                        cargo_miden::run(args.into_iter(), cargo_miden::OutputType::Wasm).unwrap();
                    assert_eq!(
                        wasm_artifacts.len(),
                        1,
                        "expected one Wasm artifact, got {:?}",
                        wasm_artifacts
                    );
                    wasm_artifacts.first().unwrap().clone()
                } else {
                    drop(command);
                    expected_wasm_artifact_path
                };
                let artifact_name =
                    wasm_artifact_path.file_stem().unwrap().to_str().unwrap().to_string();
                let input_file = InputFile::from_path(wasm_artifact_path).unwrap();
                let mut inputs = vec![input_file];
                inputs.extend(self.link_masm_modules.into_iter().map(|(path, content)| {
                    let path = path.to_string();
                    InputFile::new(
                        midenc_session::FileType::Masm,
                        InputType::Stdin {
                            name: path.into(),
                            input: content.into_bytes(),
                        },
                    )
                }));
                // dbg!(&inputs);

                let context = setup::default_context(inputs, &self.midenc_flags);
                let session = context.session_rc();
                CompilerTest {
                    config: self.config,
                    session,
                    context,
                    artifact_name: artifact_name.into(),
                    entrypoint: self.entrypoint,
                    ..Default::default()
                }
            }

            CompilerTestInputType::Cargo(config) => {
                let expected_wasm_artifact_path = config.wasm_artifact_path();
                let skip_rust_compilation =
                    std::env::var("SKIP_RUST").is_ok() && expected_wasm_artifact_path.exists();
                let wasm_artifact_path = if !skip_rust_compilation {
                    let mut child = command.spawn().unwrap_or_else(|_| {
                        panic!(
                            "Failed to execute command: {}",
                            command
                                .get_args()
                                .map(|arg| format!("'{}'", arg.to_str().unwrap()))
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    });
                    // Find the Wasm artifacts from the cargo build output for debugging purposes
                    let mut wasm_artifacts = find_wasm_artifacts(&mut child);
                    let output = child.wait().expect("Couldn't get cargo's exit status");
                    if !output.success() {
                        report_cargo_error(child);
                    }
                    assert!(output.success());
                    // filter out dependencies
                    wasm_artifacts.retain(|path| {
                        let path_str = path.to_str().unwrap();
                        !path_str.contains("release/deps")
                    });
                    dbg!(&wasm_artifacts);
                    assert_eq!(wasm_artifacts.len(), 1, "Expected one Wasm artifact");
                    wasm_artifacts.swap_remove(0)
                } else {
                    drop(command);
                    expected_wasm_artifact_path
                };

                let input_file = InputFile::from_path(wasm_artifact_path).unwrap();
                let mut inputs = vec![input_file];
                inputs.extend(self.link_masm_modules.into_iter().map(|(path, content)| {
                    let path = path.to_string();
                    InputFile::new(
                        midenc_session::FileType::Masm,
                        InputType::Stdin {
                            name: path.into(),
                            input: content.into_bytes(),
                        },
                    )
                }));

                let context = setup::default_context(inputs, &self.midenc_flags);
                let session = context.session_rc();
                CompilerTest {
                    config: self.config,
                    session,
                    context,
                    artifact_name: config.name,
                    entrypoint: self.entrypoint,
                    ..Default::default()
                }
            }
            CompilerTestInputType::Rustc(config) => {
                // Ensure we have a fresh working directory prepared
                let working_dir = config
                    .target_dir
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join(config.name.as_ref()));
                if working_dir.exists() {
                    fs::remove_dir_all(&working_dir).unwrap();
                }
                fs::create_dir_all(&working_dir).unwrap();

                // Prepare inputs
                let basename = working_dir.join(config.name.as_ref());
                let input_file = basename.with_extension("rs");
                fs::write(&input_file, config.source_code.as_ref()).unwrap();

                // Output is the same name as the input, just with a different extension
                let output_file = basename.with_extension("wasm");

                let output = command
                    .args(["-C", "opt-level=z"]) // optimize for size
                    .arg("--target")
                    .arg(config.target.as_ref())
                    .arg("-o")
                    .arg(&output_file)
                    .arg(&input_file)
                    .output()
                    .expect("rustc invocation failed");
                if !output.status.success() {
                    eprintln!("pwd: {:?}", std::env::current_dir().unwrap());
                    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                    panic!("Rust to Wasm compilation failed!");
                }
                let input_file = InputFile::from_path(output_file).unwrap();
                let mut inputs = vec![input_file];
                inputs.extend(self.link_masm_modules.into_iter().map(|(path, content)| {
                    let path = path.to_string();
                    InputFile::new(
                        midenc_session::FileType::Masm,
                        InputType::Stdin {
                            name: path.into(),
                            input: content.into_bytes(),
                        },
                    )
                }));

                let context = setup::default_context(inputs, &self.midenc_flags);
                let session = context.session_rc();
                CompilerTest {
                    config: self.config,
                    session,
                    context,
                    artifact_name: config.name,
                    entrypoint: self.entrypoint,
                    ..Default::default()
                }
            }
        }
    }
}

/// Convenience builders
impl CompilerTestBuilder {
    /// Compile the Rust project using cargo-miden
    pub fn rust_source_cargo_miden(
        cargo_project_folder: impl AsRef<Path>,
        config: WasmTranslationConfig,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let name = cargo_project_folder
            .as_ref()
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or("".to_string());
        let mut builder = CompilerTestBuilder::new(CompilerTestInputType::CargoMiden(
            CargoTest::new(name, cargo_project_folder.as_ref().to_path_buf()),
        ));
        builder.with_wasm_translation_config(config);
        builder.with_midenc_flags(["--target".into(), "rollup".into()]);
        builder.with_midenc_flags(midenc_flags);
        builder
    }

    /// Set the Rust source code to compile a library Cargo project to Wasm module
    pub fn rust_source_cargo_lib(
        cargo_project_folder: impl AsRef<Path>,
        artifact_name: impl Into<Cow<'static, str>>,
        is_build_std: bool,
        entry_func_name: Option<Cow<'static, str>>,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let cargo_project_folder = cargo_project_folder.as_ref().to_path_buf();
        let config =
            CargoTest::new(artifact_name, cargo_project_folder).with_build_std(is_build_std);
        let mut builder = CompilerTestBuilder::new(match entry_func_name {
            Some(entry) => config.with_entrypoint(entry),
            None => config,
        });
        builder.with_midenc_flags(midenc_flags);
        builder
    }

    /// Set the Rust source code to compile using a Cargo project and binary bundle name
    pub fn rust_source_cargo(
        cargo_project_folder: impl AsRef<Path>,
        artifact_name: impl Into<Cow<'static, str>>,
        entrypoint: impl Into<Cow<'static, str>>,
    ) -> Self {
        let temp_dir = std::env::temp_dir();
        let target_dir = temp_dir.join(cargo_project_folder.as_ref());
        let project_dir = Path::new("../rust-apps-wasm")
            .join(cargo_project_folder.as_ref())
            .canonicalize()
            .unwrap_or_else(|_| {
                panic!(
                    "unknown project folder: ../rust-apps-wasm/{}",
                    cargo_project_folder.as_ref().display()
                )
            });
        let config = CargoTest::new(artifact_name, project_dir)
            .with_build_alloc(true)
            .with_target_dir(target_dir)
            .with_target("wasm32-unknown-unknown")
            .with_entrypoint(entrypoint);
        CompilerTestBuilder::new(config)
    }

    /// Set the Rust source code to compile
    pub fn rust_source_program(rust_source: impl Into<Cow<'static, str>>) -> Self {
        let rust_source = rust_source.into();
        let name = format!("test_rust_{}", hash_string(&rust_source));
        CompilerTestBuilder::new(RustcTest::new(name, rust_source))
    }

    /// Set the Rust source code to compile and add a binary operation test
    pub fn rust_fn_body(
        rust_source: &str,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let name = format!("test_rust_{}", hash_string(rust_source));
        Self::rust_fn_body_with_artifact_name(name, rust_source, midenc_flags)
    }

    /// Set the Rust source code to compile and add a binary operation test
    pub fn rust_fn_body_with_artifact_name(
        name: impl Into<Cow<'static, str>>,
        rust_source: &str,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let rust_source = format!(
            r#"
            #![no_std]
            #![no_main]

            #[panic_handler]
            fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
                core::arch::wasm32::unreachable()
            }}

            #[no_mangle]
            pub extern "C" fn entrypoint{}
            "#,
            rust_source
        );
        let name = name.into();
        let module_name = Ident::with_empty_span(Symbol::intern(&name));
        let mut builder = CompilerTestBuilder::new(RustcTest::new(name, rust_source));
        builder.with_midenc_flags(midenc_flags).with_entrypoint(FunctionIdent {
            module: module_name,
            function: Ident::with_empty_span(Symbol::intern("entrypoint")),
        });
        builder
    }

    /// Set the Rust source code to compile with `miden-stdlib-sys` (stdlib + intrinsics)
    pub fn rust_fn_body_with_stdlib_sys(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let name = name.into();
        let stdlib_sys_path = stdlib_sys_crate_path();
        let sdk_alloc_path = sdk_alloc_crate_path();
        let proj = project(name.as_ref())
            .file(
                "Cargo.toml",
                format!(
                    r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2021"
                authors = []

                [dependencies]
                miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
                miden-stdlib-sys = {{ path = "{stdlib_sys_path}" }}

                [lib]
                crate-type = ["cdylib"]

                [profile.release]
                panic = "abort"
                # optimize for size
                opt-level = "z"
                debug = true
            "#,
                    sdk_alloc_path = sdk_alloc_path.display(),
                    stdlib_sys_path = stdlib_sys_path.display(),
                )
                .as_str(),
            )
            .file(
                "src/lib.rs",
                format!(
                    r#"
                #![no_std]
                #![no_main]
                #![allow(unused_imports)]

                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
                    core::arch::wasm32::unreachable()
                }}


                #[global_allocator]
                static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

                extern crate miden_stdlib_sys;
                use miden_stdlib_sys::*;

                #[no_mangle]
                #[allow(improper_ctypes_definitions)]
                pub extern "C" fn entrypoint{}
            "#,
                    source
                )
                .as_str(),
            )
            .build();
        Self::rust_source_cargo_lib(
            proj.root(),
            name,
            is_build_std,
            Some("entrypoint".into()),
            midenc_flags,
        )
    }

    /// Set the Rust source code to compile with `miden-sdk` (sdk + intrinsics)
    pub fn rust_source_with_sdk(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        entrypoint: Option<Cow<'static, str>>,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let name = name.into();
        let sdk_path = sdk_crate_path();
        let sdk_alloc_path = sdk_alloc_crate_path();
        let proj = project(name.as_ref())
            .file(
                "Cargo.toml",
                format!(
                    r#"[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
authors = []

[dependencies]
miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
miden = {{ path = "{sdk_path}" }}

[lib]
crate-type = ["cdylib"]

[profile.release]
panic = "abort"
# optimize for size
opt-level = "z"
debug = true
"#,
                    sdk_path = sdk_path.display(),
                    sdk_alloc_path = sdk_alloc_path.display(),
                )
                .as_str(),
            )
            .file(
                "src/lib.rs",
                format!(
                    r#"#![no_std]
#![no_main]
#![allow(unused_imports)]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
    core::arch::wasm32::unreachable()
}}


#[global_allocator]
static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

extern crate miden;
use miden::*;

extern crate alloc;
use alloc::vec::Vec;

{}
"#,
                    source
                )
                .as_str(),
            )
            .build();

        Self::rust_source_cargo_lib(proj.root(), name, is_build_std, entrypoint, midenc_flags)
    }

    /// Like `rust_source_with_sdk`, but expects the source code to be the body of a function
    /// which will be used as the entrypoint.
    pub fn rust_fn_body_with_sdk(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        let source = format!("#[no_mangle]\npub extern \"C\" fn entrypoint{source}");
        Self::rust_source_with_sdk(
            name,
            &source,
            is_build_std,
            Some("entrypoint".into()),
            midenc_flags,
        )
    }
}

/// Compile to different stages (e.g. Wasm, IR, MASM) and compare the results against expected
/// output
pub struct CompilerTest {
    /// The Wasm translation configuration
    pub config: WasmTranslationConfig,
    /// The compiler session
    pub session: Rc<Session>,
    /// The compiler context
    pub context: Rc<Context>,
    /// The artifact name from which this test is derived
    artifact_name: Cow<'static, str>,
    /// The entrypoint function to use when building the IR
    entrypoint: Option<FunctionIdent>,
    /// The compiled IR
    hir: Option<midenc_compile::LinkOutput>,
    /// The MASM source code
    masm_src: Option<String>,
    /// The compiled IR MASM program
    ir_masm_program: Option<Result<Arc<midenc_codegen_masm::MasmComponent>, String>>,
    /// The compiled package containing a program executable by the VM
    package: Option<Result<Arc<miden_mast_package::Package>, String>>,
}

impl fmt::Debug for CompilerTest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CompilerTest")
            .field("config", &self.config)
            .field("session", &self.session)
            .field("artifact_name", &self.artifact_name)
            .field("entrypoint", &self.entrypoint)
            .field_with("hir", |f| match self.hir.as_ref() {
                None => f.debug_tuple("None").finish(),
                Some(link_output) => {
                    f.debug_tuple("Some").field(&link_output.component.borrow().id()).finish()
                }
            })
            .finish_non_exhaustive()
    }
}

impl Default for CompilerTest {
    fn default() -> Self {
        let context = setup::dummy_context(&[]);
        let session = context.session_rc();
        Self {
            config: WasmTranslationConfig::default(),
            session,
            context,
            artifact_name: "unknown".into(),
            entrypoint: None,
            hir: None,
            masm_src: None,
            ir_masm_program: None,
            package: None,
        }
    }
}

impl CompilerTest {
    /// Return the name of the artifact this test is derived from
    pub fn artifact_name(&self) -> &str {
        self.artifact_name.as_ref()
    }

    /// Return the entrypoint for this test, if specified
    pub fn entrypoint(&self) -> Option<FunctionIdent> {
        self.entrypoint
    }

    /// Compile the Rust project using cargo-miden
    pub fn rust_source_cargo_miden(
        cargo_project_folder: impl AsRef<Path>,
        config: WasmTranslationConfig,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_source_cargo_miden(cargo_project_folder, config, midenc_flags)
            .build()
    }

    /// Set the Rust source code to compile a library Cargo project to Wasm module
    pub fn rust_source_cargo_lib(
        cargo_project_folder: impl AsRef<Path>,
        artifact_name: impl Into<Cow<'static, str>>,
        is_build_std: bool,
        entry_func_name: Option<Cow<'static, str>>,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_source_cargo_lib(
            cargo_project_folder,
            artifact_name,
            is_build_std,
            entry_func_name,
            midenc_flags,
        )
        .build()
    }

    /// Set the Rust source code to compile using a Cargo project and binary bundle name
    pub fn rust_source_cargo(
        cargo_project_folder: &str,
        artifact_name: impl Into<Cow<'static, str>>,
        entrypoint: impl Into<Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_source_cargo(cargo_project_folder, artifact_name, entrypoint)
            .build()
    }

    /// Set the Rust source code to compile
    pub fn rust_source_program(rust_source: impl Into<Cow<'static, str>>) -> Self {
        CompilerTestBuilder::rust_source_program(rust_source).build()
    }

    /// Set the Rust source code to compile and add a binary operation test
    pub fn rust_fn_body(
        source: &str,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_fn_body(source, midenc_flags).build()
    }

    /// Set the Rust source code to compile and add a binary operation test
    pub fn rust_fn_body_with_artifact_name(
        name: impl Into<Cow<'static, str>>,
        rust_source: &str,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_fn_body_with_artifact_name(name, rust_source, midenc_flags)
            .build()
    }

    /// Set the Rust source code to compile with `miden-stdlib-sys` (stdlib + intrinsics)
    pub fn rust_fn_body_with_stdlib_sys(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_fn_body_with_stdlib_sys(name, source, is_build_std, midenc_flags)
            .build()
    }

    /// Set the Rust source code to compile with `miden-sdk` (sdk + intrinsics)
    pub fn rust_source_with_sdk(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        entrypoint: Option<Cow<'static, str>>,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_source_with_sdk(
            name,
            source,
            is_build_std,
            entrypoint,
            midenc_flags,
        )
        .build()
    }

    /// Like [Self::rust_source_with_sdk], but expects the source code to be a function parameter
    /// list and body, rather than arbitrary source code.
    ///
    /// NOTE: It is valid to append additional sources _after_ the closing brace of the function
    /// body.
    pub fn rust_fn_body_with_sdk(
        name: impl Into<Cow<'static, str>>,
        source: &str,
        is_build_std: bool,
        midenc_flags: impl IntoIterator<Item = Cow<'static, str>>,
    ) -> Self {
        CompilerTestBuilder::rust_fn_body_with_sdk(name, source, is_build_std, midenc_flags).build()
    }

    /// Compare the compiled Wasm against the expected output
    pub fn expect_wasm(&self, expected_wat_file: expect_test::ExpectFile) {
        let wasm_bytes = self.wasm_bytes();
        let wat = demangle(wasm_to_wat(&wasm_bytes));
        expected_wat_file.assert_eq(&wat);
    }

    /// Get the translated IR component, translating the Wasm if it has not been done yet
    pub fn hir(&mut self) -> builtin::ComponentRef {
        self.link_output().component
    }

    /// Get a reference to the full IR linker output, translating the Wasm if needed.
    pub fn link_output(&mut self) -> &midenc_compile::LinkOutput {
        use midenc_compile::compile_to_optimized_hir;

        if self.hir.is_none() {
            let link_output = compile_to_optimized_hir(self.context.clone())
                .map_err(format_report)
                .expect("failed to translate wasm to hir component");
            self.hir = Some(link_output);
        }
        self.hir.as_ref().unwrap()
    }

    /// Compare the compiled(optimized) IR against the expected output
    pub fn expect_ir(&mut self, expected_hir_file: expect_test::ExpectFile) {
        use midenc_hir::Op;

        let ir = demangle(self.hir().borrow().as_operation().to_string());
        expected_hir_file.assert_eq(&ir);
    }

    /// Compare the compiled(unoptimized) IR against the expected output
    pub fn expect_ir_unoptimized(&mut self, expected_hir_file: expect_test::ExpectFile) {
        let component = compile_to_unoptimized_hir(self.context.clone())
            .map_err(format_report)
            .expect("failed to translate wasm to hir component")
            .component;

        let ir = demangle(component.borrow().as_operation().to_string());
        expected_hir_file.assert_eq(&ir);
    }

    /// Compare the compiled MASM against the expected output
    pub fn expect_masm(&mut self, expected_masm_file: expect_test::ExpectFile) {
        let program = demangle(self.masm_src().as_str());
        std::println!("{program}");
        expected_masm_file.assert_eq(&program);
    }

    /// Get the compiled IR MASM program
    pub fn ir_masm_program(&mut self) -> Arc<midenc_codegen_masm::MasmComponent> {
        if self.ir_masm_program.is_none() {
            self.compile_wasm_to_masm_program().unwrap();
        }
        match self.ir_masm_program.as_ref().unwrap().as_ref() {
            Ok(component) => component.clone(),
            Err(msg) => panic!("{msg}"),
        }
    }

    /// Get the compiled [miden_mast_package::Package]
    pub fn compiled_package(&mut self) -> Arc<miden_mast_package::Package> {
        if self.package.is_none() {
            self.compile_wasm_to_masm_program().unwrap();
        }
        match self.package.as_ref().unwrap().as_ref() {
            Ok(prog) => prog.clone(),
            Err(msg) => panic!("{msg}"),
        }
    }

    /// Get the MASM source code
    pub fn masm_src(&mut self) -> String {
        if self.masm_src.is_none() {
            if let Err(err) = self.compile_wasm_to_masm_program() {
                panic!("{err}");
            }
        }
        self.masm_src.clone().unwrap()
    }

    /// The compiled Wasm component/module
    fn wasm_bytes(&self) -> Vec<u8> {
        match &self.session.inputs[0].file {
            InputType::Real(file_path) => fs::read(file_path)
                .unwrap_or_else(|_| panic!("Failed to read Wasm file: {}", file_path.display())),
            InputType::Stdin { name: _, input } => input.clone(),
        }
    }

    /// Assemble the Wasm input to Miden Assembly
    ///
    /// If the Wasm has already been translated to the IR, it is just assembled, otherwise the
    /// Wasm will be translated to the IR, caching the translation results, and then assembled.
    pub(crate) fn compile_wasm_to_masm_program(&mut self) -> Result<(), String> {
        use midenc_compile::CodegenOutput;
        use midenc_hir::Context;

        let mut src = None;
        let mut masm_program = None;
        let mut stage = |output: CodegenOutput, _context: Rc<Context>| {
            src = Some(output.component.to_string());
            if output.component.entrypoint.is_some() {
                masm_program = Some(Arc::clone(&output.component));
            }
            Ok(output)
        };

        let link_output = self.link_output().clone();
        let package = compile_link_output_to_masm_with_pre_assembly_stage(link_output, &mut stage)
            .map_err(format_report)?
            .unwrap_mast();

        assert!(src.is_some(), "failed to pretty print masm artifact");
        self.masm_src = src;
        self.ir_masm_program = masm_program.map(Ok);
        self.package = Some(Ok(Arc::new(package)));
        Ok(())
    }
}

fn stdlib_sys_crate_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    cwd.parent().unwrap().parent().unwrap().join("sdk").join("stdlib-sys")
}

pub fn sdk_alloc_crate_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    cwd.parent().unwrap().parent().unwrap().join("sdk").join("alloc")
}

pub fn sdk_crate_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    cwd.parent().unwrap().parent().unwrap().join("sdk").join("sdk")
}

/// Get the directory for the top-level workspace
fn get_workspace_dir() -> String {
    // Get the directory for the integration test suite project
    let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or(std::env::current_dir().unwrap().to_str().unwrap().to_string());
    let cargo_manifest_dir_path = Path::new(&cargo_manifest_dir);
    // "Exit" the integration test suite project directory to the compiler workspace directory
    // i.e. out of the `tests/integration` directory
    let compiler_workspace_dir =
        cargo_manifest_dir_path.parent().unwrap().parent().unwrap().to_str().unwrap();
    compiler_workspace_dir.to_string()
}

fn report_cargo_error(child: std::process::Child) {
    eprintln!("pwd: {:?}", std::env::current_dir().unwrap());
    let mut stderr = Vec::new();
    child.stderr.unwrap().read_exact(&mut stderr).expect("Failed to read stderr");
    let stderr = String::from_utf8(stderr).expect("Failed to parse stderr");
    eprintln!("stderr: {}", stderr);
    panic!("Rust to Wasm compilation failed!");
}

fn find_wasm_artifacts(child: &mut std::process::Child) -> Vec<std::path::PathBuf> {
    let mut wasm_artifacts = Vec::new();
    let reader = std::io::BufReader::new(child.stdout.take().unwrap());
    for message in cargo_metadata::Message::parse_stream(reader) {
        if let cargo_metadata::Message::CompilerArtifact(artifact) =
            message.expect("Failed to parse cargo metadata")
        {
            // find the Wasm artifact in artifact.filenames
            for filename in artifact.filenames {
                if filename.as_str().ends_with(".wasm") {
                    wasm_artifacts.push(filename.into_std_path_buf());
                }
            }
        }
    }
    wasm_artifacts
}

fn wasm_to_wat(wasm_bytes: &[u8]) -> String {
    // Disable printing of the various custom sections, e.g. "producers", either because they
    // contain strings which are highly variable (but not important), or because they are debug info
    // related.
    struct NoCustomSectionsPrinter<T: wasmprinter::Print>(T);
    impl<T: wasmprinter::Print> wasmprinter::Print for NoCustomSectionsPrinter<T> {
        fn write_str(&mut self, s: &str) -> std::io::Result<()> {
            self.0.write_str(s)
        }

        fn newline(&mut self) -> std::io::Result<()> {
            self.0.newline()
        }

        fn start_line(&mut self, binary_offset: Option<usize>) {
            self.0.start_line(binary_offset);
        }

        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> std::io::Result<()> {
            self.0.write_fmt(args)
        }

        fn print_custom_section(
            &mut self,
            name: &str,
            binary_offset: usize,
            data: &[u8],
        ) -> std::io::Result<bool> {
            match name {
                "producers" | "target_features" => Ok(true),
                debug if debug.starts_with(".debug") => Ok(true),
                _ => self.0.print_custom_section(name, binary_offset, data),
            }
        }

        fn start_literal(&mut self) -> std::io::Result<()> {
            self.0.start_literal()
        }

        fn start_name(&mut self) -> std::io::Result<()> {
            self.0.start_name()
        }

        fn start_keyword(&mut self) -> std::io::Result<()> {
            self.0.start_keyword()
        }

        fn start_type(&mut self) -> std::io::Result<()> {
            self.0.start_type()
        }

        fn start_comment(&mut self) -> std::io::Result<()> {
            self.0.start_comment()
        }

        fn reset_color(&mut self) -> std::io::Result<()> {
            self.0.reset_color()
        }

        fn supports_async_color(&self) -> bool {
            self.0.supports_async_color()
        }
    }

    let mut wat = String::with_capacity(1024);
    let config = wasmprinter::Config::new();
    let mut wasm_printer = NoCustomSectionsPrinter(wasmprinter::PrintFmtWrite(&mut wat));
    config.print(wasm_bytes, &mut wasm_printer).unwrap();
    wat
}

fn hash_string(inputs: &str) -> String {
    let hash = <sha2::Sha256 as sha2::Digest>::digest(inputs.as_bytes());
    format!("{:x}", hash)
}
