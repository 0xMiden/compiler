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
    mut midenc_args: Vec<String>,
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

    let mut args: Vec<String> = vec![
        "--output-dir".to_string(),
        output_folder.to_str().unwrap().to_string(),
        "-o".to_string(),
        output_file.to_str().unwrap().to_string(),
        "--verbose".to_string(),
    ];
    args.append(&mut midenc_args);

    let session = Rc::new(Compiler::new_session([input], None, args));
    let context = Rc::new(Context::new(session));
    midenc_compile::compile(context.clone())?;
    Ok(output_file)
}
