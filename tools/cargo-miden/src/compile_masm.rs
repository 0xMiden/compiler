use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use midenc_compile::{Compiler, Context};
use midenc_session::{
    diagnostics::{IntoDiagnostic, Report, WrapErr},
    InputFile, OutputType,
};

pub fn wasm_to_masm(
    wasm_file_path: &Path,
    output_folder: &Path,
    is_bin: bool,
    dependency_paths: &[PathBuf], // New parameter
) -> Result<PathBuf, Report> {
    if !output_folder.exists() {
        return Err(Report::msg(format!(
            "MASM output folder '{}' does not exist.",
            output_folder.to_str().unwrap()
        )));
    }
    log::debug!(
        "Compiling '{}' Wasm to '{}' directory with midenc ...",
        wasm_file_path.to_str().unwrap(),
        &output_folder.to_str().unwrap()
    );
    let input = InputFile::from_path(wasm_file_path)
        .into_diagnostic()
        .wrap_err("Invalid input file")?;
    let masm_file_name = wasm_file_path
        .file_stem()
        .expect("invalid wasm file path: no file stem")
        .to_str()
        .unwrap();
    let output_file =
        output_folder.join(masm_file_name).with_extension(OutputType::Masp.extension());
    let project_type = if is_bin { "--exe" } else { "--lib" };
    let entrypoint_opt = format!("--entrypoint={masm_file_name}::entrypoint");
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "--output-dir".as_ref(),
        output_folder.as_os_str(),
        "-o".as_ref(),
        output_file.as_os_str(),
        project_type.as_ref(),
        "--verbose".as_ref(),
        "--target".as_ref(),
        "rollup".as_ref(),
    ];

    if is_bin {
        args.push(entrypoint_opt.as_ref());
    }

    // Add dependency linker arguments (Step 3.3)
    for dep_path in dependency_paths {
        args.push("--link-library".as_ref());
        // Ensure the path is valid OsStr
        args.push(dep_path.as_os_str());
    }

    let session = Rc::new(Compiler::new_session([input], None, args));
    let context = Rc::new(Context::new(session));
    midenc_compile::compile(context.clone())?;
    Ok(output_file)
}
