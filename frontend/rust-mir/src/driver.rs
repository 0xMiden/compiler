//! Drives rustc over one Rust source file and translates the MIR it produces.
//!
//! The frontend does not spawn a rustc process. It links against rustc and runs the compiler
//! in-process, thus the MIR stays in memory and no MIR text is parsed.

use std::{path::Path, process::Command, rc::Rc};

use midenc_hir::{Context, Report};

use crate::{FrontendOutput, RustMirTranslationConfig, translator};

/// The stack size of the thread that runs rustc.
///
/// rustc needs a large stack, thus the default thread stack is not enough.
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Translates every local, non-generic function of the Rust source file into a Miden IR
/// component.
///
/// The process that calls this function must run inside a directory that resolves to the
/// toolchain this frontend was built with, because the rustc sysroot comes from
/// `rustc --print sysroot`.
pub fn translate(
    source_path: &Path,
    config: &RustMirTranslationConfig,
    context: Rc<Context>,
) -> Result<FrontendOutput, Report> {
    if !source_path.is_file() {
        return Err(Report::msg(format!(
            "rust mir frontend: input file does not exist: {}",
            source_path.display()
        )));
    }

    let crate_name = match config.crate_name.as_deref() {
        Some(name) => name.to_string(),
        None => default_crate_name(source_path)?,
    };

    // The temporary directory must outlive the rustc run, thus it is bound here.
    let temp_out_dir = match config.out_dir {
        Some(_) => None,
        None => Some(tempfile::tempdir().map_err(|err| {
            Report::msg(format!("rust mir frontend: could not create a temporary directory: {err}"))
        })?),
    };
    let out_dir = match config.out_dir.as_deref() {
        Some(dir) => dir.to_path_buf(),
        None => temp_out_dir.as_ref().unwrap().path().to_path_buf(),
    };

    let args = rustc_args(source_path, config, &crate_name, &out_dir)?;
    let component = run_translation(args, crate_name, context)?;

    Ok(FrontendOutput { component })
}

/// Moves a value to the thread that runs rustc, and moves the result back.
///
/// Miden IR entities and the [Context] are not [Send], but the rustc driver requires its
/// callback and the callback result to be [Send].
struct ThreadHandoff<T>(T);

// SAFETY: The caller builds the value, hands it to exactly one worker thread, and then blocks
// in `JoinHandle::join` until that thread stops. While the worker runs, the caller thread is
// blocked, thus it cannot touch aliases of the shared state inside the value (for example
// other `Rc` clones of the same `Context`), and safe code cannot have moved such aliases to a
// third thread because the aliases are not `Send`. Therefore only one thread accesses the
// shared allocations at a time, and the join gives the happens-before relation that the
// non-atomic reference counts need. The same argument covers the second hop into the thread
// that the rustc driver spawns, because the driver joins that thread before it returns.
unsafe impl<T> Send for ThreadHandoff<T> {}

impl<T> ThreadHandoff<T> {
    /// Takes the value out of the handoff.
    ///
    /// A closure that reads the field directly captures the field alone, which drops the [Send]
    /// property of the handoff. A method call captures the whole handoff instead.
    fn into_inner(self) -> T {
        self.0
    }
}

/// Runs rustc over the given argument vector and translates the MIR of the compiled crate.
fn run_translation(
    args: Vec<String>,
    module_name: String,
    context: Rc<Context>,
) -> Result<midenc_hir::dialects::builtin::ComponentRef, Report> {
    let inputs = ThreadHandoff((module_name, context));

    let worker = std::thread::Builder::new()
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || {
            let inputs = inputs;
            rustc_public::run!(&args, move || {
                let (module_name, context) = inputs.into_inner();
                let result = translator::translate_crate(&module_name, context);
                ControlFlow::<(), _>::Continue(ThreadHandoff(result))
            })
        })
        .map_err(|err| {
            Report::msg(format!("rust mir frontend: could not start the rustc thread: {err}"))
        })?;

    let outcome = worker
        .join()
        .map_err(|_| Report::msg("rust mir frontend: the rustc thread panicked"))?;

    match outcome {
        Ok(result) => result.into_inner(),
        Err(rustc_public::CompilerError::Failed) => {
            Err(Report::msg("rust mir frontend: rustc rejected the input file"))
        }
        Err(rustc_public::CompilerError::Skipped) => {
            Err(Report::msg("rust mir frontend: rustc did not compile the input file"))
        }
        Err(rustc_public::CompilerError::Interrupted(())) => {
            Err(Report::msg("rust mir frontend: the translation was interrupted"))
        }
    }
}

/// Builds the rustc argument vector for the given input file.
fn rustc_args(
    source_path: &Path,
    config: &RustMirTranslationConfig,
    crate_name: &str,
    out_dir: &Path,
) -> Result<Vec<String>, Report> {
    let sysroot = sysroot()?;

    Ok(vec![
        // The rustc driver expects the program name in the first position.
        "rustc".to_string(),
        format!("--edition={}", config.edition),
        "--crate-type=rlib".to_string(),
        format!("--crate-name={crate_name}"),
        // Metadata is the only output that is needed. It stops rustc before code generation.
        "--emit=metadata".to_string(),
        format!("--target={}", config.target),
        // Overflow checks turn every addition into a checked addition and a branch. The
        // frontend does not support that lowering yet.
        "-Coverflow-checks=off".to_string(),
        // MIR optimizations make the MIR shape depend on the rustc version.
        "-Zmir-opt-level=0".to_string(),
        format!("--out-dir={}", out_dir.display()),
        format!("--sysroot={sysroot}"),
        source_path.display().to_string(),
    ])
}

/// Returns the sysroot of the rustc toolchain that this frontend was built with.
fn sysroot() -> Result<String, Report> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(&rustc)
        .args(["--print", "sysroot"])
        .output()
        .map_err(|err| Report::msg(format!("rust mir frontend: could not run rustc: {err}")))?;
    if !output.status.success() {
        return Err(Report::msg("rust mir frontend: `rustc --print sysroot` failed"));
    }
    let sysroot = String::from_utf8(output.stdout).map_err(|_| {
        Report::msg("rust mir frontend: `rustc --print sysroot` did not print valid UTF-8")
    })?;
    Ok(sysroot.trim().to_string())
}

/// Derives a crate name from the file stem of the input file.
fn default_crate_name(source_path: &Path) -> Result<String, Report> {
    let stem = source_path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
        Report::msg(format!(
            "rust mir frontend: cannot derive a crate name from {}",
            source_path.display()
        ))
    })?;
    Ok(stem.replace(['-', '.'], "_"))
}
