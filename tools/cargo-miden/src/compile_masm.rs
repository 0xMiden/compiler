use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use midenc_compile::Compiler;
use midenc_session::{
    diagnostics::{IntoDiagnostic, Report, WrapErr},
    InputFile, OutputType,
};

pub fn wasm_to_masm(
    wasm_file_path: &Path,
    output_folder: &Path,
    is_bin: bool,
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
    let args: Vec<&std::ffi::OsStr> = vec![
        "--output-dir".as_ref(),
        output_folder.as_os_str(),
        "-o".as_ref(),
        output_file.as_os_str(),
        project_type.as_ref(),
        "--verbose".as_ref(),
        "--target".as_ref(),
        "rollup".as_ref(),
        entrypoint_opt.as_ref(),
    ];
    let session = Rc::new(Compiler::new_session([input], None, args));
    midenc_compile::compile(session.clone())?;
    Ok(output_file)
}
