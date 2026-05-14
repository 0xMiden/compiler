use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use midenc_compile::Context;
use midenc_session::{
    InputFile, OutputType,
    diagnostics::{IntoDiagnostic, Report, WrapErr},
};

pub fn wasm_to_masm(
    wasm_file_path: &Path,
    output_folder: &Path,
    mut options: Box<midenc_session::Options>,
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

    options.output_dir = Some(output_folder.to_path_buf());
    options.output_file = Some(midenc_session::OutputFile::Real(output_file.clone()));
    options.diagnostics.verbosity = midenc_session::Verbosity::Debug;

    log::debug!("midenc options: {options:#?}");
    let session = options.into_session(input, None, None).map(Rc::new)?;
    let context = Rc::new(Context::new(session));

    println!("Creating Miden package {}", output_file.display());

    midenc_compile::compile(context.clone())?;

    Ok(output_file)
}
